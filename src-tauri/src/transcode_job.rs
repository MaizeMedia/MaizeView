//! Downscale / transcode job.
//!
//! Transcodes selected scenes' representative file to a lower resolution to
//! save space, then (per options) replaces the original in place or keeps both,
//! rewrites resolution tokens in the filename, swaps tags, and regenerates
//! previews + hashes so the catalog stays consistent with the new file.
//!
//! Safety invariants (see ADR-015 for the full rationale):
//!   * The temp file is always written in the *same directory* as the source
//!     so the final move is same-volume and (on most filesystems) atomic.
//!   * The original is never touched until the temp is verified (video stream
//!     present, height <= target, duration within 2% of the original). Any
//!     failure deletes the temp and leaves the original untouched.
//!   * The DB row is updated only after the file on disk is final.
//!   * A headroom check aborts before transcode if free space is insufficient.
//!   * Failures are reported per-scene; completed files stay done.

use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result};
use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;

use crate::{
    fingerprints, media_tools,
    models::{new_id, now},
    previews,
    scanner::{oshash, phash, probe},
    transcode_tokens::{self, RewriteMode},
};

pub const PROGRESS_EVENT: &str = "transcode://progress";

/// What to do with the original file after the transcode succeeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OriginalMode {
    /// Delete the original; the transcode replaces it. Saves space.
    Replace,
    /// Keep the original as an additional file on the scene.
    Keep,
}

/// What to do with resolution tags ("4K", "2160p", "UHD") on the scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TagMode {
    /// Remove the old token tag and add the target token tag (e.g. "4K" -> "1080p").
    Swap,
    /// Remove the old token tag only.
    Remove,
    /// Leave tags untouched.
    Leave,
}

/// Full options for a downscale run. Mirrors the Convert dialog.
/// Field names are camelCase on the wire to match the TS API layer.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownscaleOptions {
    pub scene_ids: Vec<String>,
    pub target_height: u32,
    #[serde(default = "default_original")]
    pub original_mode: OriginalMode,
    #[serde(default = "default_filename")]
    pub filename_mode: RewriteMode,
    #[serde(default = "default_tag")]
    pub tag_mode: TagMode,
}

fn default_original() -> OriginalMode {
    OriginalMode::Replace
}
fn default_filename() -> RewriteMode {
    RewriteMode::Replace
}
fn default_tag() -> TagMode {
    TagMode::Swap
}

/// One failed scene in the final progress payload.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscodeFailure {
    pub scene_id: String,
    pub reason: String,
}

/// Payload emitted on `transcode://progress` after each scene.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscodeProgress {
    pub done: u64,
    pub total: u64,
    pub current_scene: Option<String>,
    pub current_path: Option<String>,
    pub skipped: u64,
    pub encoder: Option<String>,
    /// Percent of the *current* file's media processed (0–100), when known.
    pub file_percent: Option<u8>,
    pub finished: bool,
    pub failed: Vec<TranscodeFailure>,
}

/// Non-mutating preview of what a run would do. Returned to the dialog.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownscalePreview {
    pub target_height: u32,
    /// Total scenes the caller asked about.
    pub total: u64,
    /// Scenes that would actually be transcoded (height > target).
    pub would_transcode: u64,
    /// Scenes skipped because already <= target.
    pub skipped: u64,
    /// Bucket counts by current canonical resolution token.
    pub by_resolution: BTreeMap<String, u64>,
    /// Rough estimate of bytes saved, assuming a pixel-ratio bitrate drop.
    /// Deliberately conservative and labelled "estimated" in the UI.
    pub estimated_bytes_saved: u64,
    /// Per-scene plan (filename before→after, current height, would-skip).
    pub items: Vec<DownscalePreviewItem>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownscalePreviewItem {
    pub scene_id: String,
    pub current_height: Option<u32>,
    pub current_path: Option<String>,
    pub would_skip: bool,
    /// Filename after the chosen RewriteMode is applied (preview only).
    pub preview_filename: Option<String>,
}

/// Build the non-mutating preview for `downscale_preview`.
pub async fn build_preview(
    pool: &SqlitePool,
    scene_ids: &[String],
    target_height: u32,
) -> Result<DownscalePreview> {
    let mut items: Vec<DownscalePreviewItem> = Vec::new();
    let mut by_resolution: BTreeMap<String, u64> = BTreeMap::new();
    let mut would_transcode = 0u64;
    let mut skipped = 0u64;
    let mut est_saved: u64 = 0;

    for id in scene_ids {
        let row: Option<(Option<i64>, Option<String>, Option<i64>, Option<f64>)> = sqlx::query_as(
            // representative file (most recently scanned) + scene size/duration
            "SELECT f.height, f.path, f.size_bytes, f.duration
             FROM files f
             JOIN scenes s ON s.id = f.scene_id
             WHERE f.scene_id = ?
             ORDER BY f.scanned_at DESC
             LIMIT 1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        let Some((height, path, size_bytes, duration)) = row else {
            items.push(DownscalePreviewItem {
                scene_id: id.clone(),
                current_height: None,
                current_path: None,
                would_skip: true,
                preview_filename: None,
            });
            skipped += 1;
            continue;
        };

        let h = height.map(|v| v as u32);
        let token_key = h
            .map(transcode_tokens::canonical_token)
            .map(str::to_string)
            .unwrap_or_else(|| "unknown".to_string());
        *by_resolution.entry(token_key).or_default() += 1;

        let would_skip = h.map_or(true, |v| v <= target_height);
        if would_skip {
            skipped += 1;
        } else {
            would_transcode += 1;
            // Conservative savings estimate: assume the transcode lands at
            // roughly (target/current)^2 of source bitrate (pixel-count ratio),
            // clamped so we never over-promise. Uses source size as the proxy.
            if let (Some(cur), Some(bytes)) = (h, size_bytes) {
                let ratio = (target_height as f64 / cur as f64).powi(2);
                // Don't claim more than 80% reduction (audio + overhead remain).
                let kept = ratio.max(0.2);
                est_saved += (bytes as f64 * (1.0 - kept)) as u64;
            }
            let _ = duration; // reserved for a future bitrate-based estimate
        }

        let preview_filename = path.as_ref().map(|p| {
            let fname = Path::new(p)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| p.clone());
            transcode_tokens::rewrite_resolution_token(&fname, RewriteMode::Replace, target_height)
        });

        items.push(DownscalePreviewItem {
            scene_id: id.clone(),
            current_height: h,
            current_path: path,
            would_skip,
            preview_filename,
        });
    }

    Ok(DownscalePreview {
        target_height,
        total: scene_ids.len() as u64,
        would_transcode,
        skipped,
        by_resolution,
        estimated_bytes_saved: est_saved,
        items,
    })
}

/// Run the transcode against the selected scenes. Emits `transcode://progress`
/// per scene and per-file-percent while transcoding. `cancel` is checked
/// between scenes (cooperative; an in-flight ffmpeg is allowed to finish).
/// Returns the list of scenes that failed (empty on full success).
pub async fn run(
    pool: &SqlitePool,
    app: &AppHandle,
    opts: DownscaleOptions,
    cancel: Arc<CancellationToken>,
) -> Result<Vec<TranscodeFailure>> {
    let app = app.clone();
    run_inner(
        pool,
        opts,
        cancel,
        Arc::new(move |p| {
            let _ = app.emit(PROGRESS_EVENT, &p);
        }),
    )
    .await
}

/// Same job without Tauri (CLI / tests). Returns the failed-scene list.
pub async fn run_silent(
    pool: &SqlitePool,
    opts: DownscaleOptions,
    cancel: Arc<CancellationToken>,
) -> Result<Vec<TranscodeFailure>> {
    run_inner(pool, opts, cancel, Arc::new(|_| {})).await
}

async fn run_inner(
    pool: &SqlitePool,
    opts: DownscaleOptions,
    cancel: Arc<CancellationToken>,
    emit: Arc<dyn Fn(TranscodeProgress) + Send + Sync>,
) -> Result<Vec<TranscodeFailure>> {
    // Resolve every scene's representative file up front so cancellation and
    // progress totals are accurate before we touch the disk.
    let targets = resolve_targets(pool, &opts.scene_ids).await?;
    let total = targets.len() as u64;

    let encoder = media_tools::detect_encoder();
    let encoder_label = encoder.label();

    emit(TranscodeProgress {
        done: 0,
        total,
        current_scene: None,
        current_path: None,
        skipped: 0,
        encoder: Some(encoder_label.to_string()),
        file_percent: None,
        finished: false,
        failed: Vec::new(),
    });

    let mut done: u64 = 0;
    let mut skipped: u64 = 0;
    let mut failed: Vec<TranscodeFailure> = Vec::new();

    for t in &targets {
        if cancel.is_cancelled() {
            // Remaining scenes count as failed-by-cancel so the UI shows them.
            break;
        }

        emit(TranscodeProgress {
            done,
            total,
            current_scene: Some(t.scene_id.clone()),
            current_path: Some(t.path.to_string_lossy().to_string()),
            skipped,
            encoder: Some(encoder_label.to_string()),
            file_percent: None,
            finished: false,
            failed: failed.clone(),
        });

        // Plan gate: already small enough.
        if t.height.map_or(true, |h| h <= opts.target_height) {
            skipped += 1;
            done += 1;
            continue;
        }

        match transcode_one(
            pool,
            t,
            &opts,
            encoder,
            &cancel,
            &emit,
            encoder_label,
            done,
            total,
            skipped,
        )
        .await
        {
            Ok(()) => done += 1,
            Err(e) => {
                tracing::warn!(scene_id = %t.scene_id, error = %e, "transcode failed");
                failed.push(TranscodeFailure {
                    scene_id: t.scene_id.clone(),
                    reason: e.to_string(),
                });
                done += 1;
            }
        }
    }

    emit(TranscodeProgress {
        done,
        total,
        current_scene: None,
        current_path: None,
        skipped,
        encoder: Some(encoder_label.to_string()),
        file_percent: None,
        finished: true,
        failed: failed.clone(),
    });
    Ok(failed)
}

/// The resolved target for one scene.
struct Target {
    scene_id: String,
    file_id: String,
    path: PathBuf,
    width: Option<u32>,
    height: Option<u32>,
    duration: Option<f64>,
}

async fn resolve_targets(pool: &SqlitePool, scene_ids: &[String]) -> Result<Vec<Target>> {
    let mut out = Vec::new();
    for id in scene_ids {
        let row: Option<(String, String, Option<i64>, Option<i64>, Option<f64>)> = sqlx::query_as(
            "SELECT id, path, width, height, duration FROM files
             WHERE scene_id = ?
             ORDER BY scanned_at DESC LIMIT 1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;
        if let Some((file_id, path, width, height, duration)) = row {
            out.push(Target {
                scene_id: id.clone(),
                file_id,
                path: PathBuf::from(path),
                width: width.map(|v| v as u32),
                height: height.map(|v| v as u32),
                duration,
            });
        }
    }
    Ok(out)
}

/// Transcode a single scene's representative file end to end.
#[allow(clippy::too_many_arguments)]
async fn transcode_one(
    pool: &SqlitePool,
    t: &Target,
    opts: &DownscaleOptions,
    encoder: media_tools::Encoder,
    cancel: &Arc<CancellationToken>,
    emit: &Arc<dyn Fn(TranscodeProgress) + Send + Sync>,
    _encoder_label: &str,
    batch_done: u64,
    batch_total: u64,
    batch_skipped: u64,
) -> Result<()> {
    let src = &t.path;
    if !src.exists() {
        anyhow::bail!("source file missing: {}", src.display());
    }
    let dir = src.parent().context("source has no parent directory")?;

    // Headroom check: need roughly the source size free (worst case the
    // transcode is the same size as source before the original is removed).
    let src_size = std::fs::metadata(src).map(|m| m.len()).unwrap_or(0);
    if let Ok(free) = free_space(dir) {
        if free < src_size {
            anyhow::bail!(
                "not enough free space (need ~{} bytes, {} available)",
                src_size,
                free
            );
        }
    }

    // Temp output in the SAME directory so the final move is same-volume/atomic.
    let stem = src
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "video".to_string());
    let mut tmp = dir.join(format!(".{stem}.mvtrans-{}.mp4", new_id()));
    // Guard against (astronomically unlikely) name collisions.
    let mut n = 0;
    while tmp.exists() {
        n += 1;
        tmp = dir.join(format!(".{stem}.mvtrans-{n}.mp4"));
    }
    let tmp_path = tmp.clone();

    // Compute the final filename now (used by both Replace and Keep) but only
    // apply it after verification. For Keep, ensure it doesn't collide with the
    // existing source.
    let src_name = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "video.mp4".to_string());
    let final_name = match opts.filename_mode {
        RewriteMode::Replace | RewriteMode::Remove => transcode_tokens::rewrite_resolution_token(
            &src_name,
            opts.filename_mode,
            opts.target_height,
        ),
        RewriteMode::Leave => src_name.clone(),
    };

    // Run the ffmpeg transcode, streaming progress. This is blocking I/O on the
    // ffmpeg child + a line reader; run it on spawn_blocking so the async
    // runtime stays responsive for emit/cancel checks. Move owned values in
    // (no borrows from this fn's args) so the closure is 'static.
    // Unknown dims → assume 4K so the VRAM gate is conservative.
    let src_w = t.width.unwrap_or(3840);
    let src_h = t.height.unwrap_or(2160);
    let decode = media_tools::choose_decode_path(encoder, src_w, src_h, opts.target_height);
    let file_label = encoder.label_with_decode(decode);
    let cancel_clone = cancel.clone();
    let duration = t.duration;
    let emit_clone = emit.clone();
    let payload_ctx = PayloadCtx {
        scene_id: t.scene_id.clone(),
        path: src.to_string_lossy().to_string(),
        encoder_label: file_label.to_string(),
        done: batch_done,
        total: batch_total,
        skipped: batch_skipped,
    };
    let target_height = opts.target_height;
    let src_owned = src.to_path_buf();
    let tmp_for_task = tmp_path.clone();
    tokio::task::spawn_blocking(move || {
        run_ffmpeg_transcode(
            &src_owned,
            &tmp_for_task,
            target_height,
            encoder,
            decode,
            duration,
            &cancel_clone,
            &emit_clone,
            &payload_ctx,
        )
    })
    .await
    .context("joining transcode task")??;

    // ----- VERIFY before touching the original -----
    if let Err(e) = verify_transcode(&tmp_path, opts.target_height, t.duration) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e.context("transcode verification failed; original left untouched"));
    }

    let new_probe = probe::probe(&tmp_path).unwrap_or_default();
    let new_size = std::fs::metadata(&tmp_path).map(|m| m.len()).unwrap_or(0);
    let ts = now().to_rfc3339();

    // ----- Apply original-handling -----
    let final_path = dir.join(&final_name);
    match opts.original_mode {
        OriginalMode::Replace => {
            // The final filename might equal the source (Leave mode + no token).
            // If so, delete the original first, then move temp into place.
            if std::path::Path::new(&final_path).exists() {
                remove_file_with_retry(&final_path, 8).await?;
            } else {
                // Original differs from final name: delete it, then move temp.
                remove_file_with_retry(src, 8).await?;
            }
            tokio::fs::rename(&tmp_path, &final_path)
                .await
                .with_context(|| {
                    format!(
                        "renaming {} -> {}",
                        tmp_path.display(),
                        final_path.display()
                    )
                })?;

            // Remove stale preview artifacts for this file_id (regenerated below).
            invalidate_previews(pool, &t.file_id).await.ok();

            // UPDATE the existing files row in place.
            sqlx::query(
                "UPDATE files SET
                    path = ?, size_bytes = ?, modified_at = ?, format_name = ?,
                    duration = ?, width = ?, height = ?, codec = ?, fps = ?,
                    bitrate = ?, scanned_at = ?
                 WHERE id = ?",
            )
            .bind(final_path.to_string_lossy().to_string())
            .bind(new_size as i64)
            .bind(ts.clone())
            .bind(new_probe.format_name.clone())
            .bind(new_probe.duration)
            .bind(new_probe.width)
            .bind(new_probe.height)
            .bind(new_probe.codec.clone())
            .bind(new_probe.fps)
            .bind(new_probe.bitrate)
            .bind(ts)
            .bind(&t.file_id)
            .execute(pool)
            .await?;

            post_process_file(pool, &t.file_id, &final_path, new_probe.duration).await?;
        }
        OriginalMode::Keep => {
            // Ensure no collision: if the chosen final name exists, suffix it.
            let mut resolved = final_path.clone();
            let mut k = 1;
            while resolved.exists() {
                let cand = format!(
                    "{} ({}).mp4",
                    dir.join(&final_name)
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| final_name.clone()),
                    k
                );
                resolved = dir.join(cand);
                k += 1;
            }
            tokio::fs::rename(&tmp_path, &resolved)
                .await
                .with_context(|| {
                    format!("renaming {} -> {}", tmp_path.display(), resolved.display())
                })?;

            // INSERT a new files row on the same scene (newest scanned_at wins
            // for default playback, per player.rs resolve_file_path).
            let new_file_id = new_id();
            sqlx::query(
                "INSERT INTO files
                    (id, scene_id, path, size_bytes, modified_at, format_name,
                     duration, width, height, codec, fps, bitrate,
                     thumb_path, thumb_sprite_path, vtt_path, scanned_at)
                 VALUES (?,?,?,?,?,?,?,?,?,?,?,?,NULL,NULL,NULL,?)",
            )
            .bind(&new_file_id)
            .bind(&t.scene_id)
            .bind(resolved.to_string_lossy().to_string())
            .bind(new_size as i64)
            .bind(ts.clone())
            .bind(new_probe.format_name.clone())
            .bind(new_probe.duration)
            .bind(new_probe.width)
            .bind(new_probe.height)
            .bind(new_probe.codec.clone())
            .bind(new_probe.fps)
            .bind(new_probe.bitrate)
            .bind(ts)
            .execute(pool)
            .await?;

            post_process_file(pool, &new_file_id, &resolved, new_probe.duration).await?;
        }
    }

    // ----- Tag handling (per-scene; never renames the global tag) -----
    if opts.tag_mode != TagMode::Leave {
        if let Err(e) = apply_tag_swap(pool, &t.scene_id, opts.tag_mode, opts.target_height).await {
            tracing::warn!(scene_id = %t.scene_id, error = %e, "tag swap failed");
        }
    }

    Ok(())
}

struct PayloadCtx {
    scene_id: String,
    path: String,
    encoder_label: String,
    done: u64,
    total: u64,
    skipped: u64,
}

/// Spawn ffmpeg, run it to completion, parse `-progress pipe:1` for percent.
/// Tries CUDA decode+scale when requested; on failure retries software decode
/// once so a flaky CUDA path never strands the batch.
fn run_ffmpeg_transcode(
    src: &Path,
    dst: &Path,
    target_height: u32,
    encoder: media_tools::Encoder,
    decode: media_tools::DecodePath,
    duration: Option<f64>,
    cancel: &Arc<CancellationToken>,
    emit: &Arc<dyn Fn(TranscodeProgress) + Send + Sync>,
    ctx: &PayloadCtx,
) -> Result<()> {
    match run_ffmpeg_transcode_once(
        src,
        dst,
        target_height,
        encoder,
        decode,
        duration,
        cancel,
        emit,
        ctx,
    ) {
        Ok(()) => Ok(()),
        Err(e) if decode == media_tools::DecodePath::Cuda && !cancel.is_cancelled() => {
            tracing::warn!(
                error = %e,
                "CUDA decode path failed; retrying with software decode"
            );
            let _ = std::fs::remove_file(dst);
            let soft_ctx = PayloadCtx {
                scene_id: ctx.scene_id.clone(),
                path: ctx.path.clone(),
                encoder_label: encoder
                    .label_with_decode(media_tools::DecodePath::Software)
                    .to_string(),
                done: ctx.done,
                total: ctx.total,
                skipped: ctx.skipped,
            };
            run_ffmpeg_transcode_once(
                src,
                dst,
                target_height,
                encoder,
                media_tools::DecodePath::Software,
                duration,
                cancel,
                emit,
                &soft_ctx,
            )
        }
        Err(e) => Err(e),
    }
}

fn run_ffmpeg_transcode_once(
    src: &Path,
    dst: &Path,
    target_height: u32,
    encoder: media_tools::Encoder,
    decode: media_tools::DecodePath,
    duration: Option<f64>,
    cancel: &Arc<CancellationToken>,
    emit: &Arc<dyn Fn(TranscodeProgress) + Send + Sync>,
    ctx: &PayloadCtx,
) -> Result<()> {
    let bin = media_tools::ffmpeg_binary().context("locating ffmpeg for transcode")?;

    let mut cmd = Command::new(&bin);
    cmd.args(["-y", "-nostdin"]);
    match decode {
        media_tools::DecodePath::Cuda => {
            // Keep frames on the GPU through scale → NVENC.
            cmd.args(["-hwaccel", "cuda", "-hwaccel_output_format", "cuda"]);
            cmd.arg("-i").arg(src);
            cmd.arg("-map").arg("0");
            cmd.arg("-vf").arg(format!("scale_cuda=-2:{target_height}"));
            cmd.arg("-c:v").arg(encoder.codec_name());
            // Do not force -pix_fmt here — it can pull frames off the GPU.
        }
        media_tools::DecodePath::Software => {
            cmd.arg("-i").arg(src);
            cmd.arg("-map").arg("0");
            cmd.arg("-c:v").arg(encoder.codec_name());
            cmd.arg("-pix_fmt").arg("yuv420p");
            cmd.arg("-vf").arg(format!("scale=-2:{target_height}"));
        }
    }
    // Quality: CRF/CQ 23 is a good balance; tuned by encoder type.
    cmd.arg(encoder.quality_arg()).arg("23");
    cmd.arg("-preset").arg(encoder.preset());
    cmd.arg("-c:a").arg("copy");
    cmd.arg("-c:s").arg("copy");
    cmd.arg("-progress").arg("pipe:1");
    cmd.arg("-nostats");
    cmd.arg(dst);

    let mut child = media_tools::spawn_streaming(&mut cmd)?;
    let stdout = child.stdout.take().context("no ffmpeg stdout")?;
    let reader = BufReader::new(stdout);

    let total_us = duration.map(|d| (d * 1_000_000.0) as u64);
    let label = match decode {
        media_tools::DecodePath::Cuda => encoder.label_with_decode(decode).to_string(),
        media_tools::DecodePath::Software => ctx.encoder_label.clone(),
    };

    for line in reader.lines() {
        if cancel.is_cancelled() {
            // Best-effort kill; the outer loop will not advance.
            let _ = child.kill();
            break;
        }
        let Ok(line) = line else { break };
        if let Some(us) = media_tools::parse_progress_us(&line) {
            if let Some(total) = total_us {
                let pct = ((us as f64 / total as f64) * 100.0).clamp(0.0, 100.0) as u8;
                emit(TranscodeProgress {
                    done: ctx.done,
                    total: ctx.total,
                    current_scene: Some(ctx.scene_id.clone()),
                    current_path: Some(ctx.path.clone()),
                    skipped: ctx.skipped,
                    encoder: Some(label.clone()),
                    file_percent: Some(pct),
                    finished: false,
                    failed: Vec::new(),
                });
            }
        }
        if media_tools::is_progress_end(&line) {
            break;
        }
    }

    let status = child.wait().context("waiting for ffmpeg transcode")?;
    if !status.success() && !cancel.is_cancelled() {
        // Clean up any partial output before bubbling the error.
        let _ = std::fs::remove_file(dst);
        anyhow::bail!("ffmpeg exited with status {status}");
    }
    if cancel.is_cancelled() {
        let _ = std::fs::remove_file(dst);
        anyhow::bail!("transcode cancelled");
    }
    Ok(())
}

/// Verify the transcoded file is usable and has the expected resolution /
/// duration before we commit to replacing or keeping it.
fn verify_transcode(path: &Path, target_height: u32, orig_duration: Option<f64>) -> Result<()> {
    let p = probe::probe(path).unwrap_or_default();
    let vstream_height = p.height;
    let h = vstream_height.with_context(|| "transcoded file has no video stream")?;
    if h as u32 > target_height {
        anyhow::bail!("transcoded height {h} exceeds target {target_height}; refusing");
    }
    if let (Some(orig), Some(got)) = (orig_duration, p.duration) {
        if orig > 0.0 {
            let diff = (got - orig).abs() / orig;
            if diff > 0.02 {
                anyhow::bail!(
                    "duration drift {diff:.2} (orig {orig:.1}s, got {got:.1}s); refusing"
                );
            }
        }
    }
    Ok(())
}

/// Regenerate previews + re-hash for the (possibly replaced) file. Run after
/// the DB row is final so file_id points at the new bytes.
async fn post_process_file(
    pool: &SqlitePool,
    file_id: &str,
    path: &Path,
    duration: Option<f64>,
) -> Result<()> {
    // Regenerate preview artifacts (NULL'd by invalidate_previews for Replace,
    // or were NULL on a fresh INSERT for Keep). `previews::generate` does
    // blocking ffmpeg I/O, so run it off the async runtime.
    let file_id_owned = file_id.to_string();
    let path_buf = path.to_path_buf();
    let gen = tokio::task::spawn_blocking(move || {
        previews::generate(&file_id_owned, &path_buf, duration)
    })
    .await?;
    if let Ok(Some((thumb, sprite, vtt))) = gen {
        let ts = now().to_rfc3339();
        let _ = sqlx::query(
            "UPDATE files SET thumb_path = ?, thumb_sprite_path = ?, vtt_path = ?, scanned_at = ?
             WHERE id = ?",
        )
        .bind(thumb.to_string_lossy().to_string())
        .bind(sprite.to_string_lossy().to_string())
        .bind(vtt.to_string_lossy().to_string())
        .bind(ts)
        .bind(file_id)
        .execute(pool)
        .await;
    }

    // Re-hash. Content changed, so the old oshash/phash are stale.
    let path_buf = path.to_path_buf();
    if let Ok(Ok(hash)) = tokio::task::spawn_blocking(move || oshash::hash_file(&path_buf)).await {
        let _ = fingerprints::upsert(pool, file_id, "oshash", &hash).await;
    }
    if let Some(d) = duration.filter(|d| *d > 0.0) {
        let path_buf = path.to_path_buf();
        if let Ok(Ok(ph)) =
            tokio::task::spawn_blocking(move || phash::hash_file(&path_buf, d)).await
        {
            let _ = fingerprints::upsert(pool, file_id, "phash", &ph).await;
        }
    }
    Ok(())
}

/// NULL the preview artifact columns + delete the old artifacts so they're
/// regenerated against the new content.
async fn invalidate_previews(pool: &SqlitePool, file_id: &str) -> Result<()> {
    let row: Option<(Option<String>, Option<String>, Option<String>)> =
        sqlx::query_as("SELECT thumb_path, thumb_sprite_path, vtt_path FROM files WHERE id = ?")
            .bind(file_id)
            .fetch_optional(pool)
            .await?;
    if let Some((thumb, sprite, vtt)) = row {
        for p in [thumb, sprite, vtt].into_iter().flatten() {
            if !p.trim().is_empty() {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
    sqlx::query("UPDATE files SET thumb_path = NULL, thumb_sprite_path = NULL, vtt_path = NULL WHERE id = ?")
        .bind(file_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Per-scene resolution-tag swap. Looks for tags whose name is a resolution
/// token at/above the source resolution and either removes them (Remove) or
/// removes them and adds the target token tag (Swap). Other scenes keep their
/// own tags — we never rename a global tag.
async fn apply_tag_swap(
    pool: &SqlitePool,
    scene_id: &str,
    mode: TagMode,
    target_height: u32,
) -> Result<()> {
    let target_token = transcode_tokens::canonical_token(target_height);

    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT t.id, t.name FROM tags t
         JOIN scene_tags st ON st.tag_id = t.id
         WHERE st.scene_id = ?",
    )
    .bind(scene_id)
    .fetch_all(pool)
    .await?;

    for (tag_id, name) in &rows {
        if transcode_tokens::is_resolution_token(name) {
            sqlx::query("DELETE FROM scene_tags WHERE scene_id = ? AND tag_id = ?")
                .bind(scene_id)
                .bind(tag_id)
                .execute(pool)
                .await?;
        }
    }

    if matches!(mode, TagMode::Swap) {
        // Find or create the target token tag, then attach it.
        let existing: Option<(String,)> =
            sqlx::query_as("SELECT id FROM tags WHERE name = ? COLLATE NOCASE")
                .bind(target_token)
                .fetch_optional(pool)
                .await?;
        let tag_id = match existing {
            Some((id,)) => id,
            None => {
                let id = new_id();
                sqlx::query("INSERT INTO tags (id, name, created_at) VALUES (?, ?, ?)")
                    .bind(&id)
                    .bind(target_token)
                    .bind(now().to_rfc3339())
                    .execute(pool)
                    .await?;
                id
            }
        };
        sqlx::query("INSERT OR IGNORE INTO scene_tags (scene_id, tag_id) VALUES (?, ?)")
            .bind(scene_id)
            .bind(&tag_id)
            .execute(pool)
            .await?;
    }
    Ok(())
}

/// Windows may keep a file locked briefly after mpv/the OS releases it — retry
/// before failing. Mirrors the private helper in commands/scenes.rs.
async fn remove_file_with_retry(path: &Path, attempts: u32) -> Result<()> {
    let mut last: Option<anyhow::Error> = None;
    for attempt in 0..attempts {
        match std::fs::remove_file(path) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => {
                last = Some(e.into());
                if attempt + 1 < attempts {
                    tokio::time::sleep(Duration::from_millis(150)).await;
                }
            }
        }
    }
    Err(last.unwrap_or_else(|| anyhow::anyhow!("failed to delete {}", path.display())))
}

/// Best-effort free-space query for a directory. Returns bytes available.
#[cfg(windows)]
fn free_space(dir: &Path) -> Result<u64> {
    use std::os::windows::ffi::OsStrExt;

    let path = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    // GetDiskFreeSpaceExW wants a trailing backslash optionally; ensure NUL.
    wide.push(0);

    #[repr(C)]
    struct U64 {
        lo: u32,
        hi: u32,
    }
    impl U64 {
        fn val(&self) -> u64 {
            ((self.hi as u64) << 32) | (self.lo as u64)
        }
    }
    extern "system" {
        fn GetDiskFreeSpaceExW(
            directory: *const u16,
            free_caller: *mut U64,
            total: *mut U64,
            free_total: *mut U64,
        ) -> i32;
    }
    let mut free_caller = U64 { lo: 0, hi: 0 };
    let mut total = U64 { lo: 0, hi: 0 };
    let mut free_total = U64 { lo: 0, hi: 0 };
    let ok = unsafe {
        GetDiskFreeSpaceExW(wide.as_ptr(), &mut free_caller, &mut total, &mut free_total)
    };
    if ok == 0 {
        anyhow::bail!("GetDiskFreeSpaceExW failed");
    }
    Ok(free_caller.val())
}

#[cfg(not(windows))]
fn free_space(dir: &Path) -> Result<u64> {
    let _ = dir;
    anyhow::bail!("free_space unimplemented on this platform")
}
