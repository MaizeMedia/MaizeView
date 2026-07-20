//! Batch StashDB identify across multiple scenes.

use chrono::{Duration, Utc};
use tauri::{AppHandle, Emitter, State};

use crate::{
    commands::identify::{apply_stashdb_match_inner, identify_scene_inner, ApplyStashDbMatchInput},
    AppState,
};

pub const PROGRESS_EVENT: &str = "identify://progress";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BatchIdentifyProgress {
    pub done: u64,
    pub total: u64,
    pub skipped: u64,
    pub scene_id: Option<String>,
    pub matched: u64,
    pub applied: u64,
    /// Matched but not auto-applied (0 matches auto-apply off, or 2+ candidates).
    pub needs_review: u64,
    pub errors: u64,
    pub finished: bool,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BatchIdentifyLibraryOptions {
    pub auto_apply: bool,
    /// Skip scenes checked within this many days. Ignored when `force_rescan` is true.
    pub skip_within_days: u32,
    /// Re-identify even if checked recently.
    pub force_rescan: bool,
}

impl Default for BatchIdentifyLibraryOptions {
    fn default() -> Self {
        Self {
            auto_apply: true,
            skip_within_days: 30,
            force_rescan: false,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct StashDbIdentifyStats {
    pub total_scenes: u64,
    pub never_checked: u64,
    pub checked_recently: u64,
    pub pending: u64,
    /// Checked with 2+ matches and never applied — open scene detail to pick.
    pub needs_review: u64,
}

fn cutoff_rfc3339(within_days: u32) -> Option<String> {
    if within_days == 0 {
        return None;
    }
    Some((Utc::now() - Duration::days(within_days as i64)).to_rfc3339())
}

/// The canonical needs-review predicate, shared by the stats count
/// (batch_identify) and the library list filter (scenes.rs). Deliberately
/// has NO has-file clause: a scene needing review still needs it while its
/// drive is offline, and the filtered list shows those scenes too — the
/// counts must agree. (HAS_FILE stays on the job-selection queries, since
/// identify can only run on files that exist.)
pub(crate) const NEEDS_REVIEW_WHERE: &str =
    "stashdb_match_count > 1 AND stashdb_applied_at IS NULL AND stashdb_ignored_at IS NULL";

/// Scenes eligible for a library-wide identify run (must still have a file).
pub(crate) async fn select_library_identify_scene_ids(
    pool: &sqlx::SqlitePool,
    skip_within_days: u32,
    force_rescan: bool,
) -> Result<Vec<String>, String> {
    const HAS_FILE: &str = "EXISTS (SELECT 1 FROM files f WHERE f.scene_id = scenes.id)";

    // Always skip scenes the user marked as ignore (false-positive / do-not-identify).
    const NOT_IGNORED: &str = "stashdb_ignored_at IS NULL";

    if force_rescan {
        return sqlx::query_scalar(&format!(
            "SELECT id FROM scenes WHERE {HAS_FILE} AND {NOT_IGNORED} ORDER BY created_at DESC"
        ))
        .fetch_all(pool)
        .await
        .map_err(crate::commands::err);
    }

    let cutoff = cutoff_rfc3339(skip_within_days);
    if let Some(cutoff) = cutoff {
        sqlx::query_scalar(&format!(
            r#"
            SELECT id FROM scenes
            WHERE {HAS_FILE}
              AND {NOT_IGNORED}
              AND (stashdb_checked_at IS NULL OR stashdb_checked_at < ?)
            ORDER BY created_at DESC
            "#
        ))
        .bind(cutoff)
        .fetch_all(pool)
        .await
        .map_err(crate::commands::err)
    } else {
        sqlx::query_scalar(&format!(
            "SELECT id FROM scenes WHERE {HAS_FILE} AND {NOT_IGNORED} ORDER BY created_at DESC"
        ))
        .fetch_all(pool)
        .await
        .map_err(crate::commands::err)
    }
}

pub(crate) async fn stashdb_identify_stats_inner(
    pool: &sqlx::SqlitePool,
    skip_within_days: u32,
    force_rescan: bool,
) -> Result<StashDbIdentifyStats, String> {
    const HAS_FILE: &str = "EXISTS (SELECT 1 FROM files f WHERE f.scene_id = scenes.id)";

    let total_scenes: i64 =
        sqlx::query_scalar(&format!("SELECT COUNT(*) FROM scenes WHERE {HAS_FILE}"))
            .fetch_one(pool)
            .await
            .map_err(crate::commands::err)?;

    let never_checked: i64 = sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM scenes WHERE {HAS_FILE} AND stashdb_checked_at IS NULL"
    ))
    .fetch_one(pool)
    .await
    .map_err(crate::commands::err)?;

    let checked_recently = if force_rescan {
        0
    } else if let Some(cutoff) = cutoff_rfc3339(skip_within_days) {
        sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM scenes WHERE {HAS_FILE} AND stashdb_checked_at IS NOT NULL AND stashdb_checked_at >= ?"
        ))
        .bind(cutoff)
        .fetch_one(pool)
        .await
        .map_err(crate::commands::err)?
    } else {
        0
    };

    let pending_ids =
        select_library_identify_scene_ids(pool, skip_within_days, force_rescan).await?;

    let needs_review: i64 = sqlx::query_scalar(&format!(
        r#"
        SELECT COUNT(*) FROM scenes
        WHERE {NEEDS_REVIEW_WHERE}
        "#
    ))
    .fetch_one(pool)
    .await
    .map_err(crate::commands::err)?;

    Ok(StashDbIdentifyStats {
        total_scenes: total_scenes.max(0) as u64,
        never_checked: never_checked.max(0) as u64,
        checked_recently: checked_recently.max(0) as u64,
        pending: pending_ids.len() as u64,
        needs_review: needs_review.max(0) as u64,
    })
}

fn start_batch_identify(
    app: AppHandle,
    pool: sqlx::SqlitePool,
    scene_ids: Vec<String>,
    skipped: u64,
    auto_apply: bool,
) {
    tauri::async_runtime::spawn(async move {
        let total = scene_ids.len() as u64;
        let mut done = 0u64;
        let mut matched = 0u64;
        let mut applied = 0u64;
        let mut needs_review = 0u64;
        let mut errors = 0u64;
        let mut last_error = None;

        let emit = |p: BatchIdentifyProgress| {
            let _ = app.emit(PROGRESS_EVENT, &p);
        };

        emit(BatchIdentifyProgress {
            done: 0,
            total,
            skipped,
            scene_id: None,
            matched: 0,
            applied: 0,
            needs_review: 0,
            errors: 0,
            finished: false,
            last_error: None,
        });

        for scene_id in &scene_ids {
            emit(BatchIdentifyProgress {
                done,
                total,
                skipped,
                scene_id: Some(scene_id.clone()),
                matched,
                applied,
                needs_review,
                errors,
                finished: false,
                last_error: last_error.clone(),
            });

            match identify_scene_inner(&pool, scene_id, Default::default()).await {
                Ok(result) => {
                    use crate::commands::identify::duration_plausible_for_auto_apply;

                    let rejected: std::collections::HashSet<&str> = result
                        .rejected_remote_ids
                        .iter()
                        .map(|s| s.as_str())
                        .collect();
                    let auto_candidates: Vec<_> = result
                        .matches
                        .iter()
                        .filter(|m| !rejected.contains(m.id.as_str()))
                        .filter(|m| {
                            duration_plausible_for_auto_apply(
                                result.fingerprints.duration_secs,
                                m.duration,
                            )
                        })
                        .cloned()
                        .collect();

                    if !result.matches.is_empty() {
                        matched += 1;
                    }
                    if auto_apply && auto_candidates.len() == 1 {
                        let m = auto_candidates[0].clone();
                        let provider = result.provider_id.clone();
                        if apply_stashdb_match_inner(
                            &pool,
                            scene_id,
                            m,
                            &ApplyStashDbMatchInput {
                                title: true,
                                details: true,
                                studio: true,
                                performers: true,
                                tags: true,
                                cover: true,
                            },
                            Some(&provider),
                        )
                        .await
                        .is_ok()
                        {
                            applied += 1;
                        } else {
                            errors += 1;
                            last_error = Some(format!("apply failed for scene {scene_id}"));
                        }
                    } else if auto_candidates.len() > 1
                        || (!auto_apply && !auto_candidates.is_empty())
                        || (auto_apply && auto_candidates.is_empty() && result.matches.len() > 1)
                    {
                        needs_review += 1;
                    }
                }
                Err(e) => {
                    errors += 1;
                    last_error = Some(e);
                }
            }

            done += 1;
        }

        emit(BatchIdentifyProgress {
            done,
            total,
            skipped,
            scene_id: None,
            matched,
            applied,
            needs_review,
            errors,
            finished: true,
            last_error,
        });
    });
}

async fn batch_identify_ids(
    app: AppHandle,
    pool: sqlx::SqlitePool,
    scene_ids: Vec<String>,
    skipped: u64,
    auto_apply: bool,
) -> Result<(), String> {
    if scene_ids.is_empty() {
        return Err("no scenes to identify".into());
    }

    let targets = crate::commands::settings::stash_box_query_targets(&pool).await?;
    if targets.is_empty() {
        return Err("No stash-box API key configured — add one in Settings".into());
    }

    start_batch_identify(app, pool, scene_ids, skipped, auto_apply);
    Ok(())
}

/// When `auto_apply` is true, auto-apply metadata when exactly one StashDB match is returned.
#[tauri::command]
pub async fn batch_identify_scenes(
    app: AppHandle,
    state: State<'_, AppState>,
    scene_ids: Vec<String>,
    auto_apply: bool,
) -> Result<(), String> {
    batch_identify_ids(app, state.pool.clone(), scene_ids, 0, auto_apply).await
}

/// Identify library scenes with skip-recent / force-rescan options.
#[tauri::command]
pub async fn batch_identify_library(
    app: AppHandle,
    state: State<'_, AppState>,
    options: BatchIdentifyLibraryOptions,
) -> Result<StashDbIdentifyStats, String> {
    let stats =
        stashdb_identify_stats_inner(&state.pool, options.skip_within_days, options.force_rescan)
            .await?;

    let scene_ids = select_library_identify_scene_ids(
        &state.pool,
        options.skip_within_days,
        options.force_rescan,
    )
    .await?;

    if scene_ids.is_empty() {
        return Ok(stats);
    }

    batch_identify_ids(
        app,
        state.pool.clone(),
        scene_ids,
        stats.checked_recently,
        options.auto_apply,
    )
    .await?;

    Ok(stats)
}

/// Count scenes that would run vs be skipped for the given library identify options.
#[tauri::command]
pub async fn stashdb_identify_stats(
    state: State<'_, AppState>,
    skip_within_days: u32,
    force_rescan: bool,
) -> Result<StashDbIdentifyStats, String> {
    stashdb_identify_stats_inner(&state.pool, skip_within_days, force_rescan).await
}
