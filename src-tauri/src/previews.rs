//! Preview generation: thumbnail sprite + WebVTT for hover/scrub previews.
//!
//! For each video file we produce two artifacts in the previews dir:
//!   * `<file_id>.jpg`  — a contact sheet of N evenly-spaced frames, tiled
//!     THUMB_COLS wide. Each cell is THUMB_W × THUMB_H.
//!   * `<file_id>.vtt`  — WebVTT cues mapping time ranges → sprite cells via
//!     the standard `#xywh=left,top,width,height` fragment on the image URL.
//!
//! Fast path (generate_fast): ONE ffmpeg process — N `-ss` inputs (same
//! offsets and first-frame-after-seek picks as the legacy loop), concat +
//! scale/pad + tile in the filter graph, plus the 480px grid thumb from the
//! middle input. The legacy per-frame loop is kept as the fallback for files
//! where a fast-path input won't decode (e.g. a seek past EOF).

use std::{
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result};

use crate::media_tools;

const THUMB_W: u32 = 160;
const THUMB_H: u32 = 90;
const THUMB_COLS: u32 = 5;
/// One thumbnail per this many seconds of video (approx), capped to THUMB_MAX.
const SECS_PER_THUMB: f64 = 30.0;
const THUMB_MIN: u32 = 1;
const THUMB_MAX: u32 = 36;

/// Where preview artifacts live: %LOCALAPPDATA%/MaizeView/previews/
pub fn previews_dir() -> Result<PathBuf> {
    let base = dirs::cache_dir()
        .or_else(dirs::data_local_dir)
        .context("could not resolve local data dir")?;
    let dir = base.join("MaizeView").join("previews");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Resolve the ffmpeg binary via shared media_tools (exe dir → PATH → winget).
fn ffmpeg_binary() -> Result<PathBuf> {
    media_tools::ffmpeg_binary()
}

/// How many thumbnails to take for a video of `duration` seconds.
fn thumb_count(duration: Option<f64>) -> u32 {
    let d = duration.unwrap_or(0.0).max(0.0);
    let n = (d / SECS_PER_THUMB).floor() as u32;
    n.clamp(THUMB_MIN, THUMB_MAX)
}

/// (file_id, source path, duration) → produce sprite + vtt; return their paths.
///
/// Returns Ok(None) when the file can't be previewed (e.g. duration unknown or
/// ffmpeg fails) so the caller can skip without aborting the whole batch.
pub fn generate(
    file_id: &str,
    src: &Path,
    duration: Option<f64>,
) -> Result<Option<(PathBuf, PathBuf, PathBuf)>> {
    // Returns (thumb_path, sprite_path, vtt_path) on success.
    let n = thumb_count(duration);
    if n == 0 {
        return Ok(None);
    }
    let dur = match duration {
        Some(d) if d > 0.0 => d,
        _ => return Ok(None),
    };
    match generate_fast(file_id, src, dur, n) {
        Ok(r) => Ok(Some(r)),
        Err(e) => {
            tracing::debug!(
                error = %e,
                file = %src.display(),
                "fast preview failed, using legacy per-frame loop"
            );
            generate_legacy(file_id, src, dur, n).map(Some)
        }
    }
}

#[doc(hidden)] // parity test (tests/hash_parity.rs) — force the legacy path
pub fn generate_for_test(
    file_id: &str,
    src: &Path,
    duration: Option<f64>,
) -> Result<Option<(PathBuf, PathBuf, PathBuf)>> {
    let n = thumb_count(duration);
    let dur = match duration {
        Some(d) if d > 0.0 => d,
        _ => return Ok(None),
    };
    generate_legacy(file_id, src, dur, n).map(Some)
}

#[doc(hidden)] // parity test (tests/hash_parity.rs) — force the fast path
pub fn generate_fast_for_test(
    file_id: &str,
    src: &Path,
    duration: Option<f64>,
) -> Result<Option<(PathBuf, PathBuf, PathBuf)>> {
    let n = thumb_count(duration);
    let dur = match duration {
        Some(d) if d > 0.0 => d,
        _ => return Ok(None),
    };
    generate_fast(file_id, src, dur, n).map(Some)
}

/// Single-process preview: N input seeks, concat + tile in the filter graph,
/// and the 480px grid thumb from the middle input — one ffmpeg spawn total.
fn generate_fast(
    file_id: &str,
    src: &Path,
    dur: f64,
    n: u32,
) -> Result<(PathBuf, PathBuf, PathBuf)> {
    let bin = ffmpeg_binary().context("locating ffmpeg")?;
    let dir = previews_dir()?;
    let sprite_path = dir.join(format!("{file_id}.jpg"));
    let vtt_path = dir.join(format!("{file_id}.vtt"));
    let thumb_path = dir.join(format!("{file_id}.thumb.jpg"));

    // Same offsets as the legacy loop: (i + 0.5) / N * duration.
    let offsets: Vec<f64> = (0..n)
        .map(|i| ((i as f64 + 0.5) / n as f64) * dur)
        .collect();
    let mid = offsets.len() / 2;
    let rows = (n + THUMB_COLS - 1) / THUMB_COLS;

    let mut cmd = Command::new(&bin);
    cmd.arg("-y");
    for &t in &offsets {
        cmd.args(["-ss", &format!("{t:.3}"), "-t", "0.5", "-i"])
            .arg(src);
    }
    let mut parts: Vec<String> = (0..n as usize)
        .map(|i| format!("[{i}:v]select=eq(n\\,0)[f{i}]"))
        .collect();
    let concat_inputs: String = (0..n as usize).map(|i| format!("[f{i}]")).collect();
    parts.push(format!(
        "{concat_inputs}concat=n={n},scale={THUMB_W}:{THUMB_H}:force_original_aspect_ratio=decrease,pad={THUMB_W}:{THUMB_H}:(ow-iw)/2:(oh-ih)/2:black,tile={THUMB_COLS}x{rows}[sprite]"
    ));
    parts.push(format!(
        "[f{mid}]scale=480:270:force_original_aspect_ratio=decrease,pad=480:270:(ow-iw)/2:(oh-ih)/2:black[thumb]"
    ));
    cmd.args(["-filter_complex", &parts.join(";")]);
    cmd.args([
        "-map",
        "[sprite]",
        "-frames:v",
        "1",
        "-q:v",
        "4",
        "-update",
        "1",
    ])
    .arg(&sprite_path);
    cmd.args([
        "-map",
        "[thumb]",
        "-frames:v",
        "1",
        "-q:v",
        "3",
        "-update",
        "1",
    ])
    .arg(&thumb_path);

    let status = media_tools::silent_status(&mut cmd).context("running fast preview ffmpeg")?;
    if !status.success() {
        anyhow::bail!("fast preview ffmpeg failed (status {status})");
    }

    write_vtt(&vtt_path, &offsets, dur, n)?;
    Ok((thumb_path, sprite_path, vtt_path))
}

/// Legacy path: one ffmpeg spawn per frame (+ tile + thumb passes). Kept as
/// the fallback for files where a fast-path input won't decode.
fn generate_legacy(
    file_id: &str,
    src: &Path,
    dur: f64,
    n: u32,
) -> Result<(PathBuf, PathBuf, PathBuf)> {
    let bin = ffmpeg_binary().context("locating ffmpeg")?;
    let dir = previews_dir()?;
    let sprite_path = dir.join(format!("{file_id}.jpg"));
    let vtt_path = dir.join(format!("{file_id}.vtt"));
    // Single representative thumbnail for grid cards (separate from the
    // multi-cell sprite used by the scrubber).
    let thumb_path = dir.join(format!("{file_id}.thumb.jpg"));

    // Temp working dir for the individual frames.
    let tmp = tempfile::tempdir().context("creating temp dir for frames")?;
    let frame_pattern = tmp.path().join("frame_%03d.jpg");

    // Pass 1: extract N frames at evenly spaced offsets.
    // We pick offsets at (i + 0.5) / N * duration so we never sample exactly
    // at the start (black) or end, and each frame represents its cell's range.
    let offsets: Vec<f64> = (0..n)
        .map(|i| ((i as f64 + 0.5) / n as f64) * dur)
        .collect();

    for (i, &t) in offsets.iter().enumerate() {
        // Use -ss before -i for speed (keyframe seek), then accurate decode.
        let out = tmp.path().join(format!("frame_{:03}.jpg", i + 1));
        let status = media_tools::silent_output(
            Command::new(&bin)
                .args(["-y", "-ss", &format!("{:.3}", t), "-i"])
                .arg(src)
                .args([
                    "-frames:v",
                    "1",
                    "-vf",
                    &format!("scale={THUMB_W}:{THUMB_H}:force_original_aspect_ratio=decrease,pad={THUMB_W}:{THUMB_H}:(ow-iw)/2:(oh-ih)/2:black"),
                    "-q:v",
                    "4",
                    "-update",
                    "1",
                ])
                .arg(&out),
        );
        match status {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                tracing::warn!(
                    file = %src.display(),
                    t,
                    stderr = String::from_utf8_lossy(&o.stderr).trim(),
                    "frame extraction failed for one thumb"
                );
                // Write a black placeholder so the tile step still gets a cell.
                write_black_jpeg(&out)?;
            }
            Err(e) => {
                tracing::warn!(error = %e, "ffmpeg invocation failed for one thumb");
                write_black_jpeg(&out)?;
            }
        }
    }

    // Pass 2: tile the frames into a single sprite. tile=ColsxRows infers rows.
    let rows = (n + THUMB_COLS - 1) / THUMB_COLS;
    let tile_filter = format!("tile={THUMB_COLS}x{rows}");
    let status = media_tools::silent_status(
        Command::new(&bin)
            .args(["-y", "-framerate", "1", "-i"])
            .arg(frame_pattern.to_str().expect("utf8 temp path"))
            .args([
                "-frames:v",
                "1",
                "-vf",
                &tile_filter,
                "-q:v",
                "4",
                "-update",
                "1",
            ])
            .arg(&sprite_path),
    )
    .context("running ffmpeg tile pass")?;
    if !status.success() {
        anyhow::bail!("ffmpeg tile pass failed (status {status})");
    }

    // Pass 3: write the VTT.
    write_vtt(&vtt_path, &offsets, dur, n)?;

    // Pass 4: single representative thumbnail for grid cards — extract the
    // middle frame at a larger size (480px wide, 16:9) so cards aren't a noisy
    // contact sheet. Falls back to the first extracted frame on failure.
    let mid_offset = offsets.get(offsets.len() / 2).copied().unwrap_or(dur / 2.0);
    let thumb_status = media_tools::silent_status(
        Command::new(&bin)
            .args(["-y", "-ss", &format!("{mid_offset:.3}"), "-i"])
            .arg(src)
            .args([
                "-frames:v", "1",
                "-vf", "scale=480:270:force_original_aspect_ratio=decrease,pad=480:270:(ow-iw)/2:(oh-ih)/2:black",
                "-q:v", "3",
                "-update", "1",
            ])
            .arg(&thumb_path),
    );
    let thumb_ok = matches!(thumb_status, Ok(s) if s.success());
    if !thumb_ok {
        // Fallback: copy the middle cell out of the sprite as the thumbnail.
        let _ = extract_sprite_cell(&sprite_path, &thumb_path, offsets.len() / 2);
    }

    Ok((thumb_path, sprite_path, vtt_path))
}

/// Crop a single cell out of a contact-sheet sprite as a fallback thumbnail.
/// Best-effort — silently no-ops on failure (caller falls back to film icon).
fn extract_sprite_cell(sprite: &Path, out: &Path, cell_index: usize) -> Result<()> {
    let bin = ffmpeg_binary()?;
    let col = (cell_index as u32) % THUMB_COLS;
    let row = (cell_index as u32) / THUMB_COLS;
    let x = col * THUMB_W;
    let y = row * THUMB_H;
    let status = media_tools::silent_status(
        Command::new(&bin)
            .args(["-y", "-i"])
            .arg(sprite)
            .args([
                "-vf",
                &format!("crop={THUMB_W}:{THUMB_H}:{x}:{y},scale=480:-2"),
                "-frames:v",
                "1",
                "-q:v",
                "3",
                "-update",
                "1",
            ])
            .arg(out),
    )?;
    if !status.success() {
        anyhow::bail!("sprite cell extraction failed");
    }
    Ok(())
}

/// Write a 1×1 black JPEG so missing frames don't break the tile grid.
/// (ffmpeg accepts JPEG input for the tile filter; a minimal valid black JPEG
/// is a constant we embed.)
fn write_black_jpeg(out: &Path) -> Result<()> {
    // Minimal 2×2 black JPEG (Q8 baseline). Avoids pulling an image crate.
    static BLACK_JPEG: &[u8] = &[
        0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00, 0x00,
        0x01, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xDB, 0x00, 0x43, 0x00, 0x08, 0x08, 0x08, 0x08, 0x08,
        0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08,
        0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08,
        0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08,
        0x08, 0x08, 0x08, 0xFF, 0xC9, 0x00, 0x0B, 0x08, 0x00, 0x02, 0x00, 0x02, 0x01, 0x01, 0x11,
        0x00, 0xFF, 0xCC, 0x00, 0x06, 0x00, 0x10, 0x10, 0x05, 0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01,
        0x00, 0x00, 0x3F, 0x00, 0xFB, 0xFC, 0xFC, 0xFF, 0xD9,
    ];
    std::fs::write(out, BLACK_JPEG)?;
    Ok(())
}

/// Write a WebVTT mapping each cell's time range to its sprite rectangle.
/// Cue i covers [start_i, start_{i+1}) (last cue runs to duration).
fn write_vtt(vtt: &Path, offsets: &[f64], duration: f64, count: u32) -> Result<()> {
    use std::io::Write;
    let mut f = std::fs::File::create(vtt)?;
    writeln!(f, "WEBVTT")?;
    writeln!(f)?;
    for i in 0..count as usize {
        let start = offsets[i];
        let end = if i + 1 < offsets.len() {
            offsets[i + 1]
        } else {
            duration
        };
        let row = (i as u32) / THUMB_COLS;
        let col = (i as u32) % THUMB_COLS;
        let x = col * THUMB_W;
        let y = row * THUMB_H;
        writeln!(f, "{}", format_vtt_timestamp(start))?;
        writeln!(
            f,
            "{} --> {}",
            format_vtt_timestamp(start),
            format_vtt_timestamp(end)
        )?;
        writeln!(
            f,
            "{}#xywh={x},{y},{THUMB_W},{THUMB_H}",
            vtt.file_name()
                .unwrap()
                .to_string_lossy()
                .replace(".vtt", ".jpg")
        )?;
        writeln!(f)?;
    }
    Ok(())
}

fn format_vtt_timestamp(secs: f64) -> String {
    let total = secs.max(0.0);
    let h = (total / 3600.0) as u64;
    let m = ((total % 3600.0) / 60.0) as u64;
    let s = (total % 60.0) as u64;
    let ms = ((total - total.trunc()) * 1000.0) as u64;
    format!("{h:02}:{m:02}:{s:02}.{ms:03}")
}
