//! Resolve ffmpeg/ffprobe and spawn them without flashing console windows.
//!
//! On Windows, console-subsystem tools (ffmpeg/ffprobe) spawn a visible console
//! window unless CREATE_NO_WINDOW is set. During a library scan that means one
//! popup per file — this module centralises the fix.
//!
//! Two spawn styles live here:
//!   * `silent_output` / `silent_status` — block to completion (used for probe,
//!     thumbnails, pHash; runs are short so blocking is fine).
//!   * `spawn_streaming` — piped stdout with a line reader, for long-running
//!     transcodes where we parse `-progress pipe:1` for percent done.

use std::{
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::OnceLock,
};

use anyhow::{Context, Result};

/// Windows: CREATE_NO_WINDOW — hide the child-process console.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Candidate names for each tool (bundled next to the exe, then PATH, then winget).
const FFPROBE_NAMES: &[&str] = &["ffprobe.exe", "ffprobe"];
const FFMPEG_NAMES: &[&str] = &["ffmpeg.exe", "ffmpeg"];

/// Run a media CLI tool silently (no console flash on Windows).
pub fn silent_output(cmd: &mut Command) -> Result<Output> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.output().context("running media tool")
}

/// Run a media CLI tool silently; returns exit status only.
///
/// stdout AND stderr are nulled: on damaged files ffmpeg can emit megabytes
/// of decoder errors ("Invalid NAL unit size", …) — enough to kill a dev
/// terminal. Callers only consume the status, so nothing is lost.
pub fn silent_status(cmd: &mut Command) -> Result<std::process::ExitStatus> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("running media tool")
}

/// Resolve ffprobe. Search order:
///   1. Next to the running exe (release bundle: `ffprobe.exe` in install dir)
///   2. `{exe_dir}/bin/ffprobe.exe` (optional subfolder layout)
///   3. PATH
///   4. gyan.dev winget package (dev machines — see docs/setup.md)
pub fn ffprobe_binary() -> Result<PathBuf> {
    resolve_tool(FFPROBE_NAMES)
        .context("ffprobe not found — install FFmpeg or bundle ffprobe.exe next to MaizeView")
}

/// Resolve ffmpeg (same search order as ffprobe).
pub fn ffmpeg_binary() -> Result<PathBuf> {
    resolve_tool(FFMPEG_NAMES)
        .context("ffmpeg not found — install FFmpeg or bundle ffmpeg.exe next to MaizeView")
}

fn resolve_tool(names: &[&str]) -> Result<PathBuf> {
    if let Some(exe_dir) = current_exe_dir() {
        for name in names {
            let direct = exe_dir.join(name);
            if direct.is_file() {
                return Ok(direct);
            }
            let in_bin = exe_dir.join("bin").join(name);
            if in_bin.is_file() {
                return Ok(in_bin);
            }
        }
    }

    if let Some(path) = find_on_path(names) {
        return Ok(path);
    }

    winget_ffmpeg_bin(names)
}

fn current_exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
}

fn find_on_path(names: &[&str]) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        for name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// gyan.dev FFmpeg from winget — common on the dev machine, optional elsewhere.
/// Tries essentials (`Gyan.FFmpeg`) then shared (`Gyan.FFmpeg.Shared`).
fn winget_ffmpeg_bin(names: &[&str]) -> Result<PathBuf> {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_default();
    if base.is_empty() {
        anyhow::bail!("LOCALAPPDATA not set");
    }
    let packages = [
        "Gyan.FFmpeg_Microsoft.Winget.Source_8wekyb3d8bbwe",
        "Gyan.FFmpeg.Shared_Microsoft.Winget.Source_8wekyb3d8bbwe",
    ];
    for pkg_name in packages {
        let pkg = Path::new(&base)
            .join("Microsoft/WinGet/Packages")
            .join(pkg_name);
        let Ok(entries) = std::fs::read_dir(&pkg) else {
            continue;
        };
        let mut dirs: Vec<_> = entries.flatten().collect();
        dirs.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
        for entry in dirs {
            let bin_dir = entry.path().join("bin");
            for name in names {
                let exe = bin_dir.join(name);
                if exe.is_file() {
                    return Ok(exe);
                }
            }
        }
    }
    anyhow::bail!("no matching binary in winget FFmpeg package")
}

// =========================================================================
// Streaming spawn — for long-running transcodes with progress reporting.
// =========================================================================

/// A spawned media process with a line reader over its piped stdout. The
/// caller reads progress lines (`out_time_ms=`, `progress=continue|end`) in a
/// loop and periodically checks `is_cancelled` for cooperative cancellation.
pub struct StreamedProcess {
    pub child: std::process::Child,
}

/// Spawn ffmpeg with piped stdout (for `-progress pipe:1` parsing) and no
/// console window. stderr is sent to null — ffmpeg writes verbose per-frame
/// stats there which we don't need, and if it's piped but never drained the
/// buffer fills and ffmpeg blocks/deadlocks. stdin is null so the process
/// never waits on terminal input.
///
/// The caller owns the `Child` and is responsible for waiting on it.
pub fn spawn_streaming(cmd: &mut Command) -> Result<std::process::Child> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null());
    cmd.spawn().context("spawning media tool")
}

/// Read the next `out_time_ms=<microseconds>` value from a line of ffmpeg's
/// `-progress pipe:1` output. Returns microseconds of media processed.
pub fn parse_progress_us(line: &str) -> Option<u64> {
    let line = line.trim();
    line.strip_prefix("out_time_ms=")
        .or_else(|| line.strip_prefix("out_time_us="))
        .and_then(|v| v.trim().parse::<u64>().ok())
}

/// True when ffmpeg's progress block reports this run finished.
pub fn is_progress_end(line: &str) -> bool {
    line.trim() == "progress=end"
}

// =========================================================================
// Hardware-encoder detection — pick the fastest available H.264 encoder.
// =========================================================================

/// Which H.264 encoder to use for a transcode, and the ffmpeg flags it needs.
/// Hardware encoders are preferred when present (much faster); libx264 is the
/// universal software fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoder {
    /// NVIDIA NVENC (`h264_nvenc`). `cq` controls quality.
    Nvenc,
    /// Intel QuickSync (`h264_qsv`).
    Qsv,
    /// AMD AMF (`h264_amf`).
    Amf,
    /// Software libx264 (always available). `crf` controls quality.
    Libx264,
}

impl Encoder {
    /// The `-c:v` value ffmpeg expects.
    pub fn codec_name(self) -> &'static str {
        match self {
            Encoder::Nvenc => "h264_nvenc",
            Encoder::Qsv => "h264_qsv",
            Encoder::Amf => "h264_amf",
            Encoder::Libx264 => "libx264",
        }
    }

    /// Human label for progress UI ("NVENC", "CPU x264", ...).
    pub fn label(self) -> &'static str {
        match self {
            Encoder::Nvenc => "NVENC",
            Encoder::Qsv => "QuickSync",
            Encoder::Amf => "AMF",
            Encoder::Libx264 => "CPU x264",
        }
    }

    /// Label including decode path (e.g. "NVENC+CUDA" vs "NVENC").
    pub fn label_with_decode(self, decode: DecodePath) -> &'static str {
        match (self, decode) {
            (Encoder::Nvenc, DecodePath::Cuda) => "NVENC+CUDA",
            (enc, _) => enc.label(),
        }
    }

    /// The quality-control argument for this encoder (CRF for software,
    /// CQ for hardware). Sensible default; not user-tunable in v1.
    pub fn quality_arg(self) -> &'static str {
        match self {
            // Hardware encoders use the constant-quality option.
            Encoder::Nvenc | Encoder::Amf => "-cq",
            Encoder::Qsv => "-global_quality",
            Encoder::Libx264 => "-crf",
        }
    }

    /// Preset argument name for this encoder.
    pub fn preset(self) -> &'static str {
        match self {
            Encoder::Nvenc => "p5",
            Encoder::Qsv => "veryfast",
            Encoder::Amf => "speed",
            Encoder::Libx264 => "veryfast",
        }
    }
}

/// Best-effort detection of the best available H.264 encoder. A mere listing
/// in `ffmpeg -encoders` is not enough — a hardware encoder can be compiled in
/// yet unusable if the GPU driver is too old (e.g. NVENC API version mismatch).
/// So we attempt a tiny throwaway encode with each candidate and pick the first
/// that actually succeeds. Falls back to libx264 (always available) so this
/// never fails outright.
pub fn detect_encoder() -> Encoder {
    let bin = match ffmpeg_binary() {
        Ok(b) => b,
        Err(_) => return Encoder::Libx264,
    };

    // Hardware candidates in preference order. Probe each with a 1-frame
    // encode of a black source into the null muxer — fast and writes nothing.
    for candidate in [Encoder::Nvenc, Encoder::Qsv, Encoder::Amf] {
        if probe_encoder(&bin, candidate) {
            return candidate;
        }
    }
    Encoder::Libx264
}

/// True if ffmpeg can actually open `enc` for a 1-frame encode. Catches the
/// "listed but driver too old" case (NVENC API mismatch, missing GPU, etc.).
///
/// Probe size must be ≥ NVENC’s minimum frame dimensions (64×64 fails with
/// “Frame Dimension less than the minimum supported value” even when the GPU
/// is fine — that false-negative forced CPU x264 on working NVIDIA boxes).
fn probe_encoder(bin: &Path, enc: Encoder) -> bool {
    let out = silent_status(Command::new(bin).args([
        "-y",
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "lavfi",
        "-i",
        "color=black:s=256x256:d=0.04",
        "-frames:v",
        "1",
        "-c:v",
        enc.codec_name(),
        "-f",
        "null",
        "-",
    ]));
    matches!(out, Ok(s) if s.success())
}

// =========================================================================
// CUDA decode + scale — faster than CPU decode when VRAM allows.
// =========================================================================

/// Where video frames are decoded / scaled before encode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodePath {
    /// `-hwaccel cuda` + `scale_cuda` (NVENC only).
    Cuda,
    /// CPU decode + libswscale `scale` (always safe).
    Software,
}

/// MiB kept free for desktop / browser / other GPU users. If free VRAM after
/// this reserve is below the estimate, we stay on software decode.
pub const VRAM_RESERVE_MIB: u64 = 1024;

/// Conservative peak VRAM (MiB) for cuda decode + scale_cuda + NVENC.
/// Overestimates on purpose — false fallback to software is fine; OOM is not.
pub fn estimate_cuda_vram_mib(src_w: u32, src_h: u32, target_h: u32) -> u64 {
    let src_w = src_w.max(64) as u64;
    let src_h = src_h.max(64) as u64;
    let target_h = target_h.max(64) as u64;
    let target_w = ((src_w * target_h) / src_h).max(64);
    // NV12-ish bytes/frame.
    let src_frame = src_w * src_h * 3 / 2;
    let dst_frame = target_w * target_h * 3 / 2;
    // Decoder DPB + filter graph + encoder surfaces (pessimistic).
    let working = (src_frame + dst_frame).saturating_mul(32);
    let overhead = 384 * 1024 * 1024u64;
    (working + overhead) / (1024 * 1024)
}

/// Free VRAM on the roomiest NVIDIA GPU (MiB), via `nvidia-smi`.
/// `None` if nvidia-smi missing/fails — caller must not assume CUDA is safe.
pub fn query_nvidia_vram_free_mib() -> Option<u64> {
    let mut cmd = Command::new("nvidia-smi");
    cmd.args(["--query-gpu=memory.free", "--format=csv,noheader,nounits"]);
    let out = silent_output(&mut cmd).ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            line.trim()
                .split_whitespace()
                .next()
                .and_then(|t| t.parse::<u64>().ok())
        })
        .max()
}

/// Cached: ffmpeg can run hwupload_cuda → scale_cuda → h264_nvenc.
fn cuda_scale_pipeline_ok() -> bool {
    static OK: OnceLock<bool> = OnceLock::new();
    *OK.get_or_init(|| {
        let Ok(bin) = ffmpeg_binary() else {
            return false;
        };
        probe_cuda_scale_nvenc(&bin)
    })
}

fn probe_cuda_scale_nvenc(bin: &Path) -> bool {
    let out = silent_status(Command::new(bin).args([
        "-y",
        "-hide_banner",
        "-loglevel",
        "error",
        "-init_hw_device",
        "cuda=cuda",
        "-filter_hw_device",
        "cuda",
        "-f",
        "lavfi",
        "-i",
        "color=black:s=1280x720:d=0.1",
        "-vf",
        "hwupload_cuda,scale_cuda=-2:480",
        "-frames:v",
        "1",
        "-c:v",
        "h264_nvenc",
        "-f",
        "null",
        "-",
    ]));
    matches!(out, Ok(s) if s.success())
}

/// Pick decode path for one file. CUDA only with NVENC + working pipeline +
/// enough free VRAM (estimate + [`VRAM_RESERVE_MIB`]).
pub fn choose_decode_path(encoder: Encoder, src_w: u32, src_h: u32, target_h: u32) -> DecodePath {
    if encoder != Encoder::Nvenc {
        return DecodePath::Software;
    }
    if !cuda_scale_pipeline_ok() {
        return DecodePath::Software;
    }
    let Some(free) = query_nvidia_vram_free_mib() else {
        tracing::info!("nvidia-smi unavailable — using software decode with NVENC");
        return DecodePath::Software;
    };
    let need = estimate_cuda_vram_mib(src_w, src_h, target_h);
    let required = need.saturating_add(VRAM_RESERVE_MIB);
    if free >= required {
        tracing::info!(
            free_mib = free,
            need_mib = need,
            reserve_mib = VRAM_RESERVE_MIB,
            "using CUDA decode+scale"
        );
        DecodePath::Cuda
    } else {
        tracing::warn!(
            free_mib = free,
            need_mib = need,
            reserve_mib = VRAM_RESERVE_MIB,
            "insufficient VRAM for CUDA decode — falling back to software decode"
        );
        DecodePath::Software
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_out_time_us() {
        assert_eq!(parse_progress_us("out_time_ms=12345678"), Some(12_345_678));
        assert_eq!(parse_progress_us("out_time_us=999"), Some(999));
        assert_eq!(parse_progress_us("frame=42"), None);
        assert_eq!(parse_progress_us("nonsense"), None);
    }

    #[test]
    fn detects_progress_end() {
        assert!(is_progress_end("progress=end"));
        assert!(is_progress_end("  progress=end  "));
        assert!(!is_progress_end("progress=continue"));
    }

    #[test]
    fn cuda_vram_estimate_grows_with_resolution() {
        let hd = estimate_cuda_vram_mib(1920, 1080, 720);
        let uhd = estimate_cuda_vram_mib(3840, 2160, 1080);
        assert!(uhd > hd, "4K estimate {uhd} should exceed 1080p {hd}");
        assert!(uhd + VRAM_RESERVE_MIB < 24 * 1024); // fits a 24GB card with headroom
    }

    #[test]
    fn encoder_label_includes_cuda() {
        assert_eq!(
            Encoder::Nvenc.label_with_decode(DecodePath::Cuda),
            "NVENC+CUDA"
        );
        assert_eq!(
            Encoder::Nvenc.label_with_decode(DecodePath::Software),
            "NVENC"
        );
    }

    #[test]
    fn encoder_codec_and_label() {
        assert_eq!(Encoder::Nvenc.codec_name(), "h264_nvenc");
        assert_eq!(Encoder::Libx264.codec_name(), "libx264");
        assert_eq!(Encoder::Libx264.quality_arg(), "-crf");
        assert_eq!(Encoder::Nvenc.quality_arg(), "-cq");
    }

    #[test]
    fn detect_encoder_never_panics_without_ffmpeg() {
        // Even with no ffmpeg on PATH this must return a valid fallback.
        let enc = detect_encoder();
        assert!(matches!(
            enc,
            Encoder::Nvenc | Encoder::Qsv | Encoder::Amf | Encoder::Libx264
        ));
    }
}
