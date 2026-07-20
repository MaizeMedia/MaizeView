//! Parity tests: the single-process (fast) sprite paths must produce the
//! SAME outputs as the legacy per-frame-spawn loops — identical timestamps,
//! identical first-frame-after-seek picks, identical hashes/sprites.
//!
//! Needs a video folder (MAIZEVIEW_TEST_LIB, else ~/maizeview-test-lib)
//! and ffmpeg/ffprobe — run with:
//!   cargo test --test hash_parity -- --ignored

use std::path::PathBuf;

use maizeview_lib::scanner::probe;

#[allow(unused_imports)]
use image::GenericImageView as _;

fn test_lib() -> PathBuf {
    if let Ok(p) = std::env::var("MAIZEVIEW_TEST_LIB") {
        let pb = PathBuf::from(p);
        assert!(
            pb.is_dir(),
            "MAIZEVIEW_TEST_LIB is not a dir: {}",
            pb.display()
        );
        return pb;
    }
    dirs::home_dir()
        .expect("no home dir")
        .join("maizeview-test-lib")
}

/// Up to 3 video files from the test lib (recursive, sorted for determinism).
fn lib_files() -> Vec<PathBuf> {
    const EXTS: [&str; 6] = ["mp4", "mkv", "mov", "avi", "wmv", "m4v"];
    let mut out = Vec::new();
    let mut stack = vec![test_lib()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| EXTS.contains(&e.to_ascii_lowercase().as_str()))
                .unwrap_or(false)
            {
                out.push(path);
            }
        }
    }
    out.sort();
    out.truncate(3);
    out
}

/// (path, probed duration) pairs; skips files ffprobe can't read.
fn probed_files() -> Vec<(PathBuf, f64)> {
    lib_files()
        .into_iter()
        .filter_map(|p| match probe::probe(&p) {
            Ok(s) => s.duration.filter(|d| *d > 0.5).map(|d| (p, d)),
            Err(_) => None,
        })
        .collect()
}

#[test]
#[ignore]
fn phash_fast_matches_legacy() {
    let files = probed_files();
    if files.is_empty() {
        eprintln!("no readable videos in test lib, skipping");
        return;
    }
    for (path, dur) in files {
        let legacy = maizeview_lib::scanner::phash::generate_sprite(&path, dur)
            .unwrap_or_else(|e| panic!("legacy sprite failed for {}: {e}", path.display()));
        let fast = maizeview_lib::scanner::phash::generate_sprite_fast(&path, dur)
            .unwrap_or_else(|e| panic!("fast sprite failed for {}: {e}", path.display()));
        let legacy_hash = maizeview_lib::scanner::phash::perception_hash(&legacy).unwrap();
        let fast_hash = maizeview_lib::scanner::phash::perception_hash(&fast).unwrap();
        assert_eq!(
            legacy_hash,
            fast_hash,
            "phash differs for {} (legacy {legacy_hash:016x} != fast {fast_hash:016x})",
            path.display()
        );
        eprintln!("phash parity OK: {} ({fast_hash:016x})", path.display());
    }
}

#[test]
#[ignore]
fn timing_comparison() {
    let files = probed_files();
    if files.is_empty() {
        eprintln!("no readable videos in test lib, skipping");
        return;
    }
    for (path, dur) in files.iter().take(2) {
        let t0 = std::time::Instant::now();
        let _ = maizeview_lib::scanner::phash::generate_sprite(path, *dur).unwrap();
        let phash_legacy = t0.elapsed();
        let t1 = std::time::Instant::now();
        let _ = maizeview_lib::scanner::phash::generate_sprite_fast(path, *dur).unwrap();
        let phash_fast = t1.elapsed();

        let t2 = std::time::Instant::now();
        let _ = maizeview_lib::previews::generate_for_test("bench-legacy", path, Some(*dur))
            .unwrap()
            .unwrap();
        let prev_legacy = t2.elapsed();
        let t3 = std::time::Instant::now();
        let _ = maizeview_lib::previews::generate_fast_for_test("bench-fast", path, Some(*dur))
            .unwrap()
            .unwrap();
        let prev_fast = t3.elapsed();

        eprintln!(
            "{}\n  phash: legacy {:>6.1?}  fast {:>6.1?}  ({:.1}x)\n  preview: legacy {:>6.1?}  fast {:>6.1?}  ({:.1}x)",
            path.display(),
            phash_legacy,
            phash_fast,
            phash_legacy.as_secs_f64() / phash_fast.as_secs_f64().max(1e-9),
            prev_legacy,
            prev_fast,
            prev_legacy.as_secs_f64() / prev_fast.as_secs_f64().max(1e-9),
        );
    }
}

#[test]
#[ignore]
fn previews_fast_matches_legacy() {
    let files = probed_files();
    if files.is_empty() {
        eprintln!("no readable videos in test lib, skipping");
        return;
    }
    for (path, dur) in files {
        let legacy = maizeview_lib::previews::generate_for_test("parity-legacy", &path, Some(dur))
            .unwrap_or_else(|e| panic!("legacy preview failed for {}: {e}", path.display()))
            .expect("legacy preview None");
        let fast = maizeview_lib::previews::generate_fast_for_test("parity-fast", &path, Some(dur))
            .unwrap_or_else(|e| panic!("fast preview failed for {}: {e}", path.display()))
            .expect("fast preview None");
        // Same artifacts: thumb, sprite, vtt — VTT must be identical apart
        // from the sprite filename baked into it (same offsets/cells).
        let normalize = |s: String| {
            s.replace("parity-legacy.jpg", "SPRITE")
                .replace("parity-fast.jpg", "SPRITE")
        };
        let vtt_legacy = normalize(std::fs::read_to_string(&legacy.2).unwrap());
        let vtt_fast = normalize(std::fs::read_to_string(&fast.2).unwrap());
        assert_eq!(vtt_legacy, vtt_fast, "vtt differs for {}", path.display());
        let img_legacy = image::open(&legacy.1).unwrap();
        let img_fast = image::open(&fast.1).unwrap();
        use image::GenericImageView;
        assert_eq!(
            img_legacy.dimensions(),
            img_fast.dimensions(),
            "sprite dims differ for {}",
            path.display()
        );
        eprintln!("preview parity OK: {}", path.display());
        for p in [&legacy.0, &legacy.1, &legacy.2, &fast.0, &fast.1, &fast.2] {
            let _ = std::fs::remove_file(p);
        }
    }
}
