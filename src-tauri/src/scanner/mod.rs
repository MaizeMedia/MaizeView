//! Library scanner — discovers video files under configured paths, hashes them,
//! probes them with ffprobe, and upserts rows into the DB.
//!
//! Pipeline (idempotent re-scans detect add/move/modify/delete cheaply via the
//! existing oshash):
//!   1. Walk each scan path, collecting video files by extension.
//!   2. For each file: compute oshash + stat + ffprobe (parallel via rayon).
//!   3. DB upsert:
//!        - oshash seen → re-link path (cheap move detection)
//!        - same path+size+mtime → skip (no change since last scan)
//!        - changed path+size/mtime → update row + refresh oshash, invalidate md5
//!        - otherwise → new file row + (new scene row if no oshash match)
//!   4. Mark any DB file whose path vanished as removed; prune scenes with no files left.
//!      Paths under offline/skipped scan roots are left alone.
//!   5. Post-scan (scan command): background MD5 fingerprints + previews.
//!
//! Progress is reported via a channel so the Tauri command layer can emit
//! events to the frontend.

pub mod md5;
pub mod oshash;
pub mod phash;
pub mod probe;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Result;
use rayon::prelude::*;
use sqlx::{SqliteConnection, SqlitePool};
use tracing::{info, warn};

use crate::models::{new_id, now, ScanProgress};

/// Extensions we treat as videos. Conservative list; expand as needed.
const VIDEO_EXTS: &[&str] = &[
    "mp4", "m4v", "mkv", "webm", "mov", "avi", "wmv", "flv", "mpg", "mpeg", "ts", "m2ts", "ogv",
    "3gp", "rm", "vob",
];

/// One discovered file on disk.
#[derive(Clone)]
struct FoundFile {
    path: PathBuf,
    size: u64,
    mtime: std::time::SystemTime,
}

/// Per-file work output from the parallel stage.
struct IndexedFile {
    found: FoundFile,
    oshash: Result<String, String>,
    probe: probe::ProbeSummary,
}

/// Progress callback used by the scan loop. The Tauri command layer wires this
/// to an event emitter.
pub type ProgressSink = Arc<dyn Fn(ScanProgress) + Send + Sync>;

/// Error returned when a scan is cancelled via its CancellationToken.
#[derive(Debug, thiserror::Error)]
#[error("scan cancelled")]
pub struct ScanCancelled;

/// Run a full scan over the given paths. Cooperative cancellation via `cancel`:
/// checked between batches. On cancel, **everything written so far is kept** —
/// at most one batch's worth of hashing work (≤ BATCH files) is lost — and the
/// next scan effectively resumes because the scanner is idempotent (oshash +
/// size+mtime shortcuts skip already-indexed files).
///
/// Indexing and DB writing are interleaved per batch so a cancel loses the
/// minimum possible work.
pub async fn scan(
    pool: &SqlitePool,
    scan_run_id: &str,
    paths: &[String],
    progress: ProgressSink,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<ScanProgress, ScanCancelled> {
    scan_with_protect(pool, scan_run_id, paths, &[], progress, cancel).await
}

/// Like [`scan`], but never deletes catalog files under `protect_prefixes`
/// (typically offline scan roots skipped for this run).
pub async fn scan_with_protect(
    pool: &SqlitePool,
    scan_run_id: &str,
    paths: &[String],
    protect_prefixes: &[String],
    progress: ProgressSink,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<ScanProgress, ScanCancelled> {
    info!(paths = paths.len(), "scan starting");

    // ── Phase: walking ────────────────────────────────────────────────────
    emit(
        &progress,
        scan_run_id,
        "running",
        "walking",
        0,
        0,
        0,
        0,
        0,
        None,
    );
    if cancel.is_cancelled() {
        return Err(ScanCancelled);
    }
    let found: Vec<FoundFile> = paths.iter().flat_map(|p| walk_path(p)).collect();
    let files_found = found.len() as i64;

    // Snapshot of existing DB files (path → row) for cheap diffing. If this
    // fails, fall back to an empty snapshot — every file becomes "new" rather
    // than aborting the whole scan.
    let existing = match load_existing_files(pool).await {
        Ok(m) => m,
        Err(e) => {
            warn!(error = %e, "loading existing files snapshot failed; treating all as new");
            HashMap::new()
        }
    };

    // ── Phase: index + write, interleaved per batch ───────────────────────
    // Each batch: hash+probe in parallel, then upsert the whole batch before
    // touching the next. Cancel between batches loses nothing; cancel mid-batch
    // loses only that batch's hashing (its DB writes haven't started).
    emit(
        &progress,
        scan_run_id,
        "running",
        "indexing",
        files_found,
        0,
        0,
        0,
        0,
        None,
    );

    const BATCH: usize = 32;
    let mut added = 0i64;
    let mut updated = 0i64;
    let mut processed: i64 = 0;
    let mut processed_paths: Vec<String> = Vec::with_capacity(found.len());

    // One pool for the whole scan — worker count from Settings (job intensity).
    let workers = crate::job_parallel::media_job_workers();
    info!(workers, "scan index thread pool");
    let index_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .thread_name(|i| format!("scan-index-{i}"))
        .build()
        .unwrap_or_else(|e| {
            warn!(error = %e, "custom scan pool failed; using default rayon pool");
            rayon::ThreadPoolBuilder::new()
                .build()
                .expect("default rayon pool")
        });

    for chunk in found.chunks(BATCH) {
        if cancel.is_cancelled() {
            // Everything from prior batches is already on disk. Report partials.
            return Err(finalize_cancelled(
                &progress,
                scan_run_id,
                files_found,
                added,
                updated,
                processed,
            ));
        }

        // Hash + probe this batch in parallel.
        let batch: Vec<IndexedFile> = index_pool.install(|| {
            chunk
                .par_iter()
                .map(|f| {
                    let oshash = oshash::hash_file(&f.path).map_err(|e| e.to_string());
                    let probe = probe::probe(&f.path).unwrap_or_default();
                    IndexedFile {
                        found: f.clone(),
                        oshash,
                        probe,
                    }
                })
                .collect()
        });

        // Write the whole batch sequentially in a single transaction (SQLite is
        // single-writer anyway) — one commit per batch instead of per statement.
        // If cancelled mid-batch, we still finish writing this batch — it's
        // already hashed, so discarding it would waste that work. The cancel
        // check at the top of the next iteration stops promptly after.
        let mut tx = match pool.begin().await {
            Ok(tx) => tx,
            Err(e) => {
                // Still record the paths so removed-file detection below
                // doesn't delete rows we merely failed to re-index.
                warn!(error = %e, "batch transaction begin failed; skipping batch writes");
                for item in &batch {
                    processed_paths.push(crate::paths::normalize_windows_path(
                        &item.found.path.to_string_lossy(),
                    ));
                    processed += 1;
                }
                continue;
            }
        };
        for item in &batch {
            match upsert_file(&mut tx, item, &existing).await {
                Ok(WriteOutcome::Added) => added += 1,
                Ok(WriteOutcome::Updated) => updated += 1,
                Ok(WriteOutcome::Unchanged) => {}
                Err(e) => warn!(path = %item.found.path.display(), error = %e, "upsert failed"),
            }
            processed_paths.push(crate::paths::normalize_windows_path(
                &item.found.path.to_string_lossy(),
            ));
            processed += 1;
        }
        if let Err(e) = tx.commit().await {
            warn!(error = %e, "batch transaction commit failed; batch writes rolled back");
        }

        let current = batch
            .last()
            .map(|b| crate::paths::normalize_windows_path(&b.found.path.to_string_lossy()));
        emit(
            &progress,
            scan_run_id,
            "running",
            "indexing",
            files_found,
            added,
            updated,
            0,
            processed,
            current,
        );
    }

    // ── Phase: removed-file detection (only on a complete scan) ───────────
    // On cancel we skip this — unscanned batches must not be mis-flagged removed.
    // Offline/skipped roots stay in the catalog until that drive is online again.
    let removed = match mark_removed_paths(pool, &processed_paths, protect_prefixes).await {
        Ok(n) => n,
        Err(e) => {
            warn!(error = %e, "removed-file detection failed; skipping");
            0
        }
    };

    let final_progress = ScanProgress {
        scan_run_id: scan_run_id.to_string(),
        status: "completed".into(),
        phase: "done".into(),
        files_found,
        files_added: added,
        files_updated: updated,
        files_removed: removed,
        files_processed: files_found,
        current_path: None,
        skipped_paths: None,
    };
    emit(
        &progress,
        scan_run_id,
        "completed",
        "done",
        files_found,
        added,
        updated,
        removed,
        files_found,
        None,
    );

    info!(files_found, added, updated, removed, "scan complete");
    Ok(final_progress)
}

/// Build the cancelled-result progress (kept counts, status=cancelled) and emit
/// it so the UI reports "kept N of M" rather than just "cancelled".
fn finalize_cancelled(
    progress: &ProgressSink,
    scan_run_id: &str,
    files_found: i64,
    added: i64,
    updated: i64,
    processed: i64,
) -> ScanCancelled {
    emit(
        progress,
        scan_run_id,
        "cancelled",
        "done",
        files_found,
        added,
        updated,
        0,
        processed,
        None,
    );
    ScanCancelled
}

/// Helper to build + send a progress event without juggling all the fields.
#[allow(clippy::too_many_arguments)]
fn emit(
    sink: &ProgressSink,
    scan_run_id: &str,
    status: &str,
    phase: &str,
    files_found: i64,
    files_added: i64,
    files_updated: i64,
    files_removed: i64,
    files_processed: i64,
    current_path: Option<String>,
) {
    sink(ScanProgress {
        scan_run_id: scan_run_id.to_string(),
        status: status.to_string(),
        phase: phase.to_string(),
        files_found,
        files_added,
        files_updated,
        files_removed,
        files_processed,
        current_path,
        skipped_paths: None,
    });
}

enum WriteOutcome {
    Added,
    Updated,
    Unchanged,
}

/// Walk a single configured path, yielding video files.
fn walk_path(root: &str) -> Vec<FoundFile> {
    let root = Path::new(root);
    if !root.is_dir() {
        warn!(path = %root.display(), "scan path is not a directory; skipping");
        return Vec::new();
    }
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let is_video = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| VIDEO_EXTS.contains(&e.to_ascii_lowercase().as_str()))
            .unwrap_or(false);
        if !is_video {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        out.push(FoundFile {
            path: path.to_path_buf(),
            size: meta.len(),
            mtime: meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH),
        });
    }
    out
}

/// Load every existing file row's identity fields so we can diff cheaply.
struct ExistingRow {
    file_id: String,
    size_bytes: i64,
    modified_at: String,
}

async fn load_existing_files(pool: &SqlitePool) -> Result<HashMap<String, ExistingRow>> {
    let rows: Vec<(String, String, i64, String)> =
        sqlx::query_as("SELECT path, id, size_bytes, modified_at FROM files")
            .fetch_all(pool)
            .await?;
    Ok(rows
        .into_iter()
        .map(|(path, file_id, size_bytes, modified_at)| {
            (
                path,
                ExistingRow {
                    file_id,
                    size_bytes,
                    modified_at,
                },
            )
        })
        .collect())
}

/// Decide whether a found file is new/moved/changed and write accordingly.
/// Runs on the batch transaction's connection (`tx`).
async fn upsert_file(
    tx: &mut SqliteConnection,
    item: &IndexedFile,
    existing: &HashMap<String, ExistingRow>,
) -> Result<WriteOutcome> {
    let path_str = crate::paths::normalize_windows_path(&item.found.path.to_string_lossy());
    let mtime_str = system_time_to_rfc3339(item.found.mtime);

    // Path already known?
    if let Some(row) = existing.get(&path_str) {
        // Cheap "unchanged" check: size + mtime.
        if row.size_bytes == item.found.size as i64 && row.modified_at == mtime_str {
            return Ok(WriteOutcome::Unchanged);
        }
        // File changed in place: update the row (keep its id + scene link).
        let oshash = match &item.oshash {
            Ok(h) => h.as_str(),
            Err(e) => {
                warn!(path = %path_str, error = %e, "oshash failed on changed file");
                ""
            }
        };
        update_changed_file(&mut *tx, &row.file_id, item, &mtime_str, oshash).await?;
        return Ok(WriteOutcome::Updated);
    }

    // Path not seen before. Try to re-link by oshash (move detection).
    let oshash = match &item.oshash {
        Ok(h) => h.clone(),
        Err(e) => {
            warn!(path = %path_str, error = %e, "oshash failed; indexing as new without fingerprint");
            String::new()
        }
    };

    let scene_id = if !oshash.is_empty() {
        match find_scene_by_oshash(&mut *tx, &oshash).await? {
            Some(id) => id,
            None => create_scene_for(&mut *tx, &path_str, item.probe.title.as_deref()).await?,
        }
    } else {
        create_scene_for(&mut *tx, &path_str, item.probe.title.as_deref()).await?
    };

    insert_new_file(&mut *tx, &scene_id, &path_str, item, &mtime_str, &oshash).await?;
    Ok(WriteOutcome::Added)
}

async fn update_changed_file(
    tx: &mut SqliteConnection,
    file_id: &str,
    item: &IndexedFile,
    mtime_str: &str,
    oshash: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE files SET
            size_bytes = ?,
            modified_at = ?,
            format_name = ?,
            duration = ?,
            width = ?,
            height = ?,
            codec = ?,
            fps = ?,
            bitrate = ?,
            scanned_at = ?
        WHERE id = ?
        "#,
    )
    .bind(item.found.size as i64)
    .bind(mtime_str)
    .bind(&item.probe.format_name)
    .bind(item.probe.duration)
    .bind(item.probe.width)
    .bind(item.probe.height)
    .bind(&item.probe.codec)
    .bind(item.probe.fps)
    .bind(item.probe.bitrate)
    .bind(now().to_rfc3339())
    .bind(file_id)
    .execute(&mut *tx)
    .await?;

    // Refresh oshash. Only invalidate MD5/pHash when content identity changed —
    // mtime/size probes often flip without the video bytes changing, and wiping
    // pHash forced a full expensive recompute on every scan.
    if !oshash.is_empty() {
        let prev = fp_get(&mut *tx, file_id, "oshash").await.ok().flatten();
        fp_upsert(&mut *tx, file_id, "oshash", oshash).await?;
        if prev.as_deref() != Some(oshash) {
            fp_delete_type(&mut *tx, file_id, "md5").await?;
            fp_delete_type(&mut *tx, file_id, "phash").await?;
        }
    } else {
        // No oshash available — be conservative and drop content hashes.
        fp_delete_type(&mut *tx, file_id, "md5").await?;
        fp_delete_type(&mut *tx, file_id, "phash").await?;
    }
    Ok(())
}

async fn find_scene_by_oshash(tx: &mut SqliteConnection, oshash: &str) -> Result<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT f.scene_id
         FROM fingerprints fp JOIN files f ON f.id = fp.file_id
         WHERE fp.hash_type = 'oshash' AND fp.value = ? LIMIT 1",
    )
    .bind(oshash)
    .fetch_optional(&mut *tx)
    .await?;
    Ok(row.map(|(id,)| id))
}

async fn create_scene_for(
    tx: &mut SqliteConnection,
    path: &str,
    embedded_title: Option<&str>,
) -> Result<String> {
    let id = new_id();
    let ts = now().to_rfc3339();
    let (title, title_source) = if let Some(t) =
        embedded_title.map(str::trim).filter(|s| !s.is_empty())
    {
        (Some(t.to_string()), "embedded")
    } else {
        let from_path = crate::filename_parse::scene_title_from_path(std::path::Path::new(path));
        (from_path, "filename")
    };
    sqlx::query(
        "INSERT INTO scenes (id, title, title_source, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&title)
    .bind(title_source)
    .bind(&ts)
    .bind(&ts)
    .execute(&mut *tx)
    .await?;
    Ok(id)
}

async fn insert_new_file(
    tx: &mut SqliteConnection,
    scene_id: &str,
    path_str: &str,
    item: &IndexedFile,
    mtime_str: &str,
    oshash: &str,
) -> Result<()> {
    let file_id = new_id();
    sqlx::query(
        r#"
        INSERT INTO files (
            id, scene_id, path, size_bytes, modified_at,
            format_name, duration, width, height, codec, fps, bitrate,
            thumb_sprite_path, vtt_path, scanned_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, NULL, ?)
        "#,
    )
    .bind(&file_id)
    .bind(scene_id)
    .bind(path_str)
    .bind(item.found.size as i64)
    .bind(mtime_str)
    .bind(&item.probe.format_name)
    .bind(item.probe.duration)
    .bind(item.probe.width)
    .bind(item.probe.height)
    .bind(&item.probe.codec)
    .bind(item.probe.fps)
    .bind(item.probe.bitrate)
    .bind(now().to_rfc3339())
    .execute(&mut *tx)
    .await?;

    if !oshash.is_empty() {
        fp_upsert(&mut *tx, &file_id, "oshash", oshash).await?;
    }
    Ok(())
}

/// Fingerprint reads/writes on the batch transaction's connection.
///
/// `crate::fingerprints::*` takes `&SqlitePool`, which can't join the batch
/// transaction — a pool write would block on the tx's write lock until the
/// busy timeout. Keep these statements in sync with `fingerprints.rs`.
async fn fp_upsert(
    tx: &mut SqliteConnection,
    file_id: &str,
    hash_type: &str,
    value: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO fingerprints (id, file_id, hash_type, value, created_at)
        VALUES (?, ?, ?, ?, ?)
        ON CONFLICT(file_id, hash_type) DO UPDATE SET value = excluded.value
        "#,
    )
    .bind(new_id())
    .bind(file_id)
    .bind(hash_type)
    .bind(value)
    .bind(now().to_rfc3339())
    .execute(&mut *tx)
    .await?;
    Ok(())
}

async fn fp_delete_type(tx: &mut SqliteConnection, file_id: &str, hash_type: &str) -> Result<()> {
    sqlx::query("DELETE FROM fingerprints WHERE file_id = ? AND hash_type = ?")
        .bind(file_id)
        .bind(hash_type)
        .execute(&mut *tx)
        .await?;
    Ok(())
}

async fn fp_get(
    tx: &mut SqliteConnection,
    file_id: &str,
    hash_type: &str,
) -> Result<Option<String>> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM fingerprints WHERE file_id = ? AND hash_type = ?")
            .bind(file_id)
            .bind(hash_type)
            .fetch_optional(&mut *tx)
            .await?;
    Ok(row.map(|(v,)| v))
}

/// Delete DB file rows whose path is not among `live_paths`, except paths under
/// `protect_prefixes` (offline scan roots). Then prune scenes with no files.
async fn mark_removed_paths(
    pool: &SqlitePool,
    live_paths: &[String],
    protect_prefixes: &[String],
) -> Result<i64> {
    use std::collections::HashSet;
    let live: HashSet<&String> = live_paths.iter().collect();

    let all: Vec<(String, String)> = sqlx::query_as("SELECT id, path FROM files")
        .fetch_all(pool)
        .await?;

    let mut removed = 0i64;
    for (file_id, path) in all {
        if live.contains(&path) {
            continue;
        }
        if protect_prefixes
            .iter()
            .any(|root| crate::catalog_cleanup::path_is_under_root(&path, root))
        {
            continue;
        }
        sqlx::query("DELETE FROM files WHERE id = ?")
            .bind(&file_id)
            .execute(pool)
            .await?;
        removed += 1;
    }

    let _orphans = crate::catalog_cleanup::prune_orphan_scenes(pool).await?;
    Ok(removed)
}

fn system_time_to_rfc3339(t: std::time::SystemTime) -> String {
    let dt: chrono::DateTime<chrono::Utc> = t.into();
    dt.to_rfc3339()
}
