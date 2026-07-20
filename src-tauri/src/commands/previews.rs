//! Preview generation command. Delegates to `previews_job::run` in a background
//! task so the UI stays responsive.

use tauri::{AppHandle, State};
use tracing::warn;

use crate::job_parallel::{begin_cancellable_job, cancel_cancellable_job, end_cancellable_job};
use crate::AppState;

#[tauri::command]
pub async fn generate_previews(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let token = begin_cancellable_job(&state.preview_cancel).await?;
    let pool = state.inner().pool.clone();
    let slot = state.inner().preview_cancel.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = crate::previews_job::run(&pool, &app, token).await {
            warn!(error = %e, "preview generation job failed");
        }
        end_cancellable_job(&slot).await;
    });
    Ok(())
}

#[tauri::command]
pub async fn cancel_previews(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(cancel_cancellable_job(&state.preview_cancel).await)
}
