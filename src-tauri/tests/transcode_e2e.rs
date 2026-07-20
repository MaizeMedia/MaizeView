//! End-to-end transcode test: generates a real video with ffmpeg, inserts it
//! as a scene/file, runs the downscale pipeline, and asserts the file was
//! replaced and the DB reflects the new resolution.
//!
//! Requires ffmpeg/ffprobe resolvable (PATH then winget fallback) — same as
//! `scanner_e2e.rs`. Run with: `cargo test --test transcode_e2e -- --ignored`.

use std::path::PathBuf;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;

use maizeview_lib::transcode_job::{self, DownscaleOptions, OriginalMode, TagMode};
use maizeview_lib::transcode_tokens::RewriteMode;

async fn fresh_db() -> SqlitePool {
    let dir = tempfile::tempdir().unwrap();
    let path: PathBuf = dir.path().join("test.db");
    std::mem::forget(dir); // keep alive for the test

    let options = SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}

/// Resolve ffmpeg (PATH then winget fallback). Mirrors media_tools resolution.
fn ffmpeg_bin() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("PATH") {
        let exe = if cfg!(windows) {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        };
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(exe);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    if let Ok(base) = std::env::var("LOCALAPPDATA") {
        let pkg = std::path::Path::new(&base)
            .join("Microsoft/WinGet/Packages")
            .join("Gyan.FFmpeg_Microsoft.Winget.Source_8wekyb3d8bbwe");
        if let Ok(entries) = std::fs::read_dir(&pkg) {
            for entry in entries.flatten() {
                let exe = entry.path().join("bin").join("ffmpeg.exe");
                if exe.exists() {
                    return Some(exe);
                }
            }
        }
    }
    None
}

/// Generate a short test video at the given resolution into `dir`.
fn make_test_video(dir: &std::path::Path, name: &str, w: u32, h: u32) -> PathBuf {
    let ffmpeg = ffmpeg_bin().expect("ffmpeg required for this test");
    let out = dir.join(name);
    let res = std::process::Command::new(&ffmpeg)
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            &format!("testsrc=duration=2:size={w}x{h}:rate=25"),
            "-pix_fmt",
            "yuv420p",
            "-c:v",
            "libx264",
        ])
        .arg(&out)
        .output()
        .expect("ffmpeg generation failed");
    assert!(
        res.status.success(),
        "ffmpeg gen failed: {}",
        String::from_utf8_lossy(&res.stderr)
    );
    out
}

#[tokio::test]
#[ignore = "requires ffmpeg/ffprobe; run with --ignored"]
async fn downscale_replaces_in_place_and_updates_db() {
    let pool = fresh_db().await;
    let tmp = tempfile::tempdir().unwrap();
    let video_dir = tmp.path().to_path_buf();

    // A 1080p-ish source whose filename carries a "1080p" token.
    let src = make_test_video(&video_dir, "Scene 1080p.mp4", 1920, 1080);

    // Seed a scene + file row pointing at the source.
    let scene_id = "01J00000000000000000000001".to_string();
    let file_id = "01J00000000000000000000002".to_string();
    sqlx::query("INSERT INTO scenes (id, created_at, updated_at) VALUES (?, ?, ?)")
        .bind(&scene_id)
        .bind("2026-07-15T00:00:00Z")
        .bind("2026-07-15T00:00:00Z")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO files (id, scene_id, path, size_bytes, modified_at, width, height, duration, scanned_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&file_id)
    .bind(&scene_id)
    .bind(src.to_string_lossy().to_string())
    .bind(1000_i64)
    .bind("2026-07-15T00:00:00Z")
    .bind(1920_i64)
    .bind(1080_i64)
    .bind(2.0_f64)
    .bind("2026-07-15T00:00:00Z")
    .execute(&pool)
    .await
    .unwrap();

    // Preview should report one would-transcode, zero skipped, 1080p bucket.
    let preview = transcode_job::build_preview(&pool, &[scene_id.clone()], 720)
        .await
        .unwrap();
    assert_eq!(preview.total, 1);
    assert_eq!(preview.would_transcode, 1);
    assert_eq!(preview.skipped, 0);
    assert_eq!(
        preview.items[0].preview_filename.as_deref(),
        Some("Scene 720p.mp4")
    );

    // Run the transcode (replace mode, rewrite filename token).
    let opts = DownscaleOptions {
        scene_ids: vec![scene_id.clone()],
        target_height: 720,
        original_mode: OriginalMode::Replace,
        filename_mode: RewriteMode::Replace,
        tag_mode: TagMode::Leave,
    };
    let cancel = std::sync::Arc::new(CancellationToken::new());
    let failed = transcode_job::run_silent(&pool, opts, cancel)
        .await
        .expect("transcode job should not error");
    assert!(failed.is_empty(), "transcode had failures: {failed:?}");

    // Original source must be gone (replaced).
    assert!(!src.exists(), "original file should have been removed");

    // The new file exists with the rewritten name.
    let new_path = video_dir.join("Scene 720p.mp4");
    assert!(
        new_path.exists(),
        "transcoded file should exist at new name"
    );

    // DB row updated in place (same file_id) with new height.
    let row: (Option<i64>, String) = sqlx::query_as("SELECT height, path FROM files WHERE id = ?")
        .bind(&file_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.0, Some(720));
    assert!(row.1.ends_with("Scene 720p.mp4"));
}

#[tokio::test]
#[ignore = "requires ffmpeg/ffprobe; run with --ignored"]
async fn downscale_skips_already_small() {
    let pool = fresh_db().await;
    let tmp = tempfile::tempdir().unwrap();
    // Source at 480, target 720 → already smaller, must skip.
    let src = make_test_video(tmp.path(), "small.mp4", 640, 480);

    let scene_id = "01J00000000000000000000003".to_string();
    let file_id = "01J00000000000000000000004".to_string();
    sqlx::query("INSERT INTO scenes (id, created_at, updated_at) VALUES (?, ?, ?)")
        .bind(&scene_id)
        .bind("2026-07-15T00:00:00Z")
        .bind("2026-07-15T00:00:00Z")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO files (id, scene_id, path, size_bytes, modified_at, height, duration, scanned_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&file_id)
    .bind(&scene_id)
    .bind(src.to_string_lossy().to_string())
    .bind(1000_i64)
    .bind("2026-07-15T00:00:00Z")
    .bind(480_i64)
    .bind(2.0_f64)
    .bind("2026-07-15T00:00:00Z")
    .execute(&pool)
    .await
    .unwrap();

    let preview = transcode_job::build_preview(&pool, &[scene_id.clone()], 720)
        .await
        .unwrap();
    assert_eq!(preview.would_transcode, 0);
    assert_eq!(preview.skipped, 1);
}
