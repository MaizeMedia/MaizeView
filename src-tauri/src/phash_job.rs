//! Background pHash fingerprint generation (post-scan, like MD5/previews).

use std::{
    path::{Path, PathBuf},
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
    fingerprints,
    job_parallel::{
        distinct_drives, drive_key, media_job_workers, per_drive_workers, throttled_emit,
        DriveLimiter, ProgressEvent,
    },
    scanner::phash,
};

pub const PROGRESS_EVENT: &str = "phash://progress";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PhashProgress {
    pub done: u64,
    pub total: u64,
    pub current_path: Option<String>,
    pub finished: bool,
    #[serde(default)]
    pub cancelled: bool,
}

impl ProgressEvent for PhashProgress {
    fn is_terminal(&self) -> bool {
        self.finished || self.cancelled
    }
}

pub async fn run(
    pool: &SqlitePool,
    app: &AppHandle,
    cancel: Arc<CancellationToken>,
) -> anyhow::Result<()> {
    run_with_options(pool, app, false, cancel).await
}

/// Wipe existing pHash rows then recompute (use after algorithm upgrades).
pub async fn run_rebuild(
    pool: &SqlitePool,
    app: &AppHandle,
    cancel: Arc<CancellationToken>,
) -> anyhow::Result<()> {
    run_with_options(pool, app, true, cancel).await
}

async fn run_with_options(
    pool: &SqlitePool,
    app: &AppHandle,
    rebuild: bool,
    cancel: Arc<CancellationToken>,
) -> anyhow::Result<()> {
    let app = app.clone();
    run_inner(
        pool,
        rebuild,
        cancel,
        throttled_emit(move |p: PhashProgress| {
            let _ = app.emit(PROGRESS_EVENT, &p);
        }),
    )
    .await
}

pub async fn run_silent(pool: &SqlitePool) -> anyhow::Result<()> {
    run_inner(
        pool,
        false,
        Arc::new(CancellationToken::new()),
        Arc::new(|_| {}),
    )
    .await
}

async fn run_inner(
    pool: &SqlitePool,
    rebuild: bool,
    cancel: Arc<CancellationToken>,
    emit: Arc<dyn Fn(PhashProgress) + Send + Sync>,
) -> anyhow::Result<()> {
    if rebuild {
        let deleted = sqlx::query("DELETE FROM fingerprints WHERE hash_type = 'phash'")
            .execute(pool)
            .await?
            .rows_affected();
        tracing::info!(deleted, "cleared pHash fingerprints for rebuild");
    }

    let rows: Vec<(String, String, Option<f64>, Option<String>)> = sqlx::query_as(
        r#"
        SELECT f.id, f.path, f.duration, oshash.value
        FROM files f
        LEFT JOIN fingerprints ph ON ph.file_id = f.id AND ph.hash_type = 'phash'
        LEFT JOIN fingerprints oshash ON oshash.file_id = f.id AND oshash.hash_type = 'oshash'
        WHERE ph.id IS NULL AND f.duration IS NOT NULL AND f.duration > 0
        ORDER BY f.scanned_at DESC
        "#,
    )
    .fetch_all(pool)
    .await?;

    let total = rows.len() as u64;
    if total == 0 {
        emit(PhashProgress {
            done: 0,
            total: 0,
            current_path: None,
            finished: true,
            cancelled: false,
        });
        return Ok(());
    }

    let workers = media_job_workers();
    let drives = distinct_drives(rows.iter().map(|(_, p, _, _)| Path::new(p.as_str())));
    let per_drive = per_drive_workers(workers, drives);
    tracing::info!(
        total,
        workers,
        drives,
        per_drive,
        "computing missing pHash fingerprints"
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
                let Some((file_id, path, duration, oshash)) = rows.get(i).cloned() else {
                    return;
                };
                let drive = drive_key(Path::new(&path));
                let _permit = limiter.acquire(&drive).await;
                if cancel.is_cancelled() {
                    return;
                }

                emit(PhashProgress {
                    done: done.load(Ordering::Relaxed),
                    total,
                    current_path: Some(path.clone()),
                    finished: false,
                    cancelled: false,
                });

                let duration = duration.unwrap_or(0.0);
                if duration <= 0.0 {
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    emit(PhashProgress {
                        done: n,
                        total,
                        current_path: Some(path),
                        finished: false,
                        cancelled: false,
                    });
                    continue;
                }

                let reused = if let Some(ref osh) = oshash {
                    fingerprints::find_phash_by_oshash(&pool, osh)
                        .await
                        .ok()
                        .flatten()
                } else {
                    None
                };

                let digest = if let Some(existing) = reused {
                    Some(existing)
                } else {
                    let path_buf = PathBuf::from(&path);
                    match tokio::task::spawn_blocking(move || phash::hash_file(&path_buf, duration))
                        .await
                    {
                        Ok(Ok(d)) => Some(d),
                        Ok(Err(e)) => {
                            tracing::warn!(error = %e, path, "phash computation failed");
                            None
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, path, "phash task join failed");
                            None
                        }
                    }
                };

                if let Some(digest) = digest {
                    if let Err(e) = fingerprints::upsert(&pool, &file_id, "phash", &digest).await {
                        tracing::warn!(error = %e, file_id, path, "storing phash fingerprint failed");
                    }
                }

                let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                emit(PhashProgress {
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
            "pHash generation cancelled"
        );
    }
    emit(PhashProgress {
        done: done.load(Ordering::Relaxed),
        total,
        current_path: None,
        finished: true,
        cancelled,
    });
    Ok(())
}
