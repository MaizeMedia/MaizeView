//! Player commands — cross-window queue handoff + scene→file resolution.
//!
//! Each playback window is an isolated webview (its own JS context), so we
//! can't share JS memory between the catalog window and player windows.
//! Instead, the catalog "stages" a queue here (keyed by the player window
//! label), and the player window "claims" it on mount. This avoids URL query
//! string length limits (a playlist can be hundreds of scenes) and timing
//! races with cross-window events.
//!
//! Per ADR-011, each player window owns its own queue + shuffle state; this
//! module is just the handoff pipe, not the playback logic.

use std::collections::HashMap;
use std::sync::Mutex;

use sqlx::SqlitePool;
use tauri::{AppHandle, Manager, State};

use crate::AppState;

/// A staged queue waiting to be claimed by its player window.
#[derive(Clone, serde::Serialize)]
pub struct StagedQueue {
    /// Ordered scene IDs in the queue (play order before any shuffle).
    pub scene_ids: Vec<String>,
    /// Index into scene_ids of the scene to start playing at.
    pub start_index: usize,
    /// Default shuffle setting inherited from the source playlist (ADR-011).
    pub shuffle_by_default: bool,
}

/// Per-app stash of staged queues, keyed by player window label.
/// Claimed (popped) once by the player window on mount.
#[derive(Default)]
pub struct PlayerStash {
    queues: Mutex<HashMap<String, StagedQueue>>,
}

impl PlayerStash {
    fn insert(&self, label: String, q: StagedQueue) {
        let mut g = self.queues.lock().expect("player stash mutex poisoned");
        g.insert(label, q);
    }

    fn claim(&self, label: &str) -> Option<StagedQueue> {
        let mut g = self.queues.lock().expect("player stash mutex poisoned");
        g.remove(label)
    }
}

/// Resolve the primary playable file path for a scene (the first file with a
/// known path). Returns the absolute path as a string, or None if the scene
/// has no playable file on record.
async fn resolve_file_path(pool: &SqlitePool, scene_id: &str) -> Option<String> {
    let row: (String,) = sqlx::query_as(
        "SELECT path FROM files WHERE scene_id = ? AND path IS NOT NULL ORDER BY duration DESC NULLS LAST, scanned_at ASC LIMIT 1",
    )
    .bind(scene_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()?;
    Some(row.0)
}

// ─── commands ───────────────────────────────────────────────────────────

/// Stage a queue for a not-yet-opened player window. Called by the catalog
/// window immediately before opening the player. The player claims it on mount.
#[tauri::command]
pub async fn stage_player_queue(
    state: State<'_, AppState>,
    label: String,
    scene_ids: Vec<String>,
    start_index: Option<usize>,
    shuffle_by_default: Option<bool>,
) -> Result<(), String> {
    let q = StagedQueue {
        scene_ids,
        start_index: start_index.unwrap_or(0),
        shuffle_by_default: shuffle_by_default.unwrap_or(false),
    };
    state.player_stash.insert(label, q);
    Ok(())
}

/// Claim (pop) the staged queue for this player window. Called once on mount.
/// Returns None if nothing was staged (e.g. window opened via a single-scene
/// Play that bypassed the stash).
#[tauri::command]
pub async fn claim_player_queue(
    state: State<'_, AppState>,
    label: String,
) -> Result<Option<StagedQueue>, String> {
    Ok(state.player_stash.claim(&label))
}

/// Resolve the playable file path for a single scene. Used by the player to
/// load each scene in the queue as it advances (it only knows scene IDs).
#[tauri::command]
pub async fn scene_file_path(
    state: State<'_, AppState>,
    scene_id: String,
) -> Result<Option<String>, String> {
    Ok(resolve_file_path(&state.pool, &scene_id).await)
}

/// Scrubber sprite + VTT text for hover previews. Reads the VTT from disk so
/// the player webview does not need to `fetch()` asset-protocol URLs (unreliable
/// in WebView2).
#[derive(Clone, serde::Serialize)]
pub struct SceneScrubPreview {
    pub sprite_path: String,
    pub vtt_text: String,
}

#[tauri::command]
pub async fn scene_scrub_preview(
    state: State<'_, AppState>,
    scene_id: String,
) -> Result<Option<SceneScrubPreview>, String> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT thumb_sprite_path, vtt_path FROM files
         WHERE scene_id = ? AND thumb_sprite_path IS NOT NULL AND vtt_path IS NOT NULL
         ORDER BY scanned_at DESC LIMIT 1",
    )
    .bind(&scene_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| e.to_string())?;

    let Some((sprite_path, vtt_path)) = row else {
        return Ok(None);
    };

    let vtt_text =
        std::fs::read_to_string(&vtt_path).map_err(|e| format!("read vtt {}: {e}", vtt_path))?;
    Ok(Some(SceneScrubPreview {
        sprite_path,
        vtt_text,
    }))
}

/// Close every open player window. Catalog frontend cannot reliably close
/// sibling windows under Tauri ACL — do it from Rust instead.
#[tauri::command]
pub fn close_all_player_windows(app: AppHandle) -> Result<u32, String> {
    let mut closed = 0u32;
    for (label, window) in app.webview_windows() {
        if label.starts_with("player-") {
            window.close().map_err(|e| e.to_string())?;
            closed += 1;
        }
    }
    Ok(closed)
}
