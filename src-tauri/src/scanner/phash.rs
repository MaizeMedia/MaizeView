//! Stash-compatible video pHash (5×5 sprite collage → goimagehash PerceptionHash).
//!
//! Sprite layout matches Stash `pkg/hash/videophash`: 25 frames between 5%–95%
//! of duration, 160px-wide BMP screenshots, tiled 5×5.
//!
//! The image hash matches corona10/goimagehash `PerceptionHash` (what Stash
//! calls): resize 64×64 bilinear, grayscale, Lee DCT-II, top-left 8×8, median
//! threshold → uint64 hex. The previous 32×32 naive DCT was *not* byte-compatible
//! with StashDB.

use std::{path::Path, process::Command};

use anyhow::{Context, Result};
use image::{imageops, DynamicImage, GenericImageView, Rgba};

const SCREENSHOT_WIDTH: u32 = 160;
const COLS: u32 = 5;
const ROWS: u32 = 5;
const CHUNK_COUNT: usize = (COLS * ROWS) as usize;
/// goimagehash PerceptionHash input size (not the classic 32×32 shortcut).
const DCT_SIZE: usize = 64;
const HASH_SIZE: usize = 8;

/// Compute pHash hex string (lowercase uint64) for a video file.
///
/// Fast path: one ffmpeg process (25 input seeks + concat/tile inline —
/// identical timestamps and first-frame-after-seek semantics as the legacy
/// loop, so the hash is unchanged). Falls back to the legacy 25-spawn loop
/// when any input won't decode (e.g. a seek past EOF), so odd files still
/// hash the way they always did.
pub fn hash_file(path: &Path, duration: f64) -> Result<String> {
    if duration <= 0.0 {
        anyhow::bail!("duration required for phash");
    }
    let sprite = generate_sprite_fast(path, duration).or_else(|e| {
        tracing::debug!(
            error = %e,
            path = %path.display(),
            "fast phash sprite failed, using legacy per-frame loop"
        );
        generate_sprite(path, duration)
    })?;
    let value = perception_hash(&sprite)?;
    Ok(format!("{value:016x}"))
}

/// Parse a stored phash hex string to its `u64` value.
pub fn parse_hex(s: &str) -> Option<u64> {
    u64::from_str_radix(s.trim(), 16).ok()
}

/// Hamming distance between two stored phash hex strings.
pub fn hamming_hex(a: &str, b: &str) -> Option<u32> {
    Some((parse_hex(a)? ^ parse_hex(b)?).count_ones())
}

#[doc(hidden)] // exposed for the parity test (tests/hash_parity.rs)
pub fn generate_sprite(path: &Path, duration: f64) -> Result<DynamicImage> {
    let offset = 0.05 * duration;
    let step = (0.9 * duration) / CHUNK_COUNT as f64;

    let mut images = Vec::with_capacity(CHUNK_COUNT);
    for i in 0..CHUNK_COUNT {
        let t = offset + (i as f64 * step);
        images.push(screenshot_bmp(path, t)?);
    }

    combine_images(&images)
}

/// Single-process sprite: the same 25 seeks as separate ffmpeg INPUTS in ONE
/// command (same timestamps, same first-frame-after-seek semantics as the
/// legacy loop), concat + tile in the filter graph, one tiled BMP out the
/// pipe. One process spawn instead of 25.
#[doc(hidden)] // exposed for the parity test (tests/hash_parity.rs)
pub fn generate_sprite_fast(path: &Path, duration: f64) -> Result<DynamicImage> {
    let offset = 0.05 * duration;
    let step = (0.9 * duration) / CHUNK_COUNT as f64;

    let bin = crate::media_tools::ffmpeg_binary().context("locating ffmpeg for phash")?;
    let mut cmd = Command::new(&bin);
    cmd.arg("-y");
    for i in 0..CHUNK_COUNT {
        let t = offset + (i as f64 * step);
        cmd.args(["-ss", &format!("{t:.3}"), "-t", "0.5", "-i"])
            .arg(path);
    }
    // [k:v]select=eq(n\,0)[f_k] for each input, then concat → scale → tile.
    let mut parts: Vec<String> = (0..CHUNK_COUNT)
        .map(|i| format!("[{i}:v]select=eq(n\\,0)[f{i}]"))
        .collect();
    let concat_inputs: String = (0..CHUNK_COUNT).map(|i| format!("[f{i}]")).collect();
    parts.push(format!(
        "{concat_inputs}concat=n={CHUNK_COUNT}:v=1,scale={SCREENSHOT_WIDTH}:-1,tile={COLS}x{ROWS}"
    ));
    cmd.args(["-filter_complex", &parts.join(";")]);
    cmd.args([
        "-frames:v",
        "1",
        "-f",
        "image2pipe",
        "-vcodec",
        "bmp",
        "pipe:1",
    ]);

    let out = crate::media_tools::silent_output(&mut cmd)
        .with_context(|| format!("ffmpeg phash fast sprite for {}", path.display()))?;
    if !out.status.success() {
        anyhow::bail!(
            "ffmpeg phash fast sprite failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    image::load_from_memory(&out.stdout).context("decoding phash fast sprite BMP")
}

fn screenshot_bmp(path: &Path, t: f64) -> Result<DynamicImage> {
    let bin = crate::media_tools::ffmpeg_binary().context("locating ffmpeg for phash")?;
    let out = crate::media_tools::silent_output(
        Command::new(&bin)
            .args(["-y", "-ss", &format!("{t:.3}"), "-i"])
            .arg(path)
            .args([
                "-frames:v",
                "1",
                "-vf",
                &format!("scale={SCREENSHOT_WIDTH}:-1"),
                "-f",
                "image2pipe",
                "-vcodec",
                "bmp",
                "pipe:1",
            ]),
    )
    .with_context(|| format!("ffmpeg phash screenshot at {t}s for {}", path.display()))?;

    if !out.status.success() {
        anyhow::bail!(
            "ffmpeg phash screenshot failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }

    image::load_from_memory(&out.stdout).context("decoding phash screenshot BMP")
}

fn combine_images(images: &[DynamicImage]) -> Result<DynamicImage> {
    let first = images
        .first()
        .context("phash sprite requires at least one frame")?;
    let w = first.width();
    let h = first.height();
    let canvas_w = w * COLS;
    let canvas_h = h * ROWS;
    let mut canvas = image::RgbaImage::from_pixel(canvas_w, canvas_h, Rgba([0, 0, 0, 255]));

    for (index, img) in images.iter().enumerate() {
        let x = w * (index as u32 % COLS);
        let y = h * (index as u32 / ROWS);
        imageops::overlay(&mut canvas, img, x.into(), y.into());
    }

    Ok(DynamicImage::ImageRgba8(canvas))
}

/// goimagehash `PerceptionHash` — Stash / StashDB byte-compatible.
#[doc(hidden)] // exposed for the parity test (tests/hash_parity.rs)
pub fn perception_hash(img: &DynamicImage) -> Result<u64> {
    // nfnt/resize Bilinear ≈ Triangle in the `image` crate for our purposes.
    let resized = img.resize_exact(
        DCT_SIZE as u32,
        DCT_SIZE as u32,
        image::imageops::FilterType::Triangle,
    );

    // Match goimagehash Rgb2Gray / pixel2Gray (note: blue uses /256, not /257).
    let mut matrix = [[0.0f64; DCT_SIZE]; DCT_SIZE];
    for y in 0..DCT_SIZE {
        for x in 0..DCT_SIZE {
            let p = resized.get_pixel(x as u32, y as u32);
            // image crate gives 8-bit channels; goimagehash uses color.RGBA() 16-bit.
            let r = (p[0] as u32) * 257;
            let g = (p[1] as u32) * 257;
            let b = (p[2] as u32) * 257;
            matrix[y][x] =
                0.299 * (r / 257) as f64 + 0.587 * (g / 257) as f64 + 0.114 * (b / 256) as f64;
        }
    }

    dct2d_lee(&mut matrix);

    let mut coeffs = [0.0f64; HASH_SIZE * HASH_SIZE];
    for y in 0..HASH_SIZE {
        for x in 0..HASH_SIZE {
            coeffs[y * HASH_SIZE + x] = matrix[y][x];
        }
    }

    let median = median64(&coeffs);

    let mut hash = 0u64;
    for (i, c) in coeffs.iter().enumerate() {
        if *c > median {
            // goimagehash leftShiftSet(64 - idx - 1)
            hash |= 1u64 << (63 - i);
        }
    }

    Ok(hash)
}

fn median64(pixels: &[f64; 64]) -> f64 {
    let mut tmp = *pixels;
    tmp.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    // MedianOfPixelsFast64: even length → average of k-1 and k with k=len/2
    (tmp[31] + tmp[32]) / 2.0
}

/// DCT type II, unscaled — Byeong Gi Lee 1984 (goimagehash `DCT1D` / `forwardTransform`).
fn dct1d_lee(input: &mut [f64]) {
    let n = input.len();
    let mut scratch = vec![0.0f64; n];
    forward_transform(input, &mut scratch, n);
}

fn forward_transform(input: &mut [f64], scratch: &mut [f64], len: usize) {
    if len <= 1 {
        return;
    }

    let half = len / 2;
    for i in 0..half {
        let x = input[i];
        let y = input[len - 1 - i];
        scratch[i] = x + y;
        scratch[i + half] =
            (x - y) / (((i as f64 + 0.5) * std::f64::consts::PI / len as f64).cos() * 2.0);
    }

    // Swap roles: recurse on halves of scratch, using `input` as temporary storage
    // (same buffer reuse pattern as goimagehash).
    forward_transform(&mut scratch[..half], input, half);
    forward_transform(&mut scratch[half..len], input, half);

    for i in 0..half - 1 {
        input[i * 2] = scratch[i];
        input[i * 2 + 1] = scratch[i + half] + scratch[i + half + 1];
    }
    input[len - 2] = scratch[half - 1];
    input[len - 1] = scratch[len - 1];
}

fn dct2d_lee(matrix: &mut [[f64; DCT_SIZE]; DCT_SIZE]) {
    // Rows
    for row in matrix.iter_mut() {
        dct1d_lee(row);
    }
    // Columns
    let mut col = [0.0f64; DCT_SIZE];
    for x in 0..DCT_SIZE {
        for y in 0..DCT_SIZE {
            col[y] = matrix[y][x];
        }
        dct1d_lee(&mut col);
        for y in 0..DCT_SIZE {
            matrix[y][x] = col[y];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hamming_identical_zero() {
        assert_eq!(hamming_hex("0000000000000001", "0000000000000001"), Some(0));
        assert_eq!(
            hamming_hex("ffffffffffffffff", "0000000000000000"),
            Some(64)
        );
    }

    #[test]
    fn parse_hex_round_trip_and_rejects_garbage() {
        assert_eq!(parse_hex("0000000000000001"), Some(1));
        assert_eq!(parse_hex("ffffffffffffffff"), Some(u64::MAX));
        // Same trim-then-parse behavior hamming_hex has always had.
        assert_eq!(parse_hex(" 0123456789abcdef "), Some(0x0123_4567_89ab_cdef));
        assert_eq!(parse_hex("not-hex"), None);
        assert_eq!(parse_hex(""), None);
        // 16 hex digits max; 17 digits overflow u64.
        assert_eq!(parse_hex("10000000000000000"), None);
    }

    #[test]
    fn perception_hash_is_stable_for_flat_image() {
        let img = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            800,
            450,
            Rgba([128, 64, 32, 255]),
        ));
        let a = perception_hash(&img).expect("phash");
        let b = perception_hash(&img).expect("phash");
        assert_eq!(a, b);
        assert_ne!(format!("{a:016x}").len(), 0);
    }

    #[test]
    fn lee_dct_length2_basic() {
        let mut v = [1.0, 2.0];
        dct1d_lee(&mut v);
        // DCT-II Lee: [a+b, (a-b)/(2*cos(π/4))] = [3, (1-2)/(√2)] ≈ [3, -0.7071]
        assert!((v[0] - 3.0).abs() < 1e-9);
        assert!((v[1] - (-1.0 / std::f64::consts::SQRT_2)).abs() < 1e-9);
    }

    #[test]
    fn lee_dct_preserves_ac_energy() {
        // Impulse away from DC should produce non-zero AC after 2D DCT.
        let mut matrix = [[0.0f64; DCT_SIZE]; DCT_SIZE];
        matrix[10][20] = 255.0;
        dct2d_lee(&mut matrix);
        let mut ac = 0.0;
        for y in 0..HASH_SIZE {
            for x in 0..HASH_SIZE {
                if y != 0 || x != 0 {
                    ac += matrix[y][x].abs();
                }
            }
        }
        assert!(ac > 1.0, "expected AC energy in top-left 8x8, got {ac}");
    }

    #[test]
    fn perception_hash_differs_for_distinct_patterns() {
        let flat = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            256,
            256,
            Rgba([100, 100, 100, 255]),
        ));
        let mut stripes = image::RgbaImage::new(256, 256);
        for y in 0..256 {
            for x in 0..256 {
                let v = if (x / 16) % 2 == 0 { 20 } else { 220 };
                stripes.put_pixel(x, y, Rgba([v, v, v, 255]));
            }
        }
        let a = perception_hash(&flat).unwrap();
        let b = perception_hash(&DynamicImage::ImageRgba8(stripes)).unwrap();
        assert_ne!(a, b, "flat={a:016x} stripes={b:016x}");
    }
}
