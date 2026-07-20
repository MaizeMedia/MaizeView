//! Playlist commands: CRUD, item management, and **weighted shuffle** by
//! favorite level. A scene's weight in the shuffle is its `favorite` level
//! (0..5), with a floor of 1 so unfavorited scenes still appear (just rarely).

use tauri::{AppHandle, Emitter, State};

use rand::RngExt;

use crate::{
    commands::{err, scenes::SceneGridRow},
    models::{new_id, now},
    AppState,
};

// ─── playlist CRUD ──────────────────────────────────────────────────────

/// Emitted after any playlist mutation so other windows can refresh.
#[derive(Clone, serde::Serialize)]
struct PlaylistChangedPayload {
    playlist_id: String,
}

fn emit_playlist_changed(app: &AppHandle, playlist_id: &str) {
    let _ = app.emit(
        "playlist://changed",
        PlaylistChangedPayload {
            playlist_id: playlist_id.to_string(),
        },
    );
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct PlaylistRow {
    pub id: String,
    pub name: String,
    pub shuffle_by_default: bool,
    pub created_at: String,
    pub updated_at: String,
    pub item_count: i64,
}

#[tauri::command]
pub async fn list_playlists(state: State<'_, AppState>) -> Result<Vec<PlaylistRow>, String> {
    sqlx::query_as(
        r#"
        SELECT p.id, p.name, p.shuffle_by_default, p.created_at, p.updated_at,
               (
                 SELECT COUNT(*) FROM playlist_items pi
                 JOIN scenes s ON s.id = pi.scene_id
                 WHERE pi.playlist_id = p.id
                   AND EXISTS (SELECT 1 FROM files f WHERE f.scene_id = s.id)
               ) AS item_count
        FROM playlists p
        ORDER BY p.updated_at DESC
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(err)
}

#[tauri::command]
pub async fn create_playlist(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> Result<PlaylistRow, String> {
    let ts = now().to_rfc3339();
    let id = new_id();
    sqlx::query("INSERT INTO playlists (id, name, created_at, updated_at) VALUES (?, ?, ?, ?)")
        .bind(&id)
        .bind(name.trim())
        .bind(&ts)
        .bind(&ts)
        .execute(&state.pool)
        .await
        .map_err(err)?;
    emit_playlist_changed(&app, &id);
    Ok(PlaylistRow {
        id,
        name: name.trim().to_string(),
        shuffle_by_default: false,
        created_at: ts.clone(),
        updated_at: ts,
        item_count: 0,
    })
}

#[tauri::command]
pub async fn set_playlist_shuffle_default(
    state: State<'_, AppState>,
    id: String,
    shuffle_by_default: bool,
) -> Result<(), String> {
    sqlx::query("UPDATE playlists SET shuffle_by_default = ?, updated_at = ? WHERE id = ?")
        .bind(shuffle_by_default)
        .bind(now().to_rfc3339())
        .bind(&id)
        .execute(&state.pool)
        .await
        .map_err(err)?;
    Ok(())
}

#[tauri::command]
pub async fn rename_playlist(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> Result<(), String> {
    sqlx::query("UPDATE playlists SET name = ?, updated_at = ? WHERE id = ?")
        .bind(name.trim())
        .bind(now().to_rfc3339())
        .bind(&id)
        .execute(&state.pool)
        .await
        .map_err(err)?;
    emit_playlist_changed(&app, &id);
    Ok(())
}

#[tauri::command]
pub async fn delete_playlist(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    sqlx::query("DELETE FROM playlists WHERE id = ?")
        .bind(&id)
        .execute(&state.pool)
        .await
        .map_err(err)?;
    emit_playlist_changed(&app, &id);
    Ok(())
}

// ─── items ──────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn playlist_items(
    state: State<'_, AppState>,
    playlist_id: String,
) -> Result<Vec<SceneGridRow>, String> {
    // Grid payload only — skip sprite path (unused on cards) to shrink IPC for large lists.
    sqlx::query_as(
        r#"
        SELECT s.id, s.title, s.favorite, s.rating, s.play_count, s.created_at,
               f.duration, f.width, f.height, f.thumb_path,
               NULL AS thumb_sprite_path, f.path AS file_path
        FROM playlist_items pi
        JOIN scenes s ON s.id = pi.scene_id
        INNER JOIN files f ON f.id = (
            SELECT id FROM files WHERE scene_id = s.id
            ORDER BY duration DESC NULLS LAST, scanned_at ASC
            LIMIT 1
        )
        WHERE pi.playlist_id = ?
        ORDER BY pi.position ASC
        "#,
    )
    .bind(&playlist_id)
    .fetch_all(&state.pool)
    .await
    .map_err(err)
}

#[tauri::command]
pub async fn add_to_playlist(
    app: AppHandle,
    state: State<'_, AppState>,
    playlist_id: String,
    scene_id: String,
) -> Result<bool, String> {
    // INSERT OR IGNORE: re-adding a scene already in the playlist is a no-op.
    // Position = current max + 1 (or 0 if empty), computed in the same statement.
    let result = sqlx::query(
        "INSERT OR IGNORE INTO playlist_items (id, playlist_id, scene_id, position, added_at)
         VALUES (?, ?, ?,
                 (SELECT COALESCE(MAX(position), -1) + 1 FROM playlist_items WHERE playlist_id = ?),
                 ?)",
    )
    .bind(new_id())
    .bind(&playlist_id)
    .bind(&scene_id)
    .bind(&playlist_id)
    .bind(now().to_rfc3339())
    .execute(&state.pool)
    .await
    .map_err(err)?;

    if result.rows_affected() == 0 {
        return Ok(false); // already in the playlist — nothing changed
    }

    sqlx::query("UPDATE playlists SET updated_at = ? WHERE id = ?")
        .bind(now().to_rfc3339())
        .bind(&playlist_id)
        .execute(&state.pool)
        .await
        .map_err(err)?;
    emit_playlist_changed(&app, &playlist_id);
    Ok(true)
}

#[tauri::command]
pub async fn remove_from_playlist(
    app: AppHandle,
    state: State<'_, AppState>,
    playlist_id: String,
    scene_id: String,
) -> Result<(), String> {
    sqlx::query("DELETE FROM playlist_items WHERE playlist_id = ? AND scene_id = ?")
        .bind(&playlist_id)
        .bind(&scene_id)
        .execute(&state.pool)
        .await
        .map_err(err)?;
    // Re-compact positions so there are no gaps.
    compact_positions(&state, &playlist_id).await?;
    sqlx::query("UPDATE playlists SET updated_at = ? WHERE id = ?")
        .bind(now().to_rfc3339())
        .bind(&playlist_id)
        .execute(&state.pool)
        .await
        .map_err(err)?;
    emit_playlist_changed(&app, &playlist_id);
    Ok(())
}

/// Reorder a playlist: `scene_ids_in_order` is the full desired scene order.
#[tauri::command]
pub async fn reorder_playlist(
    app: AppHandle,
    state: State<'_, AppState>,
    playlist_id: String,
    scene_ids_in_order: Vec<String>,
) -> Result<(), String> {
    // Positional update — never DELETEs rows, so scenes added concurrently by
    // another window survive and are appended after the submitted order.
    let mut tx = state.pool.begin().await.map_err(err)?;

    // Capture the original order so un-submitted rows can keep their relative
    // order when appended after the submitted ones.
    let original: Vec<(String,)> = sqlx::query_as(
        "SELECT scene_id FROM playlist_items WHERE playlist_id = ? ORDER BY position ASC",
    )
    .bind(&playlist_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(err)?;

    // Park every row at a unique temp position. rowid is unique per row and
    // real positions are >= 0, so -rowid can't collide with a real position.
    sqlx::query("UPDATE playlist_items SET position = -rowid WHERE playlist_id = ?")
        .bind(&playlist_id)
        .execute(&mut *tx)
        .await
        .map_err(err)?;

    // Place the submitted scenes at their requested positions (a submitted
    // scene that isn't in the playlist simply matches no row).
    for (pos, scene_id) in scene_ids_in_order.iter().enumerate() {
        sqlx::query(
            "UPDATE playlist_items SET position = ? WHERE playlist_id = ? AND scene_id = ?",
        )
        .bind(pos as i64)
        .bind(&playlist_id)
        .bind(scene_id)
        .execute(&mut *tx)
        .await
        .map_err(err)?;
    }

    // Compact rows still parked (scenes not in the submitted list, e.g. added
    // concurrently) to positions continuing after the submitted ones, keeping
    // their original relative order.
    let mut next = scene_ids_in_order.len() as i64;
    for (scene_id,) in original {
        if !scene_ids_in_order.iter().any(|s| *s == scene_id) {
            sqlx::query(
                "UPDATE playlist_items SET position = ? WHERE playlist_id = ? AND scene_id = ?",
            )
            .bind(next)
            .bind(&playlist_id)
            .bind(&scene_id)
            .execute(&mut *tx)
            .await
            .map_err(err)?;
            next += 1;
        }
    }

    sqlx::query("UPDATE playlists SET updated_at = ? WHERE id = ?")
        .bind(now().to_rfc3339())
        .bind(&playlist_id)
        .execute(&mut *tx)
        .await
        .map_err(err)?;
    tx.commit().await.map_err(err)?;
    emit_playlist_changed(&app, &playlist_id);
    Ok(())
}

async fn compact_positions(state: &State<'_, AppState>, playlist_id: &str) -> Result<(), String> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT scene_id FROM playlist_items WHERE playlist_id = ? ORDER BY position ASC",
    )
    .bind(playlist_id)
    .fetch_all(&state.pool)
    .await
    .map_err(err)?;
    for (pos, (scene_id,)) in rows.into_iter().enumerate() {
        sqlx::query(
            "UPDATE playlist_items SET position = ? WHERE playlist_id = ? AND scene_id = ?",
        )
        .bind(pos as i64)
        .bind(playlist_id)
        .bind(&scene_id)
        .execute(&state.pool)
        .await
        .map_err(err)?;
    }
    Ok(())
}

// ─── weighted shuffle ───────────────────────────────────────────────────

/// A scene in a shuffle result, with its weight for transparency/debugging.
#[derive(Debug, serde::Serialize)]
pub struct ShuffleEntry {
    #[serde(flatten)]
    pub scene: SceneGridRow,
    pub weight: i64,
}

/// Weighted shuffle: returns the playlist's scenes in a new random order
/// where higher-favorite scenes appear earlier / more often. Weight per scene
/// = `max(1, favorite)` (favorite 0 → weight 1, favorite 5 → weight 5).
///
/// Algorithm: weighted reservoir draw without replacement — repeatedly pick
/// the next scene with probability proportional to its weight from those
/// remaining. This is the "music app favorites" behavior.
#[tauri::command]
pub async fn shuffle_playlist(
    state: State<'_, AppState>,
    playlist_id: String,
) -> Result<Vec<ShuffleEntry>, String> {
    let scenes: Vec<SceneGridRow> = playlist_items(state.clone(), playlist_id.clone()).await?;

    // Build (row, weight) working set.
    let mut remaining: Vec<(SceneGridRow, f64)> = scenes
        .into_iter()
        .map(|s| {
            let w = (s.favorite.max(0) as f64).max(1.0);
            (s, w)
        })
        .collect();

    // Seed RNG from entropy.
    let mut rng = rand::rng();
    let mut out: Vec<ShuffleEntry> = Vec::with_capacity(remaining.len());

    while !remaining.is_empty() {
        let total: f64 = remaining.iter().map(|(_, w)| *w).sum();
        // Pick a random cut in [0, total).
        let cut: f64 = rng.random_range(0.0..total);
        let mut acc = 0.0;
        let mut chosen = 0usize;
        for (i, (_, w)) in remaining.iter().enumerate() {
            acc += w;
            if cut < acc {
                chosen = i;
                break;
            }
        }
        let (row, w) = remaining.remove(chosen);
        out.push(ShuffleEntry {
            scene: row,
            weight: w as i64,
        });
    }

    Ok(out)
}
