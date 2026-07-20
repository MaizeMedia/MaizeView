//! Saved library filters: named JSON snapshots of search + filter state.

use tauri::State;

use crate::{
    commands::err,
    models::{new_id, now},
    AppState,
};

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct SavedFilterRow {
    pub id: String,
    pub name: String,
    pub payload: String,
    pub created_at: String,
    pub updated_at: String,
}

#[tauri::command]
pub async fn list_saved_filters(state: State<'_, AppState>) -> Result<Vec<SavedFilterRow>, String> {
    sqlx::query_as(
        "SELECT id, name, payload, created_at, updated_at FROM saved_filters ORDER BY updated_at DESC",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(err)
}

#[tauri::command]
pub async fn create_saved_filter(
    state: State<'_, AppState>,
    name: String,
    payload: String,
) -> Result<SavedFilterRow, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("name is required".into());
    }
    if payload.trim().is_empty() {
        return Err("payload is required".into());
    }
    // Validate JSON shape early so we don't store garbage.
    serde_json::from_str::<serde_json::Value>(&payload)
        .map_err(|e| format!("invalid payload JSON: {e}"))?;

    let ts = now().to_rfc3339();
    let id = new_id();
    sqlx::query(
        "INSERT INTO saved_filters (id, name, payload, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&name)
    .bind(&payload)
    .bind(&ts)
    .bind(&ts)
    .execute(&state.pool)
    .await
    .map_err(err)?;

    Ok(SavedFilterRow {
        id,
        name,
        payload,
        created_at: ts.clone(),
        updated_at: ts,
    })
}

#[tauri::command]
pub async fn delete_saved_filter(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let res = sqlx::query("DELETE FROM saved_filters WHERE id = ?")
        .bind(&id)
        .execute(&state.pool)
        .await
        .map_err(err)?;
    if res.rows_affected() == 0 {
        return Err(format!("saved filter not found: {id}"));
    }
    Ok(())
}

#[tauri::command]
pub async fn rename_saved_filter(
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> Result<(), String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("name is required".into());
    }
    let res = sqlx::query("UPDATE saved_filters SET name = ?, updated_at = ? WHERE id = ?")
        .bind(&name)
        .bind(now().to_rfc3339())
        .bind(&id)
        .execute(&state.pool)
        .await
        .map_err(err)?;
    if res.rows_affected() == 0 {
        return Err(format!("saved filter not found: {id}"));
    }
    Ok(())
}
