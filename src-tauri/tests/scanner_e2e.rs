//! End-to-end scanner test: runs the real scan pipeline against a temp
//! directory of generated videos and asserts the DB ended up populated.
//!
//! Requires ffmpeg/ffprobe on PATH (CI/dev machine requirement).

use std::path::PathBuf;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

async fn fresh_db() -> SqlitePool {
    let dir = tempfile::tempdir().unwrap();
    let path: PathBuf = dir.path().join("test.db");
    // Keep dir alive for the test by leaking it (test process is short-lived).
    std::mem::forget(dir);

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

/// Locate the test video dir. Set MAIZEVIEW_TEST_LIB to point at it; otherwise
/// default to ~/maizeview-test-lib (the dir created during Phase 1 setup).
fn ensure_test_lib() -> PathBuf {
    if let Ok(p) = std::env::var("MAIZEVIEW_TEST_LIB") {
        let pb = PathBuf::from(p);
        assert!(
            pb.is_dir(),
            "MAIZEVIEW_TEST_LIB is not a dir: {}",
            pb.display()
        );
        return pb;
    }
    let home = dirs::home_dir().expect("no home dir");
    let candidate = home.join("maizeview-test-lib");
    assert!(
        candidate.is_dir(),
        "test lib not found at {}; create it or set MAIZEVIEW_TEST_LIB",
        candidate.display()
    );
    candidate
}

/// Resolve ffmpeg for test inspection (PATH then winget fallback).
fn ffmpeg_for_tests() -> String {
    if let Some(path) = std::env::var_os("PATH") {
        let exe = if cfg!(windows) {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        };
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(exe);
            if candidate.is_file() {
                return candidate.to_string_lossy().into_owned();
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
                    return exe.to_string_lossy().into_owned();
                }
            }
        }
    }
    "ffmpeg".to_string()
}

#[tokio::test]
#[ignore = "requires the dev test-lib + ffprobe on PATH; run with --ignored"]
async fn scan_indexes_test_library() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("debug")
        .try_init();

    let pool = fresh_db().await;
    let lib = ensure_test_lib();

    // Insert the scan path the way the UI would.
    let id = maizeview_lib::models::new_id();
    let created = maizeview_lib::models::now().to_rfc3339();
    sqlx::query("INSERT INTO scan_paths (id, path, label, created_at) VALUES (?, ?, ?, ?)")
        .bind(&id)
        .bind(lib.to_string_lossy().to_string())
        .bind::<Option<&str>>(None)
        .bind(&created)
        .execute(&pool)
        .await
        .unwrap();

    // Run the scan with a no-op progress sink.
    let sink: maizeview_lib::scanner::ProgressSink = std::sync::Arc::new(|_p| {});
    let scan_run_id = maizeview_lib::models::new_id();
    let result = maizeview_lib::scanner::scan(
        &pool,
        &scan_run_id,
        &[lib.to_string_lossy().to_string()],
        sink,
        tokio_util::sync::CancellationToken::new(),
    )
    .await
    .expect("scan should succeed");

    // Expect 3 videos (red/green/blue). readme.txt must be filtered out.
    assert_eq!(result.files_found, 3, "should find exactly 3 video files");
    assert!(result.files_added >= 3, "should have added files");
    assert_eq!(result.status, "completed");

    // DB should now contain 3 scenes + 3 files + 3 oshash fingerprints.
    let scenes: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM scenes")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(scenes.0, 3, "3 scenes expected");

    let files: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM files")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(files.0, 3, "3 file rows expected");

    let fps: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM fingerprints WHERE hash_type='oshash'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(fps.0, 3, "3 oshash fingerprints expected");

    // ffprobe data should have populated duration/dimensions for each file.
    let probed: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM files WHERE duration IS NOT NULL AND width IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(probed.0, 3, "all files should have ffprobe data");

    // Idempotency: re-scan → nothing added, nothing updated.
    let sink2: maizeview_lib::scanner::ProgressSink = std::sync::Arc::new(|_p| {});
    let r2 = maizeview_lib::scanner::scan(
        &pool,
        &maizeview_lib::models::new_id(),
        &[lib.to_string_lossy().to_string()],
        sink2,
        tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(r2.files_added, 0, "re-scan should add nothing");
    assert_eq!(r2.files_updated, 0, "re-scan should update nothing");

    // ─── Preview generation ───────────────────────────────────────────────
    // Pick the longest video (green, 5s) so we get multiple thumbs.
    let (file_id, path, duration): (String, String, Option<f64>) = sqlx::query_as(
        "SELECT id, path, duration FROM files ORDER BY duration DESC NULLS LAST LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        duration.unwrap_or(0.0) > 0.0,
        "need a real duration for preview test"
    );

    let result = maizeview_lib::previews::generate(&file_id, std::path::Path::new(&path), duration)
        .expect("preview generation should succeed")
        .expect("should have produced artifacts");

    let (thumb, sprite, vtt) = result;
    assert!(
        thumb.exists(),
        "thumb file should exist: {}",
        thumb.display()
    );
    assert!(
        sprite.exists(),
        "sprite file should exist: {}",
        sprite.display()
    );
    assert!(vtt.exists(), "vtt file should exist: {}", vtt.display());

    // Thumb should be a valid JPEG too.
    let thumb_bytes = std::fs::read(&thumb).unwrap();
    assert_eq!(
        &thumb_bytes[0..2],
        &[0xFF, 0xD8],
        "thumb should start with JPEG SOI marker"
    );

    // Sprite should be a valid JPEG. (Don't assert a min size: a uniform-color
    // test clip compresses to a tiny JPEG — that's correct, not a failure.
    // Instead, decode one pixel and confirm it's not the black placeholder.)
    let sprite_bytes = std::fs::read(&sprite).unwrap();
    assert_eq!(
        &sprite_bytes[0..2],
        &[0xFF, 0xD8],
        "sprite should start with JPEG SOI marker"
    );
    assert_eq!(
        &sprite_bytes[sprite_bytes.len() - 2..],
        &[0xFF, 0xD9],
        "sprite should end with JPEG EOI marker"
    );

    // Decode the sprite's first pixel via ffmpeg and check it isn't pure black
    // (which would mean all frames fell back to the black placeholder).
    let pixel = std::process::Command::new(ffmpeg_for_tests())
        .args(["-i"])
        .arg(&sprite)
        .args([
            "-vf",
            "scale=1:1",
            "-pix_fmt",
            "rgb24",
            "-f",
            "rawvideo",
            "-",
        ])
        .output();
    if let Ok(out) = pixel {
        let px = &out.stdout;
        if px.len() >= 3 {
            let is_black = px[0] < 8 && px[1] < 8 && px[2] < 8;
            assert!(
                !is_black,
                "sprite should not be all-black (frame extraction failed); first pixel = {:?}",
                &px[..3]
            );
        }
    }

    // VTT should be well-formed: WEBVTT header + at least one cue.
    let vtt_text = std::fs::read_to_string(&vtt).unwrap();
    assert!(
        vtt_text.starts_with("WEBVTT"),
        "vtt should start with WEBVTT"
    );
    assert!(vtt_text.contains("-->"), "vtt should contain cue timings");
    assert!(
        vtt_text.contains("#xywh="),
        "vtt cues should reference sprite cells"
    );
}

/// Cancel-keeps-partials: cancelling a scan after at least one batch has
/// written must keep those rows on disk, and a follow-up scan completes the
/// library (idempotent resume). We build a 100-file test lib by copying the
/// small source videos so we exceed the 32-file batch size.
#[tokio::test]
#[ignore = "requires the dev test-lib + ffprobe on PATH; run with --ignored"]
async fn scan_cancel_keeps_partials() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info,maizeview_lib=debug")
        .try_init();

    // Build a 100-file temp library by copying one of the source videos.
    let src = ensure_test_lib().join("red-scene.mp4");
    assert!(src.exists(), "red-scene.mp4 missing from test lib");
    let big = tempfile::tempdir().unwrap();
    for i in 0..100 {
        std::fs::copy(&src, big.path().join(format!("clip_{i:03}.mp4"))).unwrap();
    }
    let big_path = big.path().to_string_lossy().to_string();

    let pool = fresh_db().await;
    let cancel = tokio_util::sync::CancellationToken::new();

    // Spawn the scan and cancel it almost immediately. With 100 files in
    // batches of 32, at least the first batch (~32 files) should write before
    // the cancel is observed at the next batch boundary.
    let sink: maizeview_lib::scanner::ProgressSink = std::sync::Arc::new(|_p| {});
    let pool_clone = pool.clone();
    let sink_clone = std::sync::Arc::clone(&sink);
    let cancel_clone = cancel.clone();
    let big_path_clone = big_path.clone();
    let handle = tokio::spawn(async move {
        maizeview_lib::scanner::scan(
            &pool_clone,
            &maizeview_lib::models::new_id(),
            &[big_path_clone],
            sink_clone,
            cancel_clone,
        )
        .await
    });

    // Give the scan a moment to start writing, then cancel.
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    cancel.cancel();
    let result = handle.await.unwrap();
    assert!(
        matches!(result, Err(maizeview_lib::scanner::ScanCancelled)),
        "expected scan to be cancelled, got {result:?}"
    );

    // Partial rows must be on disk. We can't assert an exact count (timing-
    // dependent), but it must be > 0 and < 100.
    let kept: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM files")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        kept.0 > 0,
        "cancel should have kept some partial rows, got 0"
    );
    assert!(kept.0 < 100, "cancel should not have written all 100 files");

    // Resume: a fresh scan (no cancel) should complete the library. The 100
    // source files all have identical content → identical oshash → they'll
    // be deduped to ONE scene with many files. That's fine; we just care that
    // the file count reaches 100 and nothing was lost.
    let sink2: maizeview_lib::scanner::ProgressSink = std::sync::Arc::new(|_p| {});
    let r2 = maizeview_lib::scanner::scan(
        &pool,
        &maizeview_lib::models::new_id(),
        &[big_path],
        sink2,
        tokio_util::sync::CancellationToken::new(),
    )
    .await
    .expect("resume scan should succeed");
    assert_eq!(r2.status, "completed");
    assert_eq!(r2.files_found, 100);

    let after: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM files")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(after.0, 100, "resumed scan should reach 100 files");
}
