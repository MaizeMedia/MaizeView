//! Suggest / apply embedded container tags from ffprobe (title / artist / comment).

use tauri::State;

use crate::{
    commands::err,
    models::{new_id, now},
    scanner::probe,
    AppState,
};

#[derive(Debug, Clone, serde::Serialize)]
pub struct EmbeddedMetadataSuggestion {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub comment: Option<String>,
    pub current_title: Option<String>,
    pub current_details: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ApplyEmbeddedMetadataInput {
    pub title: bool,
    pub details: bool,
    pub artist_as_performer: bool,
}

#[tauri::command]
pub async fn suggest_embedded_metadata(
    state: State<'_, AppState>,
    scene_id: String,
) -> Result<EmbeddedMetadataSuggestion, String> {
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
    let path = std::path::PathBuf::from(&file_path);

    let summary = tokio::task::spawn_blocking(move || probe::probe(&path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

    let row: Option<(Option<String>, Option<String>)> =
        sqlx::query_as("SELECT title, details FROM scenes WHERE id = ?")
            .bind(&scene_id)
            .fetch_optional(pool)
            .await
            .map_err(err)?;

    let (current_title, current_details) = row.unwrap_or((None, None));

    Ok(EmbeddedMetadataSuggestion {
        title: summary.title,
        artist: summary.artist,
        comment: summary.comment,
        current_title,
        current_details,
    })
}

#[tauri::command]
pub async fn apply_embedded_metadata(
    state: State<'_, AppState>,
    scene_id: String,
    fields: ApplyEmbeddedMetadataInput,
    title: Option<String>,
    comment: Option<String>,
    artist: Option<String>,
) -> Result<(), String> {
    let pool = &state.pool;
    let ts = now().to_rfc3339();

    if fields.title {
        if let Some(t) = title.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
            sqlx::query(
                "UPDATE scenes SET title = ?, title_source = 'embedded', updated_at = ? WHERE id = ?",
            )
            .bind(t)
            .bind(&ts)
            .bind(&scene_id)
            .execute(pool)
            .await
            .map_err(err)?;
        }
    }

    if fields.details {
        if let Some(d) = comment.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
            sqlx::query(
                "UPDATE scenes SET details = ?, details_source = 'embedded', updated_at = ? WHERE id = ?",
            )
            .bind(d)
            .bind(&ts)
            .bind(&scene_id)
            .execute(pool)
            .await
            .map_err(err)?;
        }
    }

    if fields.artist_as_performer {
        if let Some(raw) = artist.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
            // Split common multi-artist separators.
            for part in raw.split(&['/', ';', ',', '&'][..]) {
                let name = part.trim();
                if name.len() < 2 {
                    continue;
                }
                let performer_id = find_or_create_performer(pool, name).await?;
                sqlx::query(
                    "INSERT OR IGNORE INTO scene_performers (scene_id, performer_id) VALUES (?, ?)",
                )
                .bind(&scene_id)
                .bind(&performer_id)
                .execute(pool)
                .await
                .map_err(err)?;
            }
        }
    }

    Ok(())
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
