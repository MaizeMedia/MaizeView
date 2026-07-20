//! MaizeView backend entry point.
//!
//! Wires up: tracing, SQLite pool (with migrations), the library scanner, and
//! Tauri commands exposed to the frontend.

pub mod catalog_cleanup;
pub mod commands;
pub mod covers;
pub mod db;
pub mod filename_parse;
pub mod fingerprints;
pub mod fingerprints_job;
pub mod job_parallel;
pub mod media_tools;
pub mod models;
pub mod path_meta;
pub mod paths;
pub mod phash_job;
pub mod previews;
pub mod previews_job;
pub mod scanner;
pub mod stashdb;
pub mod title_search;
pub mod transcode_job;
pub mod transcode_tokens;

use sqlx::SqlitePool;
use tauri::Manager;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::{fmt, EnvFilter};

/// Everything the Tauri commands need, kept in app state.
#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    /// Cancellation token for the currently-running scan, if any.
    /// `None` means no scan is running. Replaced each time a scan starts.
    pub scan_cancel: std::sync::Arc<tokio::sync::Mutex<Option<CancellationToken>>>,
    /// Cancellation token for the currently-running transcode (downscale)
    /// job, if any. `None` means none is running. Replaced each time a run
    /// starts; cleared on completion/error/cancel. Shared as `Arc` so the
    /// job holds one ref and `downscale_cancel` holds another.
    pub transcode_cancel:
        std::sync::Arc<tokio::sync::Mutex<Option<std::sync::Arc<CancellationToken>>>>,
    /// Cancellable preview / MD5 / pHash background jobs (Settings + post-scan).
    pub preview_cancel: crate::job_parallel::JobCancelSlot,
    pub md5_cancel: crate::job_parallel::JobCancelSlot,
    pub phash_cancel: crate::job_parallel::JobCancelSlot,
    /// Cross-window queue handoff for player windows (ADR-011). The catalog
    /// stages a queue keyed by player-window label; the player claims it on
    /// mount. Not Clone-derived — it's an Arc internally via the Mutex<HashMap>.
    pub player_stash: std::sync::Arc<commands::player::PlayerStash>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_tracing();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        // libmpv host (Phase 3 playback, ADR-012). The plugin manages one mpv
        // instance per window label internally; the JS `init` command creates
        // an instance scoped to the current player window. Window close →
        // automatic mpv destroy is handled by the plugin.
        .plugin(tauri_plugin_libmpv::init())
        .setup(|app| {
            // The DB pool must be created on Tauri's own async runtime so its
            // connections outlive setup().
            let pool =
                tauri::async_runtime::block_on(db::init_pool()).expect("initializing database");

            // One-time backfill: derive scene titles from filenames for scenes
            // that have none (e.g. created before titles were populated at scan
            // time). Cheap and idempotent — only touches NULL titles.
            let backfilled =
                tauri::async_runtime::block_on(commands::scenes::backfill_scene_titles_sqlx(&pool))
                    .unwrap_or(0);
            if backfilled > 0 {
                tracing::info!(backfilled, "backfilled scene titles from filenames");
            }

            tauri::async_runtime::block_on(commands::settings::apply_job_workers_from_db(&pool));
            tracing::info!(
                workers = crate::job_parallel::media_job_workers(),
                cap = crate::job_parallel::job_workers_cap(),
                "job intensity (scan / preview / pHash / MD5)"
            );

            app.manage(AppState {
                pool: pool.clone(),
                scan_cancel: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
                transcode_cancel: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
                preview_cancel: crate::job_parallel::new_cancel_slot(),
                md5_cancel: crate::job_parallel::new_cancel_slot(),
                phash_cancel: crate::job_parallel::new_cancel_slot(),
                player_stash: std::sync::Arc::new(commands::player::PlayerStash::default()),
            });

            // 4Play: quadrant child-HWND bookkeeping (commands/quad.rs).
            app.manage(commands::quad::QuadState::default());

            // Reconcile files deleted outside MaizeView in the background —
            // the per-file stat checks are slow on USB/spinning drives and
            // must not delay first paint. Errors are logged, never fatal.
            tauri::async_runtime::spawn(async move {
                if let Err(e) = crate::catalog_cleanup::reconcile_missing_files(&pool).await {
                    tracing::warn!(error = %e, "missing-file reconcile on startup failed");
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // scan paths
            commands::scan_paths::list_scan_paths,
            commands::scan_paths::add_scan_path,
            commands::scan_paths::remove_scan_path,
            // scenes / library
            commands::scenes::list_scenes,
            commands::scenes::scene_counts,
            commands::scenes::backfill_scene_titles,
            commands::scenes::set_favorite,
            commands::scenes::record_scene_play,
            commands::scenes::scenes_shuffle_meta,
            commands::scenes::scene_detail,
            commands::scenes::delete_scene,
            commands::scenes::delete_scenes,
            // scan
            commands::scan::start_scan,
            commands::scan::cancel_scan,
            // previews
            commands::previews::generate_previews,
            commands::previews::cancel_previews,
            // fingerprints
            commands::fingerprints::generate_md5_fingerprints,
            commands::fingerprints::cancel_md5_fingerprints,
            commands::fingerprints::generate_phash_fingerprints,
            commands::fingerprints::cancel_phash_fingerprints,
            commands::fingerprints::cancel_media_jobs,
            // duplicates
            commands::duplicates::find_duplicate_groups,
            commands::duplicates::resolve_duplicate_group,
            // metadata CRUD (tags/performers/studios + scene fields)
            commands::metadata::list_tags,
            commands::metadata::list_tags_with_counts,
            commands::metadata::create_tag,
            commands::metadata::delete_tag,
            commands::metadata::add_tag_to_scene,
            commands::metadata::remove_tag_from_scene,
            commands::metadata::list_performers,
            commands::metadata::create_performer,
            commands::metadata::delete_performer,
            commands::metadata::add_performer_to_scene,
            commands::metadata::remove_performer_from_scene,
            commands::metadata::list_studios,
            commands::metadata::create_studio,
            commands::metadata::set_scene_studio,
            commands::metadata::set_scene_title,
            commands::metadata::set_scene_details,
            // playlists
            commands::playlists::list_playlists,
            commands::playlists::create_playlist,
            commands::playlists::set_playlist_shuffle_default,
            commands::playlists::rename_playlist,
            commands::playlists::delete_playlist,
            commands::playlists::playlist_items,
            commands::playlists::add_to_playlist,
            commands::playlists::remove_from_playlist,
            commands::playlists::reorder_playlist,
            commands::playlists::shuffle_playlist,
            // saved filters
            commands::saved_filters::list_saved_filters,
            commands::saved_filters::create_saved_filter,
            commands::saved_filters::delete_saved_filter,
            commands::saved_filters::rename_saved_filter,
            // player (cross-window queue handoff + scene→file resolution)
            commands::player::stage_player_queue,
            commands::player::claim_player_queue,
            commands::player::scene_file_path,
            commands::player::scene_scrub_preview,
            commands::player::close_all_player_windows,
            // 4Play: quadrant child HWNDs + per-instance mpv claims
            commands::quad::quad_create_panes,
            commands::quad::quad_relayout,
            commands::quad::quad_claim_mpv,
            // settings
            commands::settings::get_player_settings,
            commands::settings::set_player_settings,
            commands::settings::get_appearance_settings,
            commands::settings::set_appearance_settings,
            commands::settings::get_job_settings,
            commands::settings::set_job_settings,
            commands::settings::check_for_updates,
            commands::settings::get_stashdb_settings,
            commands::settings::set_stashdb_settings,
            commands::settings::test_stashdb_connection,
            // identify (StashDB)
            commands::identify::identify_scene,
            commands::identify::apply_stashdb_match,
            commands::identify::clear_stashdb_identify,
            commands::identify::clear_stashdb_ignore,
            commands::identify::reject_stashdb_match,
            commands::identify::dismiss_stashdb_review,
            commands::identify::batch_set_stashdb_ignore,
            commands::batch_identify::batch_identify_scenes,
            commands::batch_identify::batch_identify_library,
            commands::batch_identify::stashdb_identify_stats,
            commands::path_meta::suggest_path_metadata,
            commands::path_meta::apply_path_metadata,
            commands::path_meta::batch_apply_path_metadata,
            commands::embedded_meta::suggest_embedded_metadata,
            commands::embedded_meta::apply_embedded_metadata,
            commands::stash_import::import_stash_metadata,
            // segments / timed bookmarks
            commands::segments::list_scene_segments,
            commands::segments::create_scene_segment,
            commands::segments::update_scene_segment,
            commands::segments::delete_scene_segment,
            // downscale / transcode
            commands::transcode::downscale_preview,
            commands::transcode::downscale_start,
            commands::transcode::downscale_cancel,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,maizeview_lib=debug,sqlx=warn"));
    let _ = fmt().with_env_filter(filter).with_target(false).try_init();
}
