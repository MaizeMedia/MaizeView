//! Suggest / apply local path metadata (ADR-013: parse only, never move files).
//!
//! Matches existing catalog names, plus review-gated create suggestions from
//! ancestor folders and tag tokens (`anal`, `1080p`, …).

use tauri::State;

use crate::{
    commands::err,
    filename_parse::{self, FolderEntityKind},
    models::{new_id, now},
    path_meta::{self, PathMetaKind},
    AppState,
};

#[derive(Debug, Clone, serde::Serialize)]
pub struct PathMetaSuggestion {
    pub kind: String,
    /// Empty when `create_new` is true.
    pub id: String,
    pub name: String,
    pub already_linked: bool,
    pub create_new: bool,
    /// `catalog` | `folder` | `token` | `bracket`
    pub source: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SuggestPathMetadataResult {
    pub file_path: String,
    pub suggestions: Vec<PathMetaSuggestion>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ApplyPathMetadataInput {
    pub studio_id: Option<String>,
    /// Create+link a studio by name when set (review-gated folder/bracket hint).
    pub create_studio_name: Option<String>,
    #[serde(default)]
    pub performer_ids: Vec<String>,
    #[serde(default)]
    pub create_performer_names: Vec<String>,
    #[serde(default)]
    pub tag_ids: Vec<String>,
    #[serde(default)]
    pub create_tag_names: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct BatchPathMetadataResult {
    pub scenes_scanned: u64,
    pub scenes_with_hits: u64,
    pub studios_linked: u64,
    pub performers_linked: u64,
    pub tags_linked: u64,
}

#[tauri::command]
pub async fn suggest_path_metadata(
    state: State<'_, AppState>,
    scene_id: String,
) -> Result<SuggestPathMetadataResult, String> {
    let pool = &state.pool;

    let file_path: Option<String> = sqlx::query_scalar(
        r#"
        SELECT path FROM files
        WHERE scene_id = ?
        ORDER BY duration DESC NULLS LAST, scanned_at ASC
        LIMIT 1
        "#,
    )
    .bind(&scene_id)
    .fetch_optional(pool)
    .await
    .map_err(err)?;

    let file_path = file_path.ok_or_else(|| "scene has no files".to_string())?;
    let path = std::path::Path::new(&file_path);

    let studios: Vec<(String, String)> = sqlx::query_as("SELECT id, name FROM studios")
        .fetch_all(pool)
        .await
        .map_err(err)?;
    let performers: Vec<(String, String)> = sqlx::query_as("SELECT id, name FROM performers")
        .fetch_all(pool)
        .await
        .map_err(err)?;
    let tags: Vec<(String, String)> = sqlx::query_as("SELECT id, name FROM tags")
        .fetch_all(pool)
        .await
        .map_err(err)?;

    let current_studio: Option<String> =
        sqlx::query_scalar("SELECT studio_id FROM scenes WHERE id = ?")
            .bind(&scene_id)
            .fetch_optional(pool)
            .await
            .map_err(err)?
            .flatten();

    let linked_performers: Vec<String> =
        sqlx::query_scalar("SELECT performer_id FROM scene_performers WHERE scene_id = ?")
            .bind(&scene_id)
            .fetch_all(pool)
            .await
            .map_err(err)?;

    let linked_tags: Vec<String> =
        sqlx::query_scalar("SELECT tag_id FROM scene_tags WHERE scene_id = ?")
            .bind(&scene_id)
            .fetch_all(pool)
            .await
            .map_err(err)?;

    let mut suggestions: Vec<PathMetaSuggestion> = Vec::new();

    // 1) Existing catalog names in path
    let hits = path_meta::match_catalog_against_path(path, &studios, &performers, &tags);
    for h in hits {
        let already_linked = match h.kind {
            PathMetaKind::Studio => current_studio.as_deref() == Some(h.id.as_str()),
            PathMetaKind::Performer => linked_performers.iter().any(|id| id == &h.id),
            PathMetaKind::Tag => linked_tags.iter().any(|id| id == &h.id),
        };
        suggestions.push(PathMetaSuggestion {
            kind: kind_str(h.kind).to_string(),
            id: h.id,
            name: h.name,
            already_linked,
            create_new: false,
            source: "catalog".into(),
        });
    }

    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");

    // 2) Ancestor folder → studio/performer (create if not in catalog)
    for hint in filename_parse::folder_entity_hints(path) {
        let kind = match hint.kind {
            FolderEntityKind::Studio => "studio",
            FolderEntityKind::Performer => "performer",
        };
        if let Some((id, name)) = find_catalog_by_name(
            match hint.kind {
                FolderEntityKind::Studio => &studios,
                FolderEntityKind::Performer => &performers,
            },
            &hint.name,
        ) {
            if suggestions.iter().any(|s| s.kind == kind && s.id == id) {
                continue;
            }
            let already_linked = match hint.kind {
                FolderEntityKind::Studio => current_studio.as_deref() == Some(id.as_str()),
                FolderEntityKind::Performer => linked_performers.iter().any(|x| x == &id),
            };
            suggestions.push(PathMetaSuggestion {
                kind: kind.into(),
                id,
                name,
                already_linked,
                create_new: false,
                source: "folder".into(),
            });
        } else if !suggestions
            .iter()
            .any(|s| s.kind == kind && s.name.eq_ignore_ascii_case(&hint.name))
        {
            suggestions.push(PathMetaSuggestion {
                kind: kind.into(),
                id: String::new(),
                name: hint.name,
                already_linked: false,
                create_new: true,
                source: "folder".into(),
            });
        }
    }

    // 3) Bracket studio candidates (e.g. [ExampleStudio])
    for name in filename_parse::bracket_studio_candidates(stem) {
        if let Some((id, catalog_name)) = find_catalog_by_name(&studios, &name) {
            if suggestions.iter().any(|s| s.kind == "studio" && s.id == id) {
                continue;
            }
            let already = current_studio.as_deref() == Some(id.as_str());
            suggestions.push(PathMetaSuggestion {
                kind: "studio".into(),
                id,
                name: catalog_name,
                already_linked: already,
                create_new: false,
                source: "bracket".into(),
            });
        } else if !suggestions
            .iter()
            .any(|s| s.kind == "studio" && s.name.eq_ignore_ascii_case(&name))
        {
            suggestions.push(PathMetaSuggestion {
                kind: "studio".into(),
                id: String::new(),
                name,
                already_linked: false,
                create_new: true,
                source: "bracket".into(),
            });
        }
    }

    // 4) Tag tokens (anal, 1080p, 1on1, …)
    for tok in filename_parse::tag_candidates_from_stem(stem) {
        let display = pretty_tag_name(&tok);
        if let Some((id, name)) =
            find_catalog_by_name(&tags, &display).or_else(|| find_catalog_by_name(&tags, &tok))
        {
            if suggestions.iter().any(|s| s.kind == "tag" && s.id == id) {
                continue;
            }
            let already = linked_tags.iter().any(|x| x == &id);
            suggestions.push(PathMetaSuggestion {
                kind: "tag".into(),
                id,
                name,
                already_linked: already,
                create_new: false,
                source: "token".into(),
            });
        } else if !suggestions
            .iter()
            .any(|s| s.kind == "tag" && s.name.eq_ignore_ascii_case(&display))
        {
            suggestions.push(PathMetaSuggestion {
                kind: "tag".into(),
                id: String::new(),
                name: display,
                already_linked: false,
                create_new: true,
                source: "token".into(),
            });
        }
    }

    Ok(SuggestPathMetadataResult {
        file_path,
        suggestions,
    })
}

/// Apply selected suggestions. Can create new catalog rows when names are provided.
#[tauri::command]
pub async fn apply_path_metadata(
    state: State<'_, AppState>,
    scene_id: String,
    fields: ApplyPathMetadataInput,
) -> Result<(), String> {
    let pool = &state.pool;
    let ts = now().to_rfc3339();

    let mut studio_id = fields.studio_id.clone().filter(|s| !s.is_empty());
    if studio_id.is_none() {
        if let Some(name) = fields
            .create_studio_name
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            studio_id = Some(find_or_create_studio(pool, name).await?);
        }
    }

    if let Some(ref sid) = studio_id {
        sqlx::query("UPDATE scenes SET studio_id = ?, updated_at = ? WHERE id = ?")
            .bind(sid)
            .bind(&ts)
            .bind(&scene_id)
            .execute(pool)
            .await
            .map_err(err)?;
    }

    for pid in &fields.performer_ids {
        if pid.trim().is_empty() {
            continue;
        }
        link_performer(pool, &scene_id, pid).await?;
    }
    for name in &fields.create_performer_names {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let pid = find_or_create_performer(pool, name).await?;
        link_performer(pool, &scene_id, &pid).await?;
    }

    for tid in &fields.tag_ids {
        if tid.trim().is_empty() {
            continue;
        }
        link_tag(pool, &scene_id, tid).await?;
    }
    for name in &fields.create_tag_names {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let tid = find_or_create_tag(pool, name).await?;
        link_tag(pool, &scene_id, &tid).await?;
    }

    Ok(())
}

/// Link existing catalog entities found in paths across the library (no creates).
#[tauri::command]
pub async fn batch_apply_path_metadata(
    state: State<'_, AppState>,
) -> Result<BatchPathMetadataResult, String> {
    let pool = &state.pool;

    let scene_ids: Vec<(String,)> = sqlx::query_as("SELECT id FROM scenes")
        .fetch_all(pool)
        .await
        .map_err(err)?;

    let studios: Vec<(String, String)> = sqlx::query_as("SELECT id, name FROM studios")
        .fetch_all(pool)
        .await
        .map_err(err)?;
    let performers: Vec<(String, String)> = sqlx::query_as("SELECT id, name FROM performers")
        .fetch_all(pool)
        .await
        .map_err(err)?;
    let tags: Vec<(String, String)> = sqlx::query_as("SELECT id, name FROM tags")
        .fetch_all(pool)
        .await
        .map_err(err)?;

    let mut scenes_with_hits = 0u64;
    let mut studios_linked = 0u64;
    let mut performers_linked = 0u64;
    let mut tags_linked = 0u64;
    let ts = now().to_rfc3339();

    for (scene_id,) in &scene_ids {
        let file_path: Option<String> = sqlx::query_scalar(
            r#"
            SELECT path FROM files
            WHERE scene_id = ?
            ORDER BY duration DESC NULLS LAST, scanned_at ASC
            LIMIT 1
            "#,
        )
        .bind(scene_id)
        .fetch_optional(pool)
        .await
        .map_err(err)?;

        let Some(file_path) = file_path else {
            continue;
        };
        let path = std::path::Path::new(&file_path);
        let hits = path_meta::match_catalog_against_path(path, &studios, &performers, &tags);
        if hits.is_empty() {
            // Still try folder names that match existing catalog
            let mut any = false;
            for hint in filename_parse::folder_entity_hints(path) {
                match hint.kind {
                    FolderEntityKind::Studio => {
                        if let Some((id, _)) = find_catalog_by_name(&studios, &hint.name) {
                            if apply_studio_if_empty(pool, scene_id, &id, &ts).await? {
                                studios_linked += 1;
                                any = true;
                            }
                        }
                    }
                    FolderEntityKind::Performer => {
                        if let Some((id, _)) = find_catalog_by_name(&performers, &hint.name) {
                            if link_performer_new(pool, scene_id, &id).await? {
                                performers_linked += 1;
                                any = true;
                            }
                        }
                    }
                }
            }
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            for tok in filename_parse::tag_candidates_from_stem(stem) {
                let display = pretty_tag_name(&tok);
                if let Some((id, _)) = find_catalog_by_name(&tags, &display)
                    .or_else(|| find_catalog_by_name(&tags, &tok))
                {
                    if link_tag_new(pool, scene_id, &id).await? {
                        tags_linked += 1;
                        any = true;
                    }
                }
            }
            if any {
                scenes_with_hits += 1;
            }
            continue;
        }

        let mut any = false;
        for h in hits {
            match h.kind {
                PathMetaKind::Studio => {
                    if apply_studio_if_empty(pool, scene_id, &h.id, &ts).await? {
                        studios_linked += 1;
                        any = true;
                    }
                }
                PathMetaKind::Performer => {
                    if link_performer_new(pool, scene_id, &h.id).await? {
                        performers_linked += 1;
                        any = true;
                    }
                }
                PathMetaKind::Tag => {
                    if link_tag_new(pool, scene_id, &h.id).await? {
                        tags_linked += 1;
                        any = true;
                    }
                }
            }
        }
        // Folder + token for existing catalog even when path substring also hit
        for hint in filename_parse::folder_entity_hints(path) {
            match hint.kind {
                FolderEntityKind::Studio => {
                    if let Some((id, _)) = find_catalog_by_name(&studios, &hint.name) {
                        if apply_studio_if_empty(pool, scene_id, &id, &ts).await? {
                            studios_linked += 1;
                            any = true;
                        }
                    }
                }
                FolderEntityKind::Performer => {
                    if let Some((id, _)) = find_catalog_by_name(&performers, &hint.name) {
                        if link_performer_new(pool, scene_id, &id).await? {
                            performers_linked += 1;
                            any = true;
                        }
                    }
                }
            }
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        for tok in filename_parse::tag_candidates_from_stem(stem) {
            let display = pretty_tag_name(&tok);
            if let Some((id, _)) =
                find_catalog_by_name(&tags, &display).or_else(|| find_catalog_by_name(&tags, &tok))
            {
                if link_tag_new(pool, scene_id, &id).await? {
                    tags_linked += 1;
                    any = true;
                }
            }
        }
        if any {
            scenes_with_hits += 1;
        }
    }

    Ok(BatchPathMetadataResult {
        scenes_scanned: scene_ids.len() as u64,
        scenes_with_hits,
        studios_linked,
        performers_linked,
        tags_linked,
    })
}

fn kind_str(k: PathMetaKind) -> &'static str {
    match k {
        PathMetaKind::Studio => "studio",
        PathMetaKind::Performer => "performer",
        PathMetaKind::Tag => "tag",
    }
}

fn find_catalog_by_name(rows: &[(String, String)], name: &str) -> Option<(String, String)> {
    rows.iter()
        .find(|(_, n)| n.eq_ignore_ascii_case(name))
        .map(|(id, n)| (id.clone(), n.clone()))
}

fn pretty_tag_name(tok: &str) -> String {
    match tok {
        "1080p" | "720p" | "2160p" | "1440p" | "4k" | "8k" | "uhd" | "hdr" => tok.to_string(),
        "1on1" => "1on1".into(),
        "2on1" => "2on1".into(),
        "3on1" => "3on1".into(),
        other => {
            let mut c = other.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        }
    }
}

async fn find_or_create_studio(pool: &sqlx::SqlitePool, name: &str) -> Result<String, String> {
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

async fn find_or_create_performer(pool: &sqlx::SqlitePool, name: &str) -> Result<String, String> {
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

async fn find_or_create_tag(pool: &sqlx::SqlitePool, name: &str) -> Result<String, String> {
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

async fn link_performer(pool: &sqlx::SqlitePool, scene_id: &str, pid: &str) -> Result<(), String> {
    sqlx::query("INSERT OR IGNORE INTO scene_performers (scene_id, performer_id) VALUES (?, ?)")
        .bind(scene_id)
        .bind(pid)
        .execute(pool)
        .await
        .map_err(err)?;
    Ok(())
}

async fn link_tag(pool: &sqlx::SqlitePool, scene_id: &str, tid: &str) -> Result<(), String> {
    sqlx::query("INSERT OR IGNORE INTO scene_tags (scene_id, tag_id) VALUES (?, ?)")
        .bind(scene_id)
        .bind(tid)
        .execute(pool)
        .await
        .map_err(err)?;
    Ok(())
}

async fn apply_studio_if_empty(
    pool: &sqlx::SqlitePool,
    scene_id: &str,
    studio_id: &str,
    ts: &str,
) -> Result<bool, String> {
    let res = sqlx::query(
        "UPDATE scenes SET studio_id = ?, updated_at = ? WHERE id = ? AND studio_id IS NULL",
    )
    .bind(studio_id)
    .bind(ts)
    .bind(scene_id)
    .execute(pool)
    .await
    .map_err(err)?;
    Ok(res.rows_affected() > 0)
}

async fn link_performer_new(
    pool: &sqlx::SqlitePool,
    scene_id: &str,
    pid: &str,
) -> Result<bool, String> {
    let res = sqlx::query(
        "INSERT OR IGNORE INTO scene_performers (scene_id, performer_id) VALUES (?, ?)",
    )
    .bind(scene_id)
    .bind(pid)
    .execute(pool)
    .await
    .map_err(err)?;
    Ok(res.rows_affected() > 0)
}

async fn link_tag_new(pool: &sqlx::SqlitePool, scene_id: &str, tid: &str) -> Result<bool, String> {
    let res = sqlx::query("INSERT OR IGNORE INTO scene_tags (scene_id, tag_id) VALUES (?, ?)")
        .bind(scene_id)
        .bind(tid)
        .execute(pool)
        .await
        .map_err(err)?;
    Ok(res.rows_affected() > 0)
}
