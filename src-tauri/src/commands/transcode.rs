//! Tauri commands for the downscale/convert feature.
//!
//!   * `downscale_preview` — non-mutating plan: feeds the Convert dialog.
//!   * `downscale_start`   — launches the transcode job in the background.
//!   * `downscale_cancel`  — cooperatively cancels the running job.
//!
//! Mirrors the scan command's pattern: a cancellation token held in
//! `AppState`, a background `tauri::async_runtime::spawn`, and progress
//! reported over the `transcode://progress` event by the job itself.

use std::sync::Arc;

use tauri::{AppHandle, State};
use tokio_util::sync::CancellationToken;

use crate::{commands::err, transcode_job, AppState};

/// Non-mutating preview of what a downscale run would do. Returns the
/// breakdown (counts by current resolution), skip count, estimated savings,
/// and a per-scene before→after filename preview. Never touches disk.
#[tauri::command]
pub async fn downscale_preview(
    state: State<'_, AppState>,
    scene_ids: Vec<String>,
    target_height: u32,
) -> Result<transcode_job::DownscalePreview, String> {
    transcode_job::build_preview(&state.pool, &scene_ids, target_height)
        .await
        .map_err(err)
}

/// Launch a downscale run. Returns immediately; progress is emitted on
/// `transcode://progress`. If a transcode is already running, this refuses.
#[tauri::command]
pub async fn downscale_start(
    app: AppHandle,
    state: State<'_, AppState>,
    opts: transcode_job::DownscaleOptions,
) -> Result<bool, String> {
    // Reject a second concurrent run. The token, if present, is owned by the
    // running job; presence means a job is in flight.
    {
        let guard = state.transcode_cancel.lock().await;
        if guard.is_some() {
            return Err("a transcode is already running".into());
        }
    }

    // Refuse an empty selection up front (otherwise the job no-ops silently).
    if opts.scene_ids.is_empty() {
        return Err("no scenes selected".into());
    }

    let token = Arc::new(CancellationToken::new());
    {
        let mut guard = state.transcode_cancel.lock().await;
        *guard = Some(token.clone());
    }

    let pool = state.inner().pool.clone();
    let cancel_slot = state.inner().transcode_cancel.clone();
    let app_handle = app.clone();

    tauri::async_runtime::spawn(async move {
        let result = transcode_job::run(&pool, &app_handle, opts, token.clone()).await;
        // Always clear the slot so a new run can start, even on error/cancel.
        let mut guard = cancel_slot.lock().await;
        *guard = None;
        if let Err(e) = result {
            tracing::warn!(error = %e, "transcode job ended with error");
        }
    });

    Ok(true)
}

/// Cancel the running transcode, if any. Returns whether a job was running.
#[tauri::command]
pub async fn downscale_cancel(state: State<'_, AppState>) -> Result<bool, String> {
    let mut guard = state.transcode_cancel.lock().await;
    if let Some(token) = guard.take() {
        token.cancel();
        Ok(true)
    } else {
        Ok(false)
    }
}
