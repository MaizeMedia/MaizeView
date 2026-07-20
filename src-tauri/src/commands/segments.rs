//! Scene segments / timed bookmarks (Cove-inspired). Schema + CRUD spike.

use tauri::State;

use crate::{
    commands::err,
    models::{new_id, now},
    AppState,
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct SceneSegment {
    pub id: String,
    pub scene_id: String,
    pub start_sec: f64,
    pub end_sec: Option<f64>,
    pub label: String,
    pub tag_id: Option<String>,
    pub performer_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[tauri::command]
pub async fn list_scene_segments(
    state: State<'_, AppState>,
    scene_id: String,
) -> Result<Vec<SceneSegment>, String> {
    sqlx::query_as(
        r#"
        SELECT id, scene_id, start_sec, end_sec, label, tag_id, performer_id, created_at, updated_at
        FROM scene_segments
        WHERE scene_id = ?
        ORDER BY start_sec ASC, created_at ASC
        "#,
    )
    .bind(&scene_id)
    .fetch_all(&state.pool)
    .await
    .map_err(err)
}

#[derive(Debug, serde::Deserialize)]
pub struct CreateSegmentArgs {
    pub scene_id: String,
    pub start_sec: f64,
    pub end_sec: Option<f64>,
    pub label: Option<String>,
    pub tag_id: Option<String>,
    pub performer_id: Option<String>,
}

#[tauri::command]
pub async fn create_scene_segment(
    state: State<'_, AppState>,
    args: CreateSegmentArgs,
) -> Result<SceneSegment, String> {
    if args.start_sec < 0.0 || !args.start_sec.is_finite() {
        return Err("start_sec must be a non-negative number".into());
    }
    if let Some(end) = args.end_sec {
        if !end.is_finite() || end < args.start_sec {
            return Err("end_sec must be >= start_sec".into());
        }
    }

    let exists: Option<(String,)> = sqlx::query_as("SELECT id FROM scenes WHERE id = ?")
        .bind(&args.scene_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(err)?;
    if exists.is_none() {
        return Err("scene not found".into());
    }

    let ts = now().to_rfc3339();
    let row = SceneSegment {
        id: new_id(),
        scene_id: args.scene_id,
        start_sec: args.start_sec,
        end_sec: args.end_sec,
        label: args.label.map(|s| s.trim().to_string()).unwrap_or_default(),
        tag_id: args.tag_id,
        performer_id: args.performer_id,
        created_at: ts.clone(),
        updated_at: ts,
    };

    sqlx::query(
        r#"
        INSERT INTO scene_segments
          (id, scene_id, start_sec, end_sec, label, tag_id, performer_id, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&row.id)
    .bind(&row.scene_id)
    .bind(row.start_sec)
    .bind(row.end_sec)
    .bind(&row.label)
    .bind(&row.tag_id)
    .bind(&row.performer_id)
    .bind(&row.created_at)
    .bind(&row.updated_at)
    .execute(&state.pool)
    .await
    .map_err(err)?;

    Ok(row)
}

#[derive(Debug, serde::Deserialize)]
pub struct UpdateSegmentArgs {
    pub id: String,
    pub start_sec: Option<f64>,
    pub end_sec: Option<f64>,
    pub label: Option<String>,
    pub tag_id: Option<String>,
    pub performer_id: Option<String>,
}

#[tauri::command]
pub async fn update_scene_segment(
    state: State<'_, AppState>,
    args: UpdateSegmentArgs,
) -> Result<SceneSegment, String> {
    let mut row: SceneSegment = sqlx::query_as(
        r#"
        SELECT id, scene_id, start_sec, end_sec, label, tag_id, performer_id, created_at, updated_at
        FROM scene_segments WHERE id = ?
        "#,
    )
    .bind(&args.id)
    .fetch_optional(&state.pool)
    .await
    .map_err(err)?
    .ok_or_else(|| "segment not found".to_string())?;

    if let Some(start) = args.start_sec {
        if start < 0.0 || !start.is_finite() {
            return Err("start_sec must be a non-negative number".into());
        }
        row.start_sec = start;
    }
    if let Some(end) = args.end_sec {
        if !end.is_finite() || end < row.start_sec {
            return Err("end_sec must be >= start_sec".into());
        }
        row.end_sec = Some(end);
    }
    if let Some(label) = args.label {
        row.label = label.trim().to_string();
    }
    if let Some(tag_id) = args.tag_id {
        row.tag_id = if tag_id.is_empty() {
            None
        } else {
            Some(tag_id)
        };
    }
    if let Some(performer_id) = args.performer_id {
        row.performer_id = if performer_id.is_empty() {
            None
        } else {
            Some(performer_id)
        };
    }
    if let Some(end) = row.end_sec {
        if end < row.start_sec {
            return Err("end_sec must be >= start_sec".into());
        }
    }
    row.updated_at = now().to_rfc3339();

    sqlx::query(
        r#"
        UPDATE scene_segments
        SET start_sec = ?, end_sec = ?, label = ?, tag_id = ?, performer_id = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(row.start_sec)
    .bind(row.end_sec)
    .bind(&row.label)
    .bind(&row.tag_id)
    .bind(&row.performer_id)
    .bind(&row.updated_at)
    .bind(&row.id)
    .execute(&state.pool)
    .await
    .map_err(err)?;

    Ok(row)
}

#[tauri::command]
pub async fn delete_scene_segment(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let res = sqlx::query("DELETE FROM scene_segments WHERE id = ?")
        .bind(&id)
        .execute(&state.pool)
        .await
        .map_err(err)?;
    if res.rows_affected() == 0 {
        return Err("segment not found".into());
    }
    Ok(())
}
