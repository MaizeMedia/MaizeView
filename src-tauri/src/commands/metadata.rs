//! Metadata CRUD: tags, performers, studios — list/create/delete + assignment
//! to scenes. Used by the scene detail drawer.

use tauri::State;

use crate::{
    commands::{
        err,
        scenes::{PerformerRow, StudioRow, TagRow},
    },
    models::{new_id, now},
    AppState,
};

// ─── tags ────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_tags(state: State<'_, AppState>) -> Result<Vec<TagRow>, String> {
    sqlx::query_as("SELECT id, name, color FROM tags ORDER BY name")
        .fetch_all(&state.pool)
        .await
        .map_err(err)
}

/// Tag with the number of scenes using it. For the Tags management view.
#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct TagWithCount {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
    pub scene_count: i64,
}

#[tauri::command]
pub async fn list_tags_with_counts(
    state: State<'_, AppState>,
) -> Result<Vec<TagWithCount>, String> {
    sqlx::query_as(
        r#"
        SELECT t.id, t.name, t.color,
               (SELECT COUNT(*) FROM scene_tags st WHERE st.tag_id = t.id) AS scene_count
        FROM tags t
        ORDER BY scene_count DESC, t.name ASC
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(err)
}

#[tauri::command]
pub async fn create_tag(state: State<'_, AppState>, name: String) -> Result<TagRow, String> {
    let row = TagRow {
        id: new_id(),
        name: name.trim().to_string(),
        color: None,
    };
    sqlx::query("INSERT INTO tags (id, name, created_at) VALUES (?, ?, ?)")
        .bind(&row.id)
        .bind(&row.name)
        .bind(now().to_rfc3339())
        .execute(&state.pool)
        .await
        .map_err(err)?;
    Ok(row)
}

#[tauri::command]
pub async fn delete_tag(state: State<'_, AppState>, id: String) -> Result<(), String> {
    sqlx::query("DELETE FROM tags WHERE id = ?")
        .bind(&id)
        .execute(&state.pool)
        .await
        .map_err(err)?;
    Ok(())
}

/// Add a tag to a scene. Idempotent (UNIQUE constraint ignored).
#[tauri::command]
pub async fn add_tag_to_scene(
    state: State<'_, AppState>,
    scene_id: String,
    tag_id: String,
) -> Result<(), String> {
    sqlx::query("INSERT OR IGNORE INTO scene_tags (scene_id, tag_id) VALUES (?, ?)")
        .bind(&scene_id)
        .bind(&tag_id)
        .execute(&state.pool)
        .await
        .map_err(err)?;
    Ok(())
}

#[tauri::command]
pub async fn remove_tag_from_scene(
    state: State<'_, AppState>,
    scene_id: String,
    tag_id: String,
) -> Result<(), String> {
    sqlx::query("DELETE FROM scene_tags WHERE scene_id = ? AND tag_id = ?")
        .bind(&scene_id)
        .bind(&tag_id)
        .execute(&state.pool)
        .await
        .map_err(err)?;
    Ok(())
}

// ─── performers ──────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_performers(state: State<'_, AppState>) -> Result<Vec<PerformerRow>, String> {
    sqlx::query_as("SELECT id, name FROM performers ORDER BY name")
        .fetch_all(&state.pool)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn create_performer(
    state: State<'_, AppState>,
    name: String,
) -> Result<PerformerRow, String> {
    let row = PerformerRow {
        id: new_id(),
        name: name.trim().to_string(),
    };
    sqlx::query("INSERT INTO performers (id, name, created_at) VALUES (?, ?, ?)")
        .bind(&row.id)
        .bind(&row.name)
        .bind(now().to_rfc3339())
        .execute(&state.pool)
        .await
        .map_err(err)?;
    Ok(row)
}

#[tauri::command]
pub async fn delete_performer(state: State<'_, AppState>, id: String) -> Result<(), String> {
    sqlx::query("DELETE FROM performers WHERE id = ?")
        .bind(&id)
        .execute(&state.pool)
        .await
        .map_err(err)?;
    Ok(())
}

#[tauri::command]
pub async fn add_performer_to_scene(
    state: State<'_, AppState>,
    scene_id: String,
    performer_id: String,
) -> Result<(), String> {
    sqlx::query("INSERT OR IGNORE INTO scene_performers (scene_id, performer_id) VALUES (?, ?)")
        .bind(&scene_id)
        .bind(&performer_id)
        .execute(&state.pool)
        .await
        .map_err(err)?;
    Ok(())
}

#[tauri::command]
pub async fn remove_performer_from_scene(
    state: State<'_, AppState>,
    scene_id: String,
    performer_id: String,
) -> Result<(), String> {
    sqlx::query("DELETE FROM scene_performers WHERE scene_id = ? AND performer_id = ?")
        .bind(&scene_id)
        .bind(&performer_id)
        .execute(&state.pool)
        .await
        .map_err(err)?;
    Ok(())
}

// ─── studios ─────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_studios(state: State<'_, AppState>) -> Result<Vec<StudioRow>, String> {
    sqlx::query_as("SELECT id, name FROM studios ORDER BY name")
        .fetch_all(&state.pool)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn create_studio(state: State<'_, AppState>, name: String) -> Result<StudioRow, String> {
    let row = StudioRow {
        id: new_id(),
        name: name.trim().to_string(),
    };
    sqlx::query("INSERT INTO studios (id, name, created_at) VALUES (?, ?, ?)")
        .bind(&row.id)
        .bind(&row.name)
        .bind(now().to_rfc3339())
        .execute(&state.pool)
        .await
        .map_err(err)?;
    Ok(row)
}

/// Set (or clear, if studio_id is None) a scene's studio.
#[tauri::command]
pub async fn set_scene_studio(
    state: State<'_, AppState>,
    scene_id: String,
    studio_id: Option<String>,
) -> Result<(), String> {
    sqlx::query("UPDATE scenes SET studio_id = ?, updated_at = ? WHERE id = ?")
        .bind(&studio_id)
        .bind(now().to_rfc3339())
        .bind(&scene_id)
        .execute(&state.pool)
        .await
        .map_err(err)?;
    Ok(())
}

// ─── scene title/details manual edit ─────────────────────────────────────

#[tauri::command]
pub async fn set_scene_title(
    state: State<'_, AppState>,
    scene_id: String,
    title: Option<String>,
) -> Result<(), String> {
    sqlx::query(
        "UPDATE scenes SET title = ?, title_source = 'manual', updated_at = ? WHERE id = ?",
    )
    .bind(&title)
    .bind(now().to_rfc3339())
    .bind(&scene_id)
    .execute(&state.pool)
    .await
    .map_err(err)?;
    Ok(())
}

#[tauri::command]
pub async fn set_scene_details(
    state: State<'_, AppState>,
    scene_id: String,
    details: Option<String>,
) -> Result<(), String> {
    sqlx::query(
        "UPDATE scenes SET details = ?, details_source = 'manual', updated_at = ? WHERE id = ?",
    )
    .bind(&details)
    .bind(now().to_rfc3339())
    .bind(&scene_id)
    .execute(&state.pool)
    .await
    .map_err(err)?;
    Ok(())
}
