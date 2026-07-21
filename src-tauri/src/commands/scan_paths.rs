//! Scan path management: list/add/remove library folders.

use tauri::State;

use crate::{
    commands::err,
    models::{new_id, now, ScanPath},
    AppState,
};

#[tauri::command]
pub async fn list_scan_paths(state: State<'_, AppState>) -> Result<Vec<ScanPathListed>, String> {
    let rows: Vec<ScanPath> =
        sqlx::query_as("SELECT id, path, label, created_at FROM scan_paths ORDER BY created_at")
            .fetch_all(&state.pool)
            .await
            .map_err(err)?;
    Ok(rows
        .into_iter()
        .map(|p| ScanPathListed {
            accessible: std::path::Path::new(&p.path).is_dir(),
            id: p.id,
            path: p.path,
            label: p.label,
            created_at: p.created_at,
        })
        .collect())
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ScanPathListed {
    pub id: String,
    pub path: String,
    pub label: Option<String>,
    pub created_at: String,
    /// False when the folder is missing (unplugged drive, renamed path, etc.).
    pub accessible: bool,
}

#[derive(Debug, serde::Deserialize)]
pub struct AddScanPathArgs {
    pub path: String,
    pub label: Option<String>,
}

#[tauri::command]
pub async fn add_scan_path(
    state: State<'_, AppState>,
    args: AddScanPathArgs,
) -> Result<ScanPath, String> {
    // Normalize drive roots: `E:\` must NOT become `E:` (Windows drive-relative).
    let normalized = crate::paths::normalize_scan_root(&args.path);
    let path = std::path::Path::new(&normalized);
    if !path.is_dir() {
        return Err(format!("not a directory: {normalized}"));
    }

    let row = ScanPath {
        id: new_id(),
        path: normalized,
        label: args.label,
        created_at: now().to_rfc3339(),
    };

    sqlx::query("INSERT INTO scan_paths (id, path, label, created_at) VALUES (?, ?, ?, ?)")
        .bind(&row.id)
        .bind(&row.path)
        .bind(&row.label)
        .bind(&row.created_at)
        .execute(&state.pool)
        .await
        .map_err(err)?;

    Ok(row)
}

#[tauri::command]
pub async fn remove_scan_path(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let row: Option<(String,)> = sqlx::query_as("SELECT path FROM scan_paths WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.pool)
        .await
        .map_err(err)?;
    let Some((root,)) = row else {
        return Ok(());
    };

    sqlx::query("DELETE FROM scan_paths WHERE id = ?")
        .bind(&id)
        .execute(&state.pool)
        .await
        .map_err(err)?;

    // Drop catalog entries under this root (files stay on disk). Set-based:
    // one DELETE instead of a per-row loop; FK cascades + orphan prune handle
    // the rest. Semantics mirror catalog_cleanup::path_is_under_root
    // (normalize, case-insensitive, drive-root special case).
    let mut norm_root = crate::paths::normalize_windows_path(&root).to_lowercase();
    while norm_root.len() > 3 && norm_root.ends_with('\\') {
        norm_root.pop();
    }
    if !norm_root.is_empty() {
        let rb = norm_root.as_bytes();
        let drive_root = matches!(rb, [d, b':'] | [d, b':', b'\\'] if d.is_ascii_alphabetic());
        let pattern = if drive_root {
            // Anything on the volume (stored paths are backslash-normalized).
            format!("{}\\%", norm_root.trim_end_matches('\\'))
        } else {
            format!("{}\\%", like_escape(&norm_root))
        };
        sqlx::query("DELETE FROM files WHERE path = ? OR path LIKE ? ESCAPE '!'")
            .bind(&norm_root)
            .bind(pattern)
            .execute(&state.pool)
            .await
            .map_err(err)?;
    }
    crate::catalog_cleanup::prune_orphan_scenes(&state.pool)
        .await
        .map_err(err)?;

    Ok(())
}

/// Escape a literal for a LIKE pattern using '!' as the escape character.
fn like_escape(lit: &str) -> String {
    lit.replace('!', "!!").replace('%', "!%").replace('_', "!_")
}
