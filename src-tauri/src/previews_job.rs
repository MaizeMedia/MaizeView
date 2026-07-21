//! Shared preview-generation job. Used by both the `generate_previews` command
//! and the post-scan auto-trigger so the logic lives in one place.

use std::{
    path::Path,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
};

use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::{
    job_parallel::{
        distinct_drives, drive_key, media_job_workers, per_drive_workers, throttled_emit,
        DriveLimiter, ProgressEvent,
    },
    models::now,
    previews,
};

pub const PROGRESS_EVENT: &str = "preview://progress";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PreviewProgress {
    pub done: u64,
    pub total: u64,
    pub current_path: Option<String>,
    pub finished: bool,
    #[serde(default)]
    pub cancelled: bool,
}

impl ProgressEvent for PreviewProgress {
    fn is_terminal(&self) -> bool {
        self.finished || self.cancelled
    }
}

/// Generate thumbnails/sprites for files missing grid thumbs.
pub async fn run(
    pool: &SqlitePool,
    app: &AppHandle,
    cancel: Arc<CancellationToken>,
) -> anyhow::Result<()> {
    let app = app.clone();
    run_inner(
        pool,
        cancel,
        throttled_emit(move |p: PreviewProgress| {
            let _ = app.emit(PROGRESS_EVENT, &p);
        }),
    )
    .await
}

/// Same job without Tauri (CLI / tests).
pub async fn run_silent(pool: &SqlitePool) -> anyhow::Result<()> {
    run_inner(pool, Arc::new(CancellationToken::new()), Arc::new(|_| {})).await
}

async fn run_inner(
    pool: &SqlitePool,
    cancel: Arc<CancellationToken>,
    emit: Arc<dyn Fn(PreviewProgress) + Send + Sync>,
) -> anyhow::Result<()> {
    let rows: Vec<(String, String, Option<f64>)> = sqlx::query_as(
        "SELECT id, path, duration FROM files
         WHERE duration IS NOT NULL AND thumb_path IS NULL
         ORDER BY scanned_at DESC",
    )
    .fetch_all(pool)
    .await?;

    let total = rows.len() as u64;
    if total == 0 {
        emit(PreviewProgress {
            done: 0,
            total: 0,
            current_path: None,
            finished: true,
            cancelled: false,
        });
        return Ok(());
    }

    let workers = media_job_workers();
    let drives = distinct_drives(rows.iter().map(|(_, p, _)| Path::new(p.as_str())));
    let per_drive = per_drive_workers(workers, drives);
    tracing::info!(
        total,
        workers,
        drives,
        per_drive,
        "generating missing previews"
    );

    let limiter = Arc::new(DriveLimiter::new(workers, per_drive));
    let rows = Arc::new(rows);
    let next = Arc::new(AtomicUsize::new(0));
    let done = Arc::new(AtomicU64::new(0));
    let mut set = JoinSet::new();

    // Fixed worker pool pulling from a shared index instead of one task per
    // file; per-drive caps still apply via the limiter inside the loop.
    for _ in 0..workers {
        let pool = pool.clone();
        let emit = emit.clone();
        let limiter = limiter.clone();
        let rows = rows.clone();
        let next = next.clone();
        let done = done.clone();
        let cancel = cancel.clone();
        set.spawn(async move {
            loop {
                if cancel.is_cancelled() {
                    return;
                }
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some((file_id, path, duration)) = rows.get(i).cloned() else {
                    return;
                };
                let drive = drive_key(Path::new(&path));
                let _permit = limiter.acquire(&drive).await;
                if cancel.is_cancelled() {
                    return;
                }

                emit(PreviewProgress {
                    done: done.load(Ordering::Relaxed),
                    total,
                    current_path: Some(path.clone()),
                    finished: false,
                    cancelled: false,
                });

                let path_for_job = path.clone();
                let file_id_for_job = file_id.clone();
                let gen = tokio::task::spawn_blocking(move || {
                    previews::generate(&file_id_for_job, Path::new(&path_for_job), duration)
                })
                .await;

                match gen {
                    Ok(Ok(Some((thumb, sprite, vtt)))) => {
                        let ts = now().to_rfc3339();
                        if let Err(e) = sqlx::query(
                            "UPDATE files SET thumb_path = ?, thumb_sprite_path = ?, vtt_path = ?, scanned_at = ? WHERE id = ?",
                        )
                        .bind(thumb.to_string_lossy().to_string())
                        .bind(sprite.to_string_lossy().to_string())
                        .bind(vtt.to_string_lossy().to_string())
                        .bind(ts)
                        .bind(&file_id)
                        .execute(&pool)
                        .await
                        {
                            tracing::warn!(error = %e, file_id, "updating preview paths failed");
                        }
                    }
                    Ok(Ok(None)) => {}
                    Ok(Err(e)) => tracing::warn!(error = %e, path, "preview generation failed"),
                    Err(e) => tracing::warn!(error = %e, path, "preview task join failed"),
                }

                let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                emit(PreviewProgress {
                    done: n,
                    total,
                    current_path: Some(path),
                    finished: false,
                    cancelled: false,
                });
            }
        });
    }

    while set.join_next().await.is_some() {}

    let cancelled = cancel.is_cancelled();
    if cancelled {
        tracing::info!(
            done = done.load(Ordering::Relaxed),
            total,
            "preview generation cancelled"
        );
    }
    emit(PreviewProgress {
        done: done.load(Ordering::Relaxed),
        total,
        current_path: None,
        finished: true,
        cancelled,
    });
    Ok(())
}
