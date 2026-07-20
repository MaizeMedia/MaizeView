//! App settings persisted in `schema_meta` (key/value).

use tauri::State;

use crate::{commands::err, AppState};

const KEY_PLAYER_VOLUME: &str = "player_volume";
const KEY_PLAYER_MUTED: &str = "player_muted";
const KEY_PLAYER_DELETE_ENABLED: &str = "player_delete_enabled";
/// Max parallel workers for scan indexing + preview/pHash/MD5 (`0` = auto).
const KEY_JOB_WORKERS_MAX: &str = "job_workers_max";
const KEY_UI_ACCENT: &str = "ui_accent_preset";
const KEY_STASHDB_API_KEY: &str = "stashdb_api_key";
const KEY_STASHDB_ENDPOINT: &str = "stashdb_endpoint";
const KEY_STASH_BOX_ACTIVE: &str = "stash_box_active";
const KEY_STASH_BOX_KEYS: &str = "stash_box_keys"; // JSON object { "stashdb": "key", ... }
const KEY_STASH_BOX_WATERFALL: &str = "stash_box_waterfall"; // "1" = try all keyed boxes
const DEFAULT_VOLUME: f64 = 75.0;
const DEFAULT_ACCENT: &str = "maize";

/// Allowed accent preset ids (must match frontend CSS data-accent values).
const ACCENT_PRESETS: &[&str] = &["maize", "teal", "coral", "slate", "rose"];

fn normalize_accent(id: &str) -> String {
    let lower = id.trim().to_lowercase();
    if ACCENT_PRESETS.contains(&lower.as_str()) {
        lower
    } else {
        DEFAULT_ACCENT.to_string()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AppearanceSettings {
    /// Accent preset id: maize | teal | coral | slate | rose
    pub accent_preset: String,
}

#[tauri::command]
pub async fn get_appearance_settings(
    state: State<'_, AppState>,
) -> Result<AppearanceSettings, String> {
    let raw = read_meta(&state.pool, KEY_UI_ACCENT)
        .await
        .unwrap_or_else(|| DEFAULT_ACCENT.to_string());
    Ok(AppearanceSettings {
        accent_preset: normalize_accent(&raw),
    })
}

#[tauri::command]
pub async fn set_appearance_settings(
    state: State<'_, AppState>,
    accent_preset: String,
) -> Result<AppearanceSettings, String> {
    let id = normalize_accent(&accent_preset);
    write_meta(&state.pool, KEY_UI_ACCENT, &id).await?;
    Ok(AppearanceSettings { accent_preset: id })
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlayerSettings {
    /// Playback volume 0..100 (mpv scale).
    pub volume: f64,
    pub muted: bool,
    /// When true, the player overlay shows a delete button (with confirmation).
    pub delete_in_player_enabled: bool,
}

async fn read_meta(pool: &sqlx::SqlitePool, key: &str) -> Option<String> {
    let row: Option<(String,)> = sqlx::query_as("SELECT value FROM schema_meta WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .ok()?;
    row.map(|(v,)| v)
}

async fn write_meta(pool: &sqlx::SqlitePool, key: &str, value: &str) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO schema_meta (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await
    .map_err(err)?;
    Ok(())
}

#[tauri::command]
pub async fn get_player_settings(state: State<'_, AppState>) -> Result<PlayerSettings, String> {
    let pool = &state.pool;
    let volume = read_meta(pool, KEY_PLAYER_VOLUME)
        .await
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(DEFAULT_VOLUME)
        .clamp(0.0, 100.0);
    let muted = read_meta(pool, KEY_PLAYER_MUTED)
        .await
        .is_some_and(|s| s == "1" || s.eq_ignore_ascii_case("true"));
    let delete_in_player_enabled = read_meta(pool, KEY_PLAYER_DELETE_ENABLED)
        .await
        .is_some_and(|s| s == "1" || s.eq_ignore_ascii_case("true"));
    Ok(PlayerSettings {
        volume,
        muted,
        delete_in_player_enabled,
    })
}

#[tauri::command]
pub async fn set_player_settings(
    state: State<'_, AppState>,
    volume: f64,
    muted: bool,
    delete_in_player_enabled: Option<bool>,
) -> Result<PlayerSettings, String> {
    let volume = volume.clamp(0.0, 100.0);
    let pool = &state.pool;
    write_meta(pool, KEY_PLAYER_VOLUME, &volume.to_string()).await?;
    write_meta(pool, KEY_PLAYER_MUTED, if muted { "1" } else { "0" }).await?;
    if let Some(enabled) = delete_in_player_enabled {
        write_meta(
            pool,
            KEY_PLAYER_DELETE_ENABLED,
            if enabled { "1" } else { "0" },
        )
        .await?;
    }
    get_player_settings(state).await
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct StashBoxPresetInfo {
    pub id: String,
    pub name: String,
    pub endpoint: String,
    pub account_url: String,
    pub api_key_set: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct StashDbSettings {
    /// Active stash-box preset id (`stashdb`, `tpdb`, `fansdb`, `javstash`).
    pub active_id: String,
    /// Whether an API key is stored for the active box (never returned).
    pub api_key_set: bool,
    pub endpoint: String,
    /// When true, fingerprint identify tries every box that has an API key
    /// (active first), then falls back to title search on the matching/active box.
    pub waterfall: bool,
    pub presets: Vec<StashBoxPresetInfo>,
}

async fn read_box_keys(
    pool: &sqlx::SqlitePool,
) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    if let Some(raw) = read_meta(pool, KEY_STASH_BOX_KEYS).await {
        if let Ok(serde_json::Value::Object(map)) = serde_json::from_str(&raw) {
            return Ok(map);
        }
    }
    // Migrate legacy single-key storage into the multi-box map.
    let mut map = serde_json::Map::new();
    if let Some(legacy) = read_meta(pool, KEY_STASHDB_API_KEY).await {
        if !legacy.trim().is_empty() {
            map.insert("stashdb".into(), serde_json::Value::String(legacy));
            let serialized = serde_json::to_string(&map).map_err(|e| e.to_string())?;
            write_meta(pool, KEY_STASH_BOX_KEYS, &serialized).await?;
        }
    }
    Ok(map)
}

/// Read active stash-box config for backend use (endpoint + API key).
pub async fn stashdb_config(pool: &sqlx::SqlitePool) -> Result<(String, Option<String>), String> {
    let active = active_stash_box_id(pool).await;
    let keys = read_box_keys(pool).await?;
    let api_key = keys
        .get(&active)
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string());

    let endpoint = if let Some(preset) = crate::stashdb::preset_by_id(&active) {
        preset.endpoint.to_string()
    } else {
        read_meta(pool, KEY_STASHDB_ENDPOINT)
            .await
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| crate::stashdb::default_endpoint().to_string())
    };

    Ok((endpoint, api_key))
}

pub async fn active_stash_box_id(pool: &sqlx::SqlitePool) -> String {
    read_meta(pool, KEY_STASH_BOX_ACTIVE)
        .await
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "stashdb".into())
}

pub async fn waterfall_enabled(pool: &sqlx::SqlitePool) -> bool {
    read_meta(pool, KEY_STASH_BOX_WATERFALL)
        .await
        .is_some_and(|s| s == "1" || s.eq_ignore_ascii_case("true"))
}

/// Boxes to try for fingerprint identify: active first, then other presets with keys.
pub async fn stash_box_query_targets(
    pool: &sqlx::SqlitePool,
) -> Result<Vec<(String, String, String)>, String> {
    let active = active_stash_box_id(pool).await;
    let keys = read_box_keys(pool).await?;
    let waterfall = waterfall_enabled(pool).await;

    let mut targets = Vec::new();
    let mut push = |id: &str| {
        let Some(preset) = crate::stashdb::preset_by_id(id) else {
            return;
        };
        let Some(key) = keys
            .get(id)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            return;
        };
        if targets.iter().any(|(existing, _, _)| existing == id) {
            return;
        }
        targets.push((id.to_string(), preset.endpoint.to_string(), key.to_string()));
    };

    push(&active);
    if waterfall {
        for p in crate::stashdb::STASH_BOX_PRESETS {
            push(p.id);
        }
    }

    Ok(targets)
}

#[tauri::command]
pub async fn get_stashdb_settings(state: State<'_, AppState>) -> Result<StashDbSettings, String> {
    let pool = &state.pool;
    let active_id = active_stash_box_id(pool).await;
    let keys = read_box_keys(pool).await?;
    let (endpoint, api_key) = stashdb_config(pool).await?;
    let waterfall = waterfall_enabled(pool).await;

    let presets = crate::stashdb::STASH_BOX_PRESETS
        .iter()
        .map(|p| StashBoxPresetInfo {
            id: p.id.to_string(),
            name: p.name.to_string(),
            endpoint: p.endpoint.to_string(),
            account_url: p.account_url.to_string(),
            api_key_set: keys
                .get(p.id)
                .and_then(|v| v.as_str())
                .is_some_and(|k| !k.trim().is_empty()),
        })
        .collect();

    Ok(StashDbSettings {
        active_id,
        api_key_set: api_key.is_some_and(|k| !k.trim().is_empty()),
        endpoint,
        waterfall,
        presets,
    })
}

/// Update active stash-box and/or its API key.
/// Pass null/empty `api_key` to clear the key for the (new) active box.
#[tauri::command]
pub async fn set_stashdb_settings(
    state: State<'_, AppState>,
    api_key: Option<String>,
    endpoint: Option<String>,
    active_id: Option<String>,
    waterfall: Option<bool>,
) -> Result<StashDbSettings, String> {
    let pool = &state.pool;

    if let Some(id) = active_id {
        let id = id.trim();
        if crate::stashdb::preset_by_id(id).is_none() {
            return Err(format!("unknown stash-box preset: {id}"));
        }
        write_meta(pool, KEY_STASH_BOX_ACTIVE, id).await?;
        if let Some(preset) = crate::stashdb::preset_by_id(id) {
            write_meta(pool, KEY_STASHDB_ENDPOINT, preset.endpoint).await?;
        }
    }

    if let Some(wf) = waterfall {
        write_meta(pool, KEY_STASH_BOX_WATERFALL, if wf { "1" } else { "0" }).await?;
    }

    let active = active_stash_box_id(pool).await;

    if let Some(key) = api_key {
        let mut keys = read_box_keys(pool).await?;
        let trimmed = key.trim();
        if trimmed.is_empty() {
            keys.remove(&active);
        } else {
            keys.insert(
                active.clone(),
                serde_json::Value::String(trimmed.to_string()),
            );
        }
        let serialized = serde_json::to_string(&keys).map_err(|e| e.to_string())?;
        write_meta(pool, KEY_STASH_BOX_KEYS, &serialized).await?;
        // Keep legacy key in sync when active is stashdb.
        if active == "stashdb" {
            if trimmed.is_empty() {
                sqlx::query("DELETE FROM schema_meta WHERE key = ?")
                    .bind(KEY_STASHDB_API_KEY)
                    .execute(pool)
                    .await
                    .map_err(err)?;
            } else {
                write_meta(pool, KEY_STASHDB_API_KEY, trimmed).await?;
            }
        }
    }

    // Custom endpoint only when active id is not a known preset (future-proof).
    if let Some(ep) = endpoint {
        let trimmed = ep.trim();
        if !trimmed.is_empty() && crate::stashdb::preset_by_id(&active).is_none() {
            write_meta(pool, KEY_STASHDB_ENDPOINT, trimmed).await?;
        }
    }

    get_stashdb_settings(state).await
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct StashDbTestResult {
    pub username: String,
}

/// Test StashDB credentials. Uses the stored key/endpoint unless overrides are passed
/// (e.g. to validate before saving a new key from Settings).
#[tauri::command]
pub async fn test_stashdb_connection(
    state: State<'_, AppState>,
    api_key: Option<String>,
    endpoint: Option<String>,
) -> Result<StashDbTestResult, String> {
    let pool = &state.pool;
    let (stored_endpoint, stored_key) = stashdb_config(pool).await?;

    let endpoint = endpoint
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(stored_endpoint);
    let key = api_key
        .filter(|s| !s.trim().is_empty())
        .or(stored_key)
        .ok_or_else(|| {
            "No API key configured — enter a key and save, or paste one to test".to_string()
        })?;

    let result = crate::stashdb::test_connection(&endpoint, &key).await?;
    Ok(StashDbTestResult {
        username: result.username,
    })
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct JobSettings {
    /// `0` = auto (CPU − reserve, clamped). Else 1..=16 hard cap.
    pub workers_max: u32,
    /// What the next scan / media job will actually use right now.
    pub effective_workers: u32,
    pub cpu_count: u32,
}

fn normalize_workers_max(raw: u32) -> u32 {
    if raw == 0 {
        0
    } else {
        raw.clamp(1, crate::job_parallel::MEDIA_JOB_WORKERS_MAX as u32)
    }
}

/// Load persisted cap into the process-wide atomic (call at startup).
pub async fn apply_job_workers_from_db(pool: &sqlx::SqlitePool) {
    let cap = read_meta(pool, KEY_JOB_WORKERS_MAX)
        .await
        .and_then(|s| s.parse::<u32>().ok())
        .map(normalize_workers_max)
        .unwrap_or(0);
    crate::job_parallel::set_job_workers_cap(cap as usize);
}

#[tauri::command]
pub async fn get_job_settings(state: State<'_, AppState>) -> Result<JobSettings, String> {
    let cap = read_meta(&state.pool, KEY_JOB_WORKERS_MAX)
        .await
        .and_then(|s| s.parse::<u32>().ok())
        .map(normalize_workers_max)
        .unwrap_or(0);
    // Keep runtime atomic in sync (e.g. after restore / first read).
    crate::job_parallel::set_job_workers_cap(cap as usize);
    Ok(JobSettings {
        workers_max: cap,
        effective_workers: crate::job_parallel::media_job_workers() as u32,
        cpu_count: crate::job_parallel::cpu_count() as u32,
    })
}

#[tauri::command]
pub async fn set_job_settings(
    state: State<'_, AppState>,
    workers_max: u32,
) -> Result<JobSettings, String> {
    let cap = normalize_workers_max(workers_max);
    write_meta(&state.pool, KEY_JOB_WORKERS_MAX, &cap.to_string()).await?;
    crate::job_parallel::set_job_workers_cap(cap as usize);
    get_job_settings(state).await
}

// ─── manual update check (user-initiated ONLY) ─────────────────────────

/// Result of the "Check for updates" button. The app makes no background
/// network calls — this only runs when the user explicitly asks.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UpdateCheck {
    pub current: String,
    pub latest: String,
    pub url: String,
    pub update_available: bool,
}

const RELEASES_API: &str = "https://api.github.com/repos/MaizeMedia/MaizeView/releases/latest";

#[tauri::command]
pub async fn check_for_updates() -> Result<UpdateCheck, String> {
    let client = reqwest::Client::builder()
        .user_agent("MaizeView update check")
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(RELEASES_API)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    if !status.is_success() {
        // NOTE: while the repo is PRIVATE this is always 404 — GitHub hides
        // private-repo releases from unauthenticated callers. Flip the repo
        // public and this just works.
        return Err(format!(
            "GitHub returned {status} (repo is private — update check works once public)"
        ));
    }
    let resp: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let latest = resp["tag_name"]
        .as_str()
        .unwrap_or("")
        .trim_start_matches('v')
        .to_string();
    let url = resp["html_url"].as_str().unwrap_or("").to_string();
    if latest.is_empty() || url.is_empty() {
        return Err("unexpected response from GitHub".into());
    }
    let current = env!("CARGO_PKG_VERSION").to_string();
    Ok(UpdateCheck {
        update_available: semver_gt(&latest, &current),
        current,
        latest,
        url,
    })
}

/// Is version `a` newer than `b`? Numeric X.Y.Z parts only (missing → 0).
fn semver_gt(a: &str, b: &str) -> bool {
    fn parts(s: &str) -> Vec<u64> {
        s.trim_start_matches('v')
            .split('.')
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    }
    parts(a) > parts(b)
}

#[cfg(test)]
mod tests {
    use super::semver_gt;

    #[test]
    fn semver_gt_basics() {
        assert!(semver_gt("0.3.3", "0.3.2"));
        assert!(semver_gt("0.4.0", "0.3.9"));
        assert!(semver_gt("1.0.0", "0.9.9"));
        assert!(semver_gt("v0.3.3", "0.3.2"));
        assert!(!semver_gt("0.3.2", "0.3.2"));
        assert!(!semver_gt("0.3.1", "0.3.2"));
        assert!(!semver_gt("0.3", "0.3.2")); // missing part = 0 → 0.3.0 < 0.3.2
    }
}
