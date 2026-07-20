//! StashDB scene identify + apply (Phase 2).

use std::path::PathBuf;

use sqlx::SqlitePool;
use tauri::State;

use crate::{
    commands::{err, settings::active_stash_box_id},
    fingerprints,
    models::{new_id, now},
    scanner::{md5, phash},
    stashdb::{self, FingerprintAlgorithm, FingerprintQueryInput, StashDbSceneMatch},
    AppState,
};

#[derive(Debug, Clone, serde::Serialize)]
pub struct SceneFingerprintStatus {
    pub file_id: String,
    pub oshash: Option<String>,
    pub md5: Option<String>,
    pub phash: Option<String>,
    pub duration_secs: Option<f64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct IdentifySceneResult {
    pub fingerprints: SceneFingerprintStatus,
    pub matches: Vec<StashDbSceneMatch>,
    pub md5_computed: bool,
    pub phash_computed: bool,
    pub title_search_used: bool,
    pub title_search_term: Option<String>,
    /// Set when fingerprints missed and the title/stem was too weak to text-search.
    pub title_search_skipped_reason: Option<String>,
    /// Active stash-box preset id used for this query (`stashdb`, `tpdb`, …).
    pub provider_id: String,
    pub provider_name: String,
    /// Remote scene ids previously rejected for this local scene + provider.
    pub rejected_remote_ids: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ApplyStashDbMatchInput {
    pub title: bool,
    pub details: bool,
    pub studio: bool,
    pub performers: bool,
    pub tags: bool,
    pub cover: bool,
}

/// Pick a title-search term, skipping weak numeric/id-like candidates.
/// Tries DB title first, then filename stem (so a bad backfilled `876` title
/// can still fall through to a richer stem when present).
/// Returns `(Some(term), None)` or `(None, Some(skip_reason))`.
async fn search_term_for_scene(
    pool: &SqlitePool,
    scene_id: &str,
    path: &std::path::Path,
) -> (Option<String>, Option<String>) {
    let title: Option<String> = sqlx::query_as("SELECT title FROM scenes WHERE id = ?")
        .bind(scene_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .map(|(t,)| t);

    let mut candidates: Vec<String> = Vec::new();
    if let Some(t) = title {
        let trimmed = t.trim();
        if !trimmed.is_empty() {
            candidates.push(trimmed.to_string());
        }
    }
    if let Some(stem) = path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        if candidates.iter().all(|c| c != &stem) {
            candidates.push(stem);
        }
    }

    let mut last_weak: Option<String> = None;
    for c in &candidates {
        if let Some(term) = crate::title_search::usable_title_search_term(c) {
            return (Some(term), None);
        }
        last_weak = Some(crate::title_search::weak_title_reason(c).to_string());
    }
    (None, last_weak)
}

async fn load_primary_file_fingerprints(
    pool: &SqlitePool,
    scene_id: &str,
) -> Result<
    (
        String,
        PathBuf,
        Option<f64>,
        Option<String>,
        Option<String>,
        Option<String>,
    ),
    String,
> {
    let row: Option<(String, String, Option<f64>)> = sqlx::query_as(
        r#"
        SELECT id, path, duration
        FROM files
        WHERE scene_id = ?
        ORDER BY duration DESC NULLS LAST, scanned_at ASC
        LIMIT 1
        "#,
    )
    .bind(scene_id)
    .fetch_optional(pool)
    .await
    .map_err(err)?;

    let (file_id, path, duration) = row.ok_or_else(|| "scene has no files".to_string())?;

    let fps: Vec<(String, String)> =
        sqlx::query_as("SELECT hash_type, value FROM fingerprints WHERE file_id = ?")
            .bind(&file_id)
            .fetch_all(pool)
            .await
            .map_err(err)?;

    let mut oshash = None;
    let mut md5 = None;
    let mut phash_val = None;
    for (t, v) in fps {
        match t.as_str() {
            "oshash" => oshash = Some(v),
            "md5" => md5 = Some(v),
            "phash" => phash_val = Some(v),
            _ => {}
        }
    }

    Ok((
        file_id,
        PathBuf::from(path),
        duration,
        oshash,
        md5,
        phash_val,
    ))
}

/// Options for a single-scene identify pass.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentifySceneOpts {
    /// Override the auto-derived title-search term (also used for title-only).
    pub title_term: Option<String>,
    /// Skip fingerprint API queries; search by title only (user-edited term).
    #[serde(default)]
    pub title_only: bool,
}

/// Core identify logic (shared by single-scene + batch commands).
/// Fingerprint queries follow the stash-box waterfall (active first, then other
/// keyed boxes when enabled). Title search runs only if fingerprints miss
/// (unless `title_only`).
pub(crate) async fn identify_scene_inner(
    pool: &SqlitePool,
    scene_id: &str,
    opts: IdentifySceneOpts,
) -> Result<IdentifySceneResult, String> {
    let (file_id, path, duration, oshash, mut md5, mut phash_val) =
        load_primary_file_fingerprints(pool, scene_id).await?;

    let title_only = opts.title_only;
    let override_term = opts
        .title_term
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    if title_only && override_term.is_none() {
        return Err("Enter a search term".to_string());
    }

    let mut md5_computed = false;
    let mut phash_computed = false;

    // Title-only re-search reuses stored fingerprints (no recompute / no fp API).
    if !title_only {
        if md5.is_none() {
            let path_clone = path.clone();
            let computed = tokio::task::spawn_blocking(move || md5::hash_file(&path_clone))
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())?;
            fingerprints::upsert(pool, &file_id, "md5", &computed)
                .await
                .map_err(err)?;
            md5 = Some(computed);
            md5_computed = true;
        }

        if phash_val.is_none() {
            // Reuse pHash from another file with the same oshash before spending ffmpeg.
            if let Some(ref osh) = oshash {
                if let Ok(Some(existing)) = fingerprints::find_phash_by_oshash(pool, osh).await {
                    if fingerprints::upsert(pool, &file_id, "phash", &existing)
                        .await
                        .is_ok()
                    {
                        phash_val = Some(existing);
                    }
                }
            }
        }
        if phash_val.is_none() {
            if let Some(dur) = duration.filter(|d| *d > 0.0) {
                let path_clone = path.clone();
                let file_id_clone = file_id.clone();
                match tokio::task::spawn_blocking(move || phash::hash_file(&path_clone, dur)).await
                {
                    Ok(Ok(computed)) => {
                        if let Err(e) =
                            fingerprints::upsert(pool, &file_id_clone, "phash", &computed).await
                        {
                            tracing::warn!(error = %e, scene_id, "storing phash failed");
                        } else {
                            phash_val = Some(computed);
                            phash_computed = true;
                        }
                    }
                    Ok(Err(e)) => {
                        tracing::warn!(error = %e, scene_id, "phash computation failed; using oshash/md5 only");
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, scene_id, "phash task join failed");
                    }
                }
            }
        }
    }

    let targets = crate::commands::settings::stash_box_query_targets(pool).await?;
    if targets.is_empty() {
        return Err("No stash-box API key configured — add one in Settings".to_string());
    }

    let mut matches = Vec::new();
    let mut provider_id = targets[0].0.clone();
    let mut title_search_endpoint = targets[0].1.clone();
    let mut title_search_key = targets[0].2.clone();

    if !title_only {
        let mut fps = Vec::new();
        if let Some(h) = oshash.as_ref() {
            fps.push(FingerprintQueryInput {
                algorithm: FingerprintAlgorithm::Oshash,
                hash: h.clone(),
            });
        }
        if let Some(h) = md5.as_ref() {
            fps.push(FingerprintQueryInput {
                algorithm: FingerprintAlgorithm::Md5,
                hash: h.clone(),
            });
        }
        if let Some(h) = phash_val.as_ref() {
            fps.push(FingerprintQueryInput {
                algorithm: FingerprintAlgorithm::Phash,
                hash: h.clone(),
            });
        }

        if fps.is_empty() {
            return Err("no fingerprints available for this file".to_string());
        }

        for (id, endpoint, api_key) in &targets {
            match stashdb::find_scenes_by_fingerprints(endpoint, api_key, fps.clone()).await {
                Ok(found) if !found.is_empty() => {
                    matches = found;
                    provider_id = id.clone();
                    title_search_endpoint = endpoint.clone();
                    title_search_key = api_key.clone();
                    break;
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(provider = %id, error = %e, "fingerprint query failed; trying next box");
                }
            }
        }
    }

    let mut title_search_used = false;
    let mut title_search_term = None;
    let mut title_search_skipped_reason = None;
    if matches.is_empty() {
        let term = if let Some(t) = override_term {
            Some(t)
        } else {
            let (auto, skip_reason) = search_term_for_scene(pool, scene_id, &path).await;
            if auto.is_none() {
                title_search_skipped_reason = skip_reason;
            }
            auto
        };

        if let Some(term) = term {
            title_search_term = Some(term.clone());
            match stashdb::search_scenes_by_term(
                &title_search_endpoint,
                &title_search_key,
                &term,
                15,
            )
            .await
            {
                Ok(found) => {
                    matches = found;
                    // Term was used even if zero hits (UI shows editable box).
                    title_search_used = true;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "title search failed");
                    title_search_used = true;
                }
            }
        } else if title_search_skipped_reason.is_some() {
            tracing::debug!(
                scene_id,
                reason = title_search_skipped_reason.as_deref().unwrap_or(""),
                "skipped stash-box title search — term too weak"
            );
        }
    }

    let rejected_remote_ids = load_rejected_remote_ids(pool, scene_id, &provider_id).await?;
    let match_count = matches.len();
    record_stashdb_check(pool, scene_id, match_count).await?;

    let provider_name = stashdb::preset_by_id(&provider_id)
        .map(|p| p.name.to_string())
        .unwrap_or_else(|| provider_id.clone());

    Ok(IdentifySceneResult {
        fingerprints: SceneFingerprintStatus {
            file_id,
            oshash,
            md5,
            phash: phash_val,
            duration_secs: duration,
        },
        matches,
        md5_computed,
        phash_computed,
        title_search_used,
        title_search_term,
        title_search_skipped_reason,
        provider_id,
        provider_name,
        rejected_remote_ids,
    })
}

/// Record the outcome of a StashDB identify attempt on a scene.
pub(crate) async fn record_stashdb_check(
    pool: &SqlitePool,
    scene_id: &str,
    match_count: usize,
) -> Result<(), String> {
    let ts = now().to_rfc3339();
    sqlx::query(
        r#"
        UPDATE scenes
        SET stashdb_checked_at = ?, stashdb_match_count = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(&ts)
    .bind(match_count as i64)
    .bind(&ts)
    .bind(scene_id)
    .execute(pool)
    .await
    .map_err(err)?;
    Ok(())
}

async fn record_stashdb_applied(
    pool: &SqlitePool,
    scene_id: &str,
    remote_id: &str,
) -> Result<(), String> {
    let ts = now().to_rfc3339();
    sqlx::query(
        r#"
        UPDATE scenes
        SET stashdb_applied_at = ?, stashdb_remote_id = ?, stashdb_ignored_at = NULL, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(&ts)
    .bind(remote_id)
    .bind(&ts)
    .bind(scene_id)
    .execute(pool)
    .await
    .map_err(err)?;
    Ok(())
}

async fn load_rejected_remote_ids(
    pool: &SqlitePool,
    scene_id: &str,
    provider_id: &str,
) -> Result<Vec<String>, String> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT remote_id FROM stashdb_rejected_matches WHERE scene_id = ? AND provider_id = ?",
    )
    .bind(scene_id)
    .bind(provider_id)
    .fetch_all(pool)
    .await
    .map_err(err)?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

/// Whether local file duration and remote scene duration are close enough for auto-apply.
pub(crate) fn duration_plausible_for_auto_apply(
    local_secs: Option<f64>,
    remote_secs: Option<i64>,
) -> bool {
    let (Some(local), Some(remote)) = (local_secs, remote_secs) else {
        return true;
    };
    if local <= 0.0 || remote <= 0 {
        return true;
    }
    let remote = remote as f64;
    let diff = (local - remote).abs();
    if diff <= 45.0 {
        return true;
    }
    let ratio = if local > remote {
        local / remote
    } else {
        remote / local
    };
    ratio <= 1.2
}

fn is_identify_provenance(source: &str) -> bool {
    let s = source.trim();
    if s.is_empty() || s == "manual" || s == "filename" || s == "embedded" {
        return false;
    }
    stashdb::preset_by_id(s).is_some()
}

#[cfg(test)]
mod duration_tests {
    use super::duration_plausible_for_auto_apply;

    #[test]
    fn rejects_large_duration_mismatch() {
        // ~43 min local vs ~2h remote (the Chanta-Rose false positive class)
        assert!(!duration_plausible_for_auto_apply(
            Some(43.0 * 60.0),
            Some(122 * 60)
        ));
    }

    #[test]
    fn accepts_close_durations() {
        assert!(duration_plausible_for_auto_apply(Some(600.0), Some(610)));
        assert!(duration_plausible_for_auto_apply(None, Some(100)));
    }
}

#[tauri::command]
pub async fn identify_scene(
    state: State<'_, AppState>,
    scene_id: String,
    opts: Option<IdentifySceneOpts>,
) -> Result<IdentifySceneResult, String> {
    identify_scene_inner(&state.pool, &scene_id, opts.unwrap_or_default()).await
}

#[tauri::command]
pub async fn apply_stashdb_match(
    state: State<'_, AppState>,
    scene_id: String,
    stashdb_scene: StashDbSceneMatch,
    fields: ApplyStashDbMatchInput,
    provider_id: Option<String>,
) -> Result<(), String> {
    apply_stashdb_match_inner(
        &state.pool,
        &scene_id,
        stashdb_scene,
        &fields,
        provider_id.as_deref(),
    )
    .await
}

pub(crate) async fn apply_stashdb_match_inner(
    pool: &SqlitePool,
    scene_id: &str,
    stashdb_scene: StashDbSceneMatch,
    fields: &ApplyStashDbMatchInput,
    provider_id: Option<&str>,
) -> Result<(), String> {
    let ts = now().to_rfc3339();
    let source = match provider_id.map(str::trim).filter(|s| !s.is_empty()) {
        Some(id) => id.to_string(),
        None => active_stash_box_id(pool).await,
    };

    if fields.title {
        let title = stashdb_scene
            .title
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        sqlx::query("UPDATE scenes SET title = ?, title_source = ?, updated_at = ? WHERE id = ?")
            .bind(&title)
            .bind(&source)
            .bind(&ts)
            .bind(scene_id)
            .execute(pool)
            .await
            .map_err(err)?;
    }

    if fields.details {
        let details = stashdb_scene
            .details
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        sqlx::query(
            "UPDATE scenes SET details = ?, details_source = ?, updated_at = ? WHERE id = ?",
        )
        .bind(&details)
        .bind(&source)
        .bind(&ts)
        .bind(scene_id)
        .execute(pool)
        .await
        .map_err(err)?;
    }

    if fields.studio {
        if let Some(studio) = stashdb_scene.studio.as_ref() {
            let name = studio.name.trim();
            if !name.is_empty() {
                let studio_id = find_or_create_studio(pool, name).await?;
                sqlx::query("UPDATE scenes SET studio_id = ?, updated_at = ? WHERE id = ?")
                    .bind(&studio_id)
                    .bind(&ts)
                    .bind(scene_id)
                    .execute(pool)
                    .await
                    .map_err(err)?;
            }
        }
    }

    if fields.performers {
        if let Some(appearances) = stashdb_scene.performers.as_ref() {
            for ap in appearances {
                let name = ap.performer.name.trim();
                if name.is_empty() {
                    continue;
                }
                let performer_id = find_or_create_performer(pool, name).await?;
                sqlx::query(
                    "INSERT OR IGNORE INTO scene_performers (scene_id, performer_id) VALUES (?, ?)",
                )
                .bind(scene_id)
                .bind(&performer_id)
                .execute(pool)
                .await
                .map_err(err)?;
            }
        }
    }

    if fields.tags {
        if let Some(tags) = stashdb_scene.tags.as_ref() {
            for tag in tags {
                let name = tag.name.trim();
                if name.is_empty() {
                    continue;
                }
                let tag_id = find_or_create_tag(pool, name).await?;
                sqlx::query("INSERT OR IGNORE INTO scene_tags (scene_id, tag_id) VALUES (?, ?)")
                    .bind(scene_id)
                    .bind(&tag_id)
                    .execute(pool)
                    .await
                    .map_err(err)?;
            }
        }
    }

    if fields.cover {
        if let Some(url) = stashdb::best_cover_url(&stashdb_scene) {
            match crate::covers::download_cover(scene_id, &url).await {
                Ok(path) => {
                    sqlx::query(
                        "UPDATE scenes SET cover_path = ?, cover_source = ?, updated_at = ? WHERE id = ?",
                    )
                    .bind(&path)
                    .bind(&source)
                    .bind(&ts)
                    .bind(scene_id)
                    .execute(pool)
                    .await
                    .map_err(err)?;
                }
                Err(e) => {
                    tracing::warn!(scene_id, error = %e, "StashDB cover download failed");
                }
            }
        }
    }

    record_stashdb_applied(pool, scene_id, &stashdb_scene.id).await?;
    Ok(())
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ClearStashDbIdentifyInput {
    /// Exclude from future batch identify (default true).
    #[serde(default = "default_true")]
    pub ignore_future: bool,
    /// Clear title/details/cover/studio when provenance is a stash-box provider.
    #[serde(default = "default_true")]
    pub clear_metadata: bool,
    /// Also reject this remote id so it won't auto-apply again.
    pub reject_remote_id: Option<String>,
    pub provider_id: Option<String>,
}

fn default_true() -> bool {
    true
}

/// Unlink a false-positive identify apply; optionally ignore scene + reject remote id.
#[tauri::command]
pub async fn clear_stashdb_identify(
    state: State<'_, AppState>,
    scene_id: String,
    opts: Option<ClearStashDbIdentifyInput>,
) -> Result<(), String> {
    let opts = opts.unwrap_or(ClearStashDbIdentifyInput {
        ignore_future: true,
        clear_metadata: true,
        reject_remote_id: None,
        provider_id: None,
    });
    clear_stashdb_identify_inner(&state.pool, &scene_id, &opts).await
}

pub(crate) async fn clear_stashdb_identify_inner(
    pool: &SqlitePool,
    scene_id: &str,
    opts: &ClearStashDbIdentifyInput,
) -> Result<(), String> {
    let ts = now().to_rfc3339();

    // Capture applied remote id before clearing (for reject).
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT stashdb_remote_id FROM scenes WHERE id = ?")
            .bind(scene_id)
            .fetch_optional(pool)
            .await
            .map_err(err)?;
    let applied_remote = row.and_then(|(id,)| id);

    if opts.clear_metadata {
        let meta: Option<(String, String, String, Option<String>)> = sqlx::query_as(
            "SELECT title_source, details_source, cover_source, cover_path FROM scenes WHERE id = ?",
        )
        .bind(scene_id)
        .fetch_optional(pool)
        .await
        .map_err(err)?;

        if let Some((title_source, details_source, cover_source, _cover)) = meta {
            if is_identify_provenance(&title_source) {
                let path: Option<String> = sqlx::query_scalar(
                    r#"
                    SELECT path FROM files
                    WHERE scene_id = ?
                    ORDER BY duration DESC NULLS LAST, scanned_at ASC
                    LIMIT 1
                    "#,
                )
                .bind(scene_id)
                .fetch_optional(pool)
                .await
                .map_err(err)?;
                let new_title = path.as_deref().and_then(|p| {
                    crate::filename_parse::scene_title_from_path(std::path::Path::new(p))
                });
                let source = if new_title.is_some() {
                    "filename"
                } else {
                    "manual"
                };
                sqlx::query(
                    "UPDATE scenes SET title = ?, title_source = ?, updated_at = ? WHERE id = ?",
                )
                .bind(&new_title)
                .bind(source)
                .bind(&ts)
                .bind(scene_id)
                .execute(pool)
                .await
                .map_err(err)?;
            }
            if is_identify_provenance(&details_source) {
                sqlx::query(
                    "UPDATE scenes SET details = NULL, details_source = 'manual', updated_at = ? WHERE id = ?",
                )
                .bind(&ts)
                .bind(scene_id)
                .execute(pool)
                .await
                .map_err(err)?;
            }
            if is_identify_provenance(&cover_source) {
                sqlx::query(
                    "UPDATE scenes SET cover_path = NULL, cover_source = 'manual', updated_at = ? WHERE id = ?",
                )
                .bind(&ts)
                .bind(scene_id)
                .execute(pool)
                .await
                .map_err(err)?;
            }
            // Studio was typically set by identify without a separate provenance column.
            sqlx::query("UPDATE scenes SET studio_id = NULL, updated_at = ? WHERE id = ?")
                .bind(&ts)
                .bind(scene_id)
                .execute(pool)
                .await
                .map_err(err)?;
        }
    }

    let ignore_at = if opts.ignore_future {
        Some(ts.clone())
    } else {
        None
    };

    sqlx::query(
        r#"
        UPDATE scenes
        SET stashdb_applied_at = NULL,
            stashdb_remote_id = NULL,
            stashdb_ignored_at = ?,
            stashdb_match_count = 0,
            updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(&ignore_at)
    .bind(&ts)
    .bind(scene_id)
    .execute(pool)
    .await
    .map_err(err)?;

    let reject_id = opts
        .reject_remote_id
        .clone()
        .or(applied_remote)
        .filter(|s| !s.is_empty());
    if let Some(remote_id) = reject_id {
        let provider = match opts
            .provider_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(id) => id.to_string(),
            None => active_stash_box_id(pool).await,
        };
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO stashdb_rejected_matches (scene_id, provider_id, remote_id, rejected_at)
            VALUES (?, ?, ?, ?)
            "#,
        )
        .bind(scene_id)
        .bind(&provider)
        .bind(&remote_id)
        .bind(&ts)
        .execute(pool)
        .await
        .map_err(err)?;
    }

    Ok(())
}

/// Allow batch identify again (clears ignore flag only).
#[tauri::command]
pub async fn clear_stashdb_ignore(
    state: State<'_, AppState>,
    scene_id: String,
) -> Result<(), String> {
    let ts = now().to_rfc3339();
    sqlx::query("UPDATE scenes SET stashdb_ignored_at = NULL, updated_at = ? WHERE id = ?")
        .bind(&ts)
        .bind(&scene_id)
        .execute(&state.pool)
        .await
        .map_err(err)?;
    Ok(())
}

/// Reject a candidate without applying (blocks future auto-apply of this remote id).
#[tauri::command]
pub async fn reject_stashdb_match(
    state: State<'_, AppState>,
    scene_id: String,
    remote_id: String,
    provider_id: Option<String>,
) -> Result<(), String> {
    let provider = match provider_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(id) => id.to_string(),
        None => active_stash_box_id(&state.pool).await,
    };
    let ts = now().to_rfc3339();
    sqlx::query(
        r#"
        INSERT OR REPLACE INTO stashdb_rejected_matches (scene_id, provider_id, remote_id, rejected_at)
        VALUES (?, ?, ?, ?)
        "#,
    )
    .bind(&scene_id)
    .bind(&provider)
    .bind(&remote_id)
    .bind(&ts)
    .execute(&state.pool)
    .await
    .map_err(err)?;
    Ok(())
}

/// User reviewed candidates and chose none — reject listed remotes, clear needs-review, skip batch.
#[tauri::command]
pub async fn dismiss_stashdb_review(
    state: State<'_, AppState>,
    scene_id: String,
    remote_ids: Vec<String>,
    provider_id: Option<String>,
) -> Result<(), String> {
    let provider = match provider_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(id) => id.to_string(),
        None => active_stash_box_id(&state.pool).await,
    };
    let ts = now().to_rfc3339();

    for remote_id in remote_ids
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO stashdb_rejected_matches (scene_id, provider_id, remote_id, rejected_at)
            VALUES (?, ?, ?, ?)
            "#,
        )
        .bind(&scene_id)
        .bind(&provider)
        .bind(&remote_id)
        .bind(&ts)
        .execute(&state.pool)
        .await
        .map_err(err)?;
    }

    sqlx::query(
        r#"
        UPDATE scenes
        SET stashdb_match_count = 0,
            stashdb_ignored_at = ?,
            stashdb_applied_at = NULL,
            stashdb_remote_id = NULL,
            stashdb_checked_at = ?,
            updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(&ts)
    .bind(&ts)
    .bind(&ts)
    .bind(&scene_id)
    .execute(&state.pool)
    .await
    .map_err(err)?;

    Ok(())
}

async fn find_or_create_studio(pool: &SqlitePool, name: &str) -> Result<String, String> {
    let existing: Option<(String,)> =
        sqlx::query_as("SELECT id FROM studios WHERE name = ? COLLATE NOCASE")
            .bind(name)
            .fetch_optional(pool)
            .await
            .map_err(err)?;
    if let Some((id,)) = existing {
        return Ok(id);
    }
    let id = new_id();
    sqlx::query("INSERT INTO studios (id, name, created_at) VALUES (?, ?, ?)")
        .bind(&id)
        .bind(name)
        .bind(now().to_rfc3339())
        .execute(pool)
        .await
        .map_err(err)?;
    Ok(id)
}

async fn find_or_create_performer(pool: &SqlitePool, name: &str) -> Result<String, String> {
    let existing: Option<(String,)> =
        sqlx::query_as("SELECT id FROM performers WHERE name = ? COLLATE NOCASE")
            .bind(name)
            .fetch_optional(pool)
            .await
            .map_err(err)?;
    if let Some((id,)) = existing {
        return Ok(id);
    }
    let id = new_id();
    sqlx::query("INSERT INTO performers (id, name, created_at) VALUES (?, ?, ?)")
        .bind(&id)
        .bind(name)
        .bind(now().to_rfc3339())
        .execute(pool)
        .await
        .map_err(err)?;
    Ok(id)
}

async fn find_or_create_tag(pool: &SqlitePool, name: &str) -> Result<String, String> {
    let existing: Option<(String,)> =
        sqlx::query_as("SELECT id FROM tags WHERE name = ? COLLATE NOCASE")
            .bind(name)
            .fetch_optional(pool)
            .await
            .map_err(err)?;
    if let Some((id,)) = existing {
        return Ok(id);
    }
    let id = new_id();
    sqlx::query("INSERT INTO tags (id, name, created_at) VALUES (?, ?, ?)")
        .bind(&id)
        .bind(name)
        .bind(now().to_rfc3339())
        .execute(pool)
        .await
        .map_err(err)?;
    Ok(id)
}
