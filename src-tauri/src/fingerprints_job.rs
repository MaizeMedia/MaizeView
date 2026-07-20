//! Background MD5 fingerprint generation — runs after scan (like previews).
//!
//! Full-file MD5 is too slow to block the scan loop; we queue it here so
//! StashDB identify has both OSHASH + MD5 without a manual drawer click.

use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
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
        distinct_drives, drive_key, media_job_workers, per_drive_workers, rotational_drives,
        DriveLimiter,
    },
    scanner::md5,
};

pub const PROGRESS_EVENT: &str = "fingerprint://progress";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FingerprintProgress {
    pub done: u64,
    pub total: u64,
    pub current_path: Option<String>,
    pub finished: bool,
    #[serde(default)]
    pub cancelled: bool,
}

/// Compute missing MD5 fingerprints. Emits `fingerprint://progress` when `app` is set.
pub async fn run(
    pool: &SqlitePool,
    app: &AppHandle,
    cancel: Arc<CancellationToken>,
) -> anyhow::Result<()> {
    let app = app.clone();
    run_inner(
        pool,
        cancel,
        Arc::new(move |p| {
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
    emit: Arc<dyn Fn(FingerprintProgress) + Send + Sync>,
) -> anyhow::Result<()> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        r#"
        SELECT f.id, f.path
        FROM files f
        LEFT JOIN fingerprints fp ON fp.file_id = f.id AND fp.hash_type = 'md5'
        WHERE fp.id IS NULL
        ORDER BY f.scanned_at DESC
        "#,
    )
    .fetch_all(pool)
    .await?;

    let total = rows.len() as u64;
    if total == 0 {
        emit(FingerprintProgress {
            done: 0,
            total: 0,
            current_path: None,
            finished: true,
            cancelled: false,
        });
        return Ok(());
    }

    let workers = media_job_workers();
    let drives = distinct_drives(rows.iter().map(|(_, p)| Path::new(p.as_str())));
    let per_drive = per_drive_workers(workers, drives);
    tracing::info!(
        total,
        workers,
        drives,
        per_drive,
        "computing missing MD5 fingerprints"
    );

    // MD5 is pure sequential I/O: on spinning disks, concurrent full-file
    // readers just move the head back and forth. One reader per HDD
    // (probed via PowerShell; SSDs keep the normal budget).
    let caps: std::collections::HashMap<String, usize> = rotational_drives()
        .into_iter()
        .map(|d| (d, 1usize))
        .collect();
    let limiter = Arc::new(DriveLimiter::with_caps(workers, per_drive, caps));
    let done = Arc::new(AtomicU64::new(0));
    let mut set = JoinSet::new();

    for (file_id, path) in rows {
        if cancel.is_cancelled() {
            break;
        }
        let pool = pool.clone();
        let emit = emit.clone();
        let limiter = limiter.clone();
        let done = done.clone();
        let cancel = cancel.clone();
        set.spawn(async move {
            if cancel.is_cancelled() {
                return;
            }
            let drive = drive_key(Path::new(&path));
            let _permit = limiter.acquire(&drive).await;
            if cancel.is_cancelled() {
                return;
            }

            emit(FingerprintProgress {
                done: done.load(Ordering::Relaxed),
                total,
                current_path: Some(path.clone()),
                finished: false,
                cancelled: false,
            });

            let path_buf = PathBuf::from(&path);
            let digest = match tokio::task::spawn_blocking(move || md5::hash_file(&path_buf)).await
            {
                Ok(Ok(d)) => Some(d),
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, path, "md5 hash failed");
                    None
                }
                Err(e) => {
                    tracing::warn!(error = %e, path, "md5 task join failed");
                    None
                }
            };

            if let Some(digest) = digest {
                if let Err(e) = fingerprints::upsert(&pool, &file_id, "md5", &digest).await {
                    tracing::warn!(error = %e, file_id, path, "storing md5 fingerprint failed");
                }
            }

            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            emit(FingerprintProgress {
                done: n,
                total,
                current_path: Some(path),
                finished: false,
                cancelled: false,
            });
        });
    }

    while set.join_next().await.is_some() {}

    let cancelled = cancel.is_cancelled();
    if cancelled {
        tracing::info!(
            done = done.load(Ordering::Relaxed),
            total,
            "MD5 fingerprint generation cancelled"
        );
    }
    emit(FingerprintProgress {
        done: done.load(Ordering::Relaxed),
        total,
        current_path: None,
        finished: true,
        cancelled,
    });
    Ok(())
}
