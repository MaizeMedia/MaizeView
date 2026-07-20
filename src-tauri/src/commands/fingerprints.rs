//! Manual fingerprint generation commands.

use tauri::{AppHandle, State};
use tracing::warn;

use crate::job_parallel::{begin_cancellable_job, cancel_cancellable_job, end_cancellable_job};
use crate::{fingerprints_job, phash_job, AppState};

#[tauri::command]
pub async fn generate_md5_fingerprints(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let token = begin_cancellable_job(&state.md5_cancel).await?;
    let pool = state.inner().pool.clone();
    let slot = state.inner().md5_cancel.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = fingerprints_job::run(&pool, &app, token).await {
            warn!(error = %e, "MD5 fingerprint job failed");
        }
        end_cancellable_job(&slot).await;
    });
    Ok(())
}

#[tauri::command]
pub async fn cancel_md5_fingerprints(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(cancel_cancellable_job(&state.md5_cancel).await)
}

/// Generate (or rebuild) pHash fingerprints.
/// When `rebuild` is true, existing pHash rows are deleted first.
#[tauri::command]
pub async fn generate_phash_fingerprints(
    app: AppHandle,
    state: State<'_, AppState>,
    rebuild: Option<bool>,
) -> Result<(), String> {
    let token = begin_cancellable_job(&state.phash_cancel).await?;
    let pool = state.inner().pool.clone();
    let slot = state.inner().phash_cancel.clone();
    let rebuild = rebuild.unwrap_or(false);
    tauri::async_runtime::spawn(async move {
        let result = if rebuild {
            phash_job::run_rebuild(&pool, &app, token).await
        } else {
            phash_job::run(&pool, &app, token).await
        };
        if let Err(e) = result {
            warn!(error = %e, "pHash fingerprint job failed");
        }
        end_cancellable_job(&slot).await;
    });
    Ok(())
}

#[tauri::command]
pub async fn cancel_phash_fingerprints(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(cancel_cancellable_job(&state.phash_cancel).await)
}

/// Stop previews + MD5 + pHash jobs (post-scan auto jobs or Settings).
#[tauri::command]
pub async fn cancel_media_jobs(state: State<'_, AppState>) -> Result<bool, String> {
    let a = cancel_cancellable_job(&state.preview_cancel).await;
    let b = cancel_cancellable_job(&state.md5_cancel).await;
    let c = cancel_cancellable_job(&state.phash_cancel).await;
    Ok(a || b || c)
}
