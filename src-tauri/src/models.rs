//! Typed domain models shared between the backend and the Tauri commands.
//!
//! These mirror the SQLite schema in `migrations/20260705000001_initial_schema.sql`.
//! Provenance fields (`*_source`) record where each piece of metadata came from
//! (ADR-006): "manual" by default, replaced by "stashdb" / "fansdb" / "iafd" / etc. when
//! a metadata server populates a field (Phase 2).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Generate a new sortable UUIDv7 string. Used for every primary key.
pub fn new_id() -> String {
    Uuid::now_v7().to_string()
}

/// Now, as RFC3339. Stored as TEXT in SQLite.
pub fn now() -> DateTime<Utc> {
    Utc::now()
}

// ─── scan_paths ───────────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ScanPath {
    pub id: String,
    pub path: String,
    pub label: Option<String>,
    pub created_at: String,
}

// ─── scenes ───────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Scene {
    pub id: String,
    pub title: Option<String>,
    pub details: Option<String>,
    pub title_source: String,
    pub details_source: String,
    pub studio_id: Option<String>,
    pub cover_path: Option<String>,
    pub cover_source: String,
    pub rating: Option<i64>,
    /// Favorite level 0..5 (0 = not favorited). Also weighted-shuffle weight.
    pub favorite: i64,
    pub play_count: i64,
    pub last_played_at: Option<String>,
    pub last_position: Option<f64>,
    pub stashdb_checked_at: Option<String>,
    pub stashdb_match_count: Option<i64>,
    pub stashdb_applied_at: Option<String>,
    /// When set, batch identify skips this scene (false-positive / manual ignore).
    pub stashdb_ignored_at: Option<String>,
    /// Remote stash-box scene id last applied (if any).
    pub stashdb_remote_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

// ─── files ────────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct VideoFile {
    pub id: String,
    pub scene_id: String,
    pub path: String,
    pub size_bytes: i64,
    pub modified_at: String,
    pub format_name: Option<String>,
    pub duration: Option<f64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub codec: Option<String>,
    pub fps: Option<f64>,
    pub bitrate: Option<i64>,
    pub thumb_path: Option<String>,
    pub thumb_sprite_path: Option<String>,
    pub vtt_path: Option<String>,
    pub scanned_at: String,
}

// ─── fingerprints ─────────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Fingerprint {
    pub id: String,
    pub file_id: String,
    pub hash_type: String,
    pub value: String,
    pub created_at: String,
}

// ─── performers / studios / tags ──────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Performer {
    pub id: String,
    pub name: String,
    pub aliases: Option<String>,
    pub image_path: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Studio {
    pub id: String,
    pub name: String,
    pub aliases: Option<String>,
    pub image_path: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Tag {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub color: Option<String>,
    pub created_at: String,
}

// ─── playlists ────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
}

// ─── scan_runs ────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ScanRun {
    pub id: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub status: String,
    pub paths_scanned: i64,
    pub files_found: i64,
    pub files_added: i64,
    pub files_updated: i64,
    pub files_removed: i64,
    pub error_message: Option<String>,
}

/// Counts reported while a scan is in progress (or after it finishes).
/// Emitted to the frontend over the Tauri event bus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanProgress {
    pub scan_run_id: String,
    pub status: String, // "running" | "completed" | "failed" | "cancelled"
    pub phase: String,  // "walking" | "indexing" | "writing" | "done" | ...
    pub files_found: i64,
    pub files_added: i64,
    pub files_updated: i64,
    pub files_removed: i64,
    /// How many files have been hashed+probed so far (for the indexing phase).
    pub files_processed: i64,
    /// Path of the file currently being processed (best-effort; for display).
    pub current_path: Option<String>,
    /// Paths configured but skipped because the folder was not reachable at scan start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped_paths: Option<Vec<String>>,
}
