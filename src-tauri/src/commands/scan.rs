//! Scan command: starts a library scan in the background, reports progress
//! over the Tauri event bus, persists a scan_runs row, and supports cancel.

use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::{
    commands::err,
    job_parallel::{begin_cancellable_job, cancel_cancellable_job, end_cancellable_job},
    models::{new_id, now, ScanPath, ScanProgress},
    scanner, AppState,
};

const PROGRESS_EVENT: &str = "scan://progress";

#[tauri::command]
pub async fn start_scan(app: AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    // Snapshot the configured paths.
    let paths: Vec<ScanPath> = sqlx::query_as("SELECT id, path, label, created_at FROM scan_paths")
        .fetch_all(&state.pool)
        .await
        .map_err(err)?;
    if paths.is_empty() {
        return Err("no scan paths configured".into());
    }

    let mut missing_paths: Vec<String> = Vec::new();
    let path_strings: Vec<String> = paths
        .iter()
        .filter_map(|p| {
            if std::path::Path::new(&p.path).is_dir() {
                Some(p.path.clone())
            } else {
                missing_paths.push(p.path.clone());
                None
            }
        })
        .collect();

    if path_strings.is_empty() {
        return Err(format!(
            "scan paths unavailable (offline or missing): {}",
            missing_paths.join(", ")
        ));
    }
    if !missing_paths.is_empty() {
        warn!(paths = ?missing_paths, "skipping unavailable scan paths");
    }

    let skipped_paths = if missing_paths.is_empty() {
        None
    } else {
        Some(missing_paths.clone())
    };

    // Create the scan_run row up-front so the UI can reference it.
    let scan_run_id = new_id();
    let started_at = now().to_rfc3339();
    sqlx::query(
        "INSERT INTO scan_runs (id, started_at, status, paths_scanned)
         VALUES (?, ?, 'running', ?)",
    )
    .bind(&scan_run_id)
    .bind(&started_at)
    .bind(path_strings.len() as i64)
    .execute(&state.pool)
    .await
    .map_err(err)?;

    // Fresh cancellation token for this scan; store it so cancel_scan can fire.
    let token = CancellationToken::new();
    {
        let mut guard = state.scan_cancel.lock().await;
        *guard = Some(token.clone());
    }

    if skipped_paths.is_some() {
        let _ = app.emit(
            PROGRESS_EVENT,
            &ScanProgress {
                scan_run_id: scan_run_id.clone(),
                status: "running".into(),
                phase: "walking".into(),
                files_found: 0,
                files_added: 0,
                files_updated: 0,
                files_removed: 0,
                files_processed: 0,
                current_path: None,
                skipped_paths: skipped_paths.clone(),
            },
        );
    }

    // Spawn the scan as a background task. Clone the pool + token out of state.
    let pool = state.inner().pool.clone();
    let app_for_sink = app.clone();
    let app_for_final_emit = app.clone();
    let app_for_previews = app.clone();
    let scan_cancel = state.inner().scan_cancel.clone();
    let preview_cancel = state.inner().preview_cancel.clone();
    let md5_cancel = state.inner().md5_cancel.clone();
    let phash_cancel = state.inner().phash_cancel.clone();
    let scan_run_id_task = scan_run_id.clone();

    tauri::async_runtime::spawn(async move {
        let progress_sink: scanner::ProgressSink = Arc::new(move |p: ScanProgress| {
            let _ = app_for_sink.emit(PROGRESS_EVENT, &p);
        });

        let result = scanner::scan_with_protect(
            &pool,
            &scan_run_id_task,
            &path_strings,
            &missing_paths,
            progress_sink,
            token,
        )
        .await;

        // Map the result into (status, counts). scan() returns Ok on success,
        // Err(ScanCancelled) on user cancel. The scanner emits its own progress
        // events (including a cancelled event with the kept counts), so on the
        // cancel path we don't re-emit — the scanner's event is authoritative.
        let (status, counts) = match result {
            Ok(p) => ("completed", p),
            Err(scanner::ScanCancelled) => {
                info!(scan_run_id = %scan_run_id_task, "scan cancelled by user; partials kept");
                // No re-emit: scanner::finalize_cancelled already sent the
                // cancelled progress with the real kept counts. Use a minimal
                // placeholder for the scan_runs row update below.
                (
                    "cancelled",
                    ScanProgress {
                        scan_run_id: scan_run_id_task.clone(),
                        status: "cancelled".into(),
                        phase: "done".into(),
                        files_found: 0,
                        files_added: 0,
                        files_updated: 0,
                        files_removed: 0,
                        files_processed: 0,
                        current_path: None,
                        skipped_paths: None,
                    },
                )
            }
        };

        // On the completed path, emit a final settle event. On cancel, the
        // scanner's emitted event already carries the real counts.
        if status == "completed" {
            let _ = app_for_final_emit.emit(PROGRESS_EVENT, &counts);
        }

        // Close the scan_runs row. For cancelled scans we don't have the final
        // counts here (they were emitted but not returned), so write zeros —
        // the authoritative counts live in the emitted progress event + the
        // actual DB rows that were written.
        let finished = now().to_rfc3339();
        let _ = sqlx::query(
            r#"
            UPDATE scan_runs SET
                finished_at = ?,
                status = ?,
                files_found = ?,
                files_added = ?,
                files_updated = ?,
                files_removed = ?,
                error_message = NULL
            WHERE id = ?
            "#,
        )
        .bind(&finished)
        .bind(status)
        .bind(counts.files_found)
        .bind(counts.files_added)
        .bind(counts.files_updated)
        .bind(counts.files_removed)
        .bind(&scan_run_id_task)
        .execute(&pool)
        .await;

        // Clear the active token.
        {
            let mut guard = scan_cancel.lock().await;
            *guard = None;
        }

        info!(scan_run_id = %scan_run_id_task, status, "scan task finalized");

        // Auto-kick preview + MD5/pHash on successful scan (cancellable from Settings).
        if status == "completed" {
            let _ = cancel_cancellable_job(&preview_cancel).await;
            let _ = cancel_cancellable_job(&md5_cancel).await;
            let _ = cancel_cancellable_job(&phash_cancel).await;

            let preview_token = begin_cancellable_job(&preview_cancel)
                .await
                .unwrap_or_else(|_| Arc::new(CancellationToken::new()));
            let md5_token = begin_cancellable_job(&md5_cancel)
                .await
                .unwrap_or_else(|_| Arc::new(CancellationToken::new()));
            let phash_token = begin_cancellable_job(&phash_cancel)
                .await
                .unwrap_or_else(|_| Arc::new(CancellationToken::new()));

            let pool_previews = pool.clone();
            let pool_md5 = pool.clone();
            let pool_phash = pool.clone();
            let app_jobs = app_for_previews.clone();
            let (preview_result, md5_result, phash_result) = tokio::join!(
                crate::previews_job::run(&pool_previews, &app_jobs, preview_token),
                crate::fingerprints_job::run(&pool_md5, &app_jobs, md5_token),
                crate::phash_job::run(&pool_phash, &app_jobs, phash_token),
            );
            end_cancellable_job(&preview_cancel).await;
            end_cancellable_job(&md5_cancel).await;
            end_cancellable_job(&phash_cancel).await;
            if let Err(e) = preview_result {
                warn!(error = %e, "auto preview generation failed");
            }
            if let Err(e) = md5_result {
                warn!(error = %e, "auto MD5 fingerprint generation failed");
            }
            if let Err(e) = phash_result {
                warn!(error = %e, "auto pHash fingerprint generation failed");
            }
        }
    });

    Ok(scan_run_id)
}

#[tauri::command]
pub async fn cancel_scan(state: State<'_, AppState>) -> Result<bool, String> {
    let mut guard = state.scan_cancel.lock().await;
    if let Some(token) = guard.take() {
        token.cancel();
        Ok(true)
    } else {
        Ok(false) // nothing running
    }
}
