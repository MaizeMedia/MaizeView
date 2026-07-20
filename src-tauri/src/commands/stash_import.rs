//! Import curated metadata from a local Stash `stash-go.sqlite` (read-only).
//! Matches our scenes by oshash / md5 / phash fingerprints. Never moves files (ADR-013).

use sqlx::{sqlite::SqliteConnectOptions, SqlitePool};
use std::collections::HashMap;
use tauri::State;

use crate::{
    commands::err,
    models::{new_id, now},
    AppState,
};

#[derive(Debug, Clone, serde::Serialize)]
pub struct ImportStashResult {
    pub matched: u64,
    pub updated: u64,
    pub skipped: u64,
    pub errors: u64,
    pub last_error: Option<String>,
}

#[derive(Debug)]
struct StashSceneMeta {
    title: Option<String>,
    details: Option<String>,
    studio: Option<String>,
    performers: Vec<String>,
    tags: Vec<String>,
}

/// Open Stash DB read-only and apply metadata onto fingerprint-matched MaizeView scenes.
#[tauri::command]
pub async fn import_stash_metadata(
    state: State<'_, AppState>,
    stash_db_path: String,
) -> Result<ImportStashResult, String> {
    let path = stash_db_path.trim();
    if path.is_empty() {
        return Err("stash database path is required".into());
    }
    if !std::path::Path::new(path).is_file() {
        return Err(format!("file not found: {path}"));
    }

    let opts = SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .foreign_keys(false);

    let stash = SqlitePool::connect_with(opts)
        .await
        .map_err(|e| format!("open stash db: {e}"))?;

    // fingerprint value (lowercase) → stash scene id
    let fp_rows: Vec<(i64, String, String)> = sqlx::query_as(
        r#"
        SELECT sf.scene_id, lower(ff.type), lower(ff.fingerprint)
        FROM files_fingerprints ff
        JOIN scenes_files sf ON sf.file_id = ff.file_id
        WHERE lower(ff.type) IN ('oshash', 'md5', 'phash')
          AND ff.fingerprint IS NOT NULL
          AND length(ff.fingerprint) > 0
        "#,
    )
    .fetch_all(&stash)
    .await
    .map_err(|e| {
        format!("query stash fingerprints failed (is this a modern stash-go.sqlite?): {e}")
    })?;

    let mut fp_to_scene: HashMap<(String, String), i64> = HashMap::new();
    for (scene_id, hash_type, value) in fp_rows {
        fp_to_scene.insert((hash_type, value), scene_id);
    }

    let our_fps: Vec<(String, String, String)> = sqlx::query_as(
        r#"
        SELECT f.scene_id, lower(fp.hash_type), lower(fp.value)
        FROM fingerprints fp
        JOIN files f ON f.id = fp.file_id
        WHERE lower(fp.hash_type) IN ('oshash', 'md5', 'phash')
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(err)?;

    let mut scene_to_stash: HashMap<String, i64> = HashMap::new();
    for (scene_id, hash_type, value) in &our_fps {
        if let Some(stash_id) = fp_to_scene.get(&(hash_type.clone(), value.clone())) {
            scene_to_stash.entry(scene_id.clone()).or_insert(*stash_id);
        }
    }

    let mut matched = 0u64;
    let mut updated = 0u64;
    let mut skipped = 0u64;
    let mut errors = 0u64;
    let mut last_error = None;

    for (scene_id, stash_scene_id) in &scene_to_stash {
        matched += 1;
        match load_stash_scene(&stash, *stash_scene_id).await {
            Ok(meta) => match apply_stash_meta(&state.pool, scene_id, &meta).await {
                Ok(true) => updated += 1,
                Ok(false) => skipped += 1,
                Err(e) => {
                    errors += 1;
                    last_error = Some(e);
                }
            },
            Err(e) => {
                errors += 1;
                last_error = Some(e);
            }
        }
    }

    stash.close().await;

    Ok(ImportStashResult {
        matched,
        updated,
        skipped,
        errors,
        last_error,
    })
}

async fn load_stash_scene(stash: &SqlitePool, scene_id: i64) -> Result<StashSceneMeta, String> {
    let row: Option<(Option<String>, Option<String>, Option<i64>)> =
        sqlx::query_as("SELECT title, details, studio_id FROM scenes WHERE id = ?")
            .bind(scene_id)
            .fetch_optional(stash)
            .await
            .map_err(|e| e.to_string())?;

    let (title, details, studio_id) =
        row.ok_or_else(|| format!("stash scene {scene_id} missing"))?;

    let studio = if let Some(sid) = studio_id {
        sqlx::query_scalar::<_, String>("SELECT name FROM studios WHERE id = ?")
            .bind(sid)
            .fetch_optional(stash)
            .await
            .map_err(|e| e.to_string())?
    } else {
        None
    };

    let performers: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT p.name FROM performers p
        JOIN performers_scenes ps ON ps.performer_id = p.id
        WHERE ps.scene_id = ?
        ORDER BY p.name
        "#,
    )
    .bind(scene_id)
    .fetch_all(stash)
    .await
    .map_err(|e| e.to_string())?;

    let tags: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT t.name FROM tags t
        JOIN scenes_tags st ON st.tag_id = t.id
        WHERE st.scene_id = ?
        ORDER BY t.name
        "#,
    )
    .bind(scene_id)
    .fetch_all(stash)
    .await
    .map_err(|e| e.to_string())?;

    Ok(StashSceneMeta {
        title,
        details,
        studio,
        performers,
        tags,
    })
}

/// Returns true if anything was written.
async fn apply_stash_meta(
    pool: &SqlitePool,
    scene_id: &str,
    meta: &StashSceneMeta,
) -> Result<bool, String> {
    let ts = now().to_rfc3339();
    let mut changed = false;

    if let Some(title) = meta
        .title
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        sqlx::query(
            "UPDATE scenes SET title = ?, title_source = 'stash_import', updated_at = ? WHERE id = ?",
        )
        .bind(title)
        .bind(&ts)
        .bind(scene_id)
        .execute(pool)
        .await
        .map_err(err)?;
        changed = true;
    }

    if let Some(details) = meta
        .details
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        sqlx::query(
            "UPDATE scenes SET details = ?, details_source = 'stash_import', updated_at = ? WHERE id = ?",
        )
        .bind(details)
        .bind(&ts)
        .bind(scene_id)
        .execute(pool)
        .await
        .map_err(err)?;
        changed = true;
    }

    if let Some(studio_name) = meta
        .studio
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        let studio_id = find_or_create_studio(pool, studio_name).await?;
        sqlx::query("UPDATE scenes SET studio_id = ?, updated_at = ? WHERE id = ?")
            .bind(&studio_id)
            .bind(&ts)
            .bind(scene_id)
            .execute(pool)
            .await
            .map_err(err)?;
        changed = true;
    }

    for name in &meta.performers {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let pid = find_or_create_performer(pool, name).await?;
        let r = sqlx::query(
            "INSERT OR IGNORE INTO scene_performers (scene_id, performer_id) VALUES (?, ?)",
        )
        .bind(scene_id)
        .bind(&pid)
        .execute(pool)
        .await
        .map_err(err)?;
        if r.rows_affected() > 0 {
            changed = true;
        }
    }

    for name in &meta.tags {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let tid = find_or_create_tag(pool, name).await?;
        let r = sqlx::query("INSERT OR IGNORE INTO scene_tags (scene_id, tag_id) VALUES (?, ?)")
            .bind(scene_id)
            .bind(&tid)
            .execute(pool)
            .await
            .map_err(err)?;
        if r.rows_affected() > 0 {
            changed = true;
        }
    }

    Ok(changed)
}

async fn find_or_create_studio(pool: &SqlitePool, name: &str) -> Result<String, String> {
    let existing: Option<(String,)> =
        sqlx::query_as("SELECT id FROM studios WHERE name = ? COLLATE NOCASE")
            .bind(name)
            .fetch_optional(pool)
            .await
            .map_err(err)?;
    if let Some((id,)) = existing {
        return Ok(id);
    }
    let id = new_id();
    sqlx::query("INSERT INTO studios (id, name, created_at) VALUES (?, ?, ?)")
        .bind(&id)
        .bind(name)
        .bind(now().to_rfc3339())
        .execute(pool)
        .await
        .map_err(err)?;
    Ok(id)
}

async fn find_or_create_performer(pool: &SqlitePool, name: &str) -> Result<String, String> {
    let existing: Option<(String,)> =
        sqlx::query_as("SELECT id FROM performers WHERE name = ? COLLATE NOCASE")
            .bind(name)
            .fetch_optional(pool)
            .await
            .map_err(err)?;
    if let Some((id,)) = existing {
        return Ok(id);
    }
    let id = new_id();
    sqlx::query("INSERT INTO performers (id, name, created_at) VALUES (?, ?, ?)")
        .bind(&id)
        .bind(name)
        .bind(now().to_rfc3339())
        .execute(pool)
        .await
        .map_err(err)?;
    Ok(id)
}

async fn find_or_create_tag(pool: &SqlitePool, name: &str) -> Result<String, String> {
    let existing: Option<(String,)> =
        sqlx::query_as("SELECT id FROM tags WHERE name = ? COLLATE NOCASE")
            .bind(name)
            .fetch_optional(pool)
            .await
            .map_err(err)?;
    if let Some((id,)) = existing {
        return Ok(id);
    }
    let id = new_id();
    sqlx::query("INSERT INTO tags (id, name, created_at) VALUES (?, ?, ?)")
        .bind(&id)
        .bind(name)
        .bind(now().to_rfc3339())
        .execute(pool)
        .await
        .map_err(err)?;
    Ok(id)
}
