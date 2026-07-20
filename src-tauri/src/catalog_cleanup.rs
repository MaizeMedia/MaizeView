//! Catalog hygiene: drop scenes that no longer have any file rows.
//!
//! Scan "removed file" detection deletes `files` rows when paths vanish, but
//! historically left empty `scenes` behind (metadata / playlist ghosts). Those
//! orphans inflate identify stats, playlist counts, and the library grid.
//!
//! Startup also drops `files` rows whose path is gone on disk (unless the
//! parent scan root is offline), then prunes the resulting empty scenes.

use std::path::Path;

use sqlx::SqlitePool;
use tracing::info;

/// Delete file rows whose path is missing on disk, then prune empty scenes.
/// Skips anything under an offline/missing scan-path root.
pub async fn reconcile_missing_files(pool: &SqlitePool) -> anyhow::Result<u64> {
    let roots: Vec<String> = sqlx::query_scalar("SELECT path FROM scan_paths")
        .fetch_all(pool)
        .await?;
    let files: Vec<(String, String)> = sqlx::query_as("SELECT id, path FROM files")
        .fetch_all(pool)
        .await?;

    // The stat checks are blocking syscalls — slow on USB/spinning drives at
    // 10k+ files — so run them off the async runtime's worker threads.
    let missing: Vec<String> = tokio::task::spawn_blocking(move || {
        let offline_roots: Vec<String> = roots
            .into_iter()
            .filter(|r| !Path::new(r).is_dir())
            .collect();
        files
            .into_iter()
            .filter(|(_, path)| {
                !offline_roots
                    .iter()
                    .any(|root| path_is_under_root(path, root))
                    && !Path::new(path).is_file()
            })
            .map(|(file_id, _)| file_id)
            .collect()
    })
    .await?;

    let mut removed = 0u64;
    for file_id in missing {
        sqlx::query("DELETE FROM files WHERE id = ?")
            .bind(&file_id)
            .execute(pool)
            .await?;
        removed += 1;
    }

    if removed > 0 {
        info!(
            missing_files = removed,
            "removed catalog files missing on disk"
        );
    }
    let _ = prune_orphan_scenes(pool).await?;
    Ok(removed)
}

/// Delete scenes with zero remaining files. Cascades playlist_items, tags, etc.
/// Returns how many scenes were removed.
pub async fn prune_orphan_scenes(pool: &SqlitePool) -> anyhow::Result<u64> {
    let result = sqlx::query(
        r#"
        DELETE FROM scenes
        WHERE NOT EXISTS (
            SELECT 1 FROM files f WHERE f.scene_id = scenes.id
        )
        "#,
    )
    .execute(pool)
    .await?;
    let n = result.rows_affected();
    if n > 0 {
        info!(orphans = n, "pruned scenes with no files");
    }
    Ok(n)
}

/// True when `file_path` is the scan root or a descendant (Windows-tolerant).
pub fn path_is_under_root(file_path: &str, root: &str) -> bool {
    let file = crate::paths::normalize_windows_path(file_path).to_lowercase();
    let mut root = crate::paths::normalize_windows_path(root).to_lowercase();
    while root.len() > 3 && root.ends_with('\\') {
        root.pop();
    }
    if root.is_empty() {
        return false;
    }
    // Bare drive root `e:` / `e:\` → anything on that volume.
    let rb = root.as_bytes();
    let drive_root = matches!(rb, [d, b':'] | [d, b':', b'\\'] if d.is_ascii_alphabetic());
    if drive_root {
        let fb = file.as_bytes();
        return fb.len() >= 2
            && fb[1] == b':'
            && fb[0] == rb[0]
            && (fb.len() == 2 || fb.get(2) == Some(&b'\\'));
    }
    file == root || file.starts_with(&(root + "\\"))
}

#[cfg(test)]
mod tests {
    use super::path_is_under_root;

    #[test]
    fn path_under_root_windows_style() {
        assert!(path_is_under_root(r"D:\vids\a.mp4", r"D:\vids"));
        assert!(path_is_under_root(r"D:\vids", r"D:\vids"));
        assert!(path_is_under_root(r"d:/vids/a.mp4", r"D:\vids"));
        assert!(!path_is_under_root(r"D:\other\a.mp4", r"D:\vids"));
        assert!(!path_is_under_root(r"D:\vids2\a.mp4", r"D:\vids"));
    }

    #[test]
    fn path_under_drive_root() {
        assert!(path_is_under_root(r"E:\Sorted\a.mp4", r"E:\"));
        assert!(path_is_under_root(r"E:\Sorted\a.mp4", "E:"));
        // Legacy drive-relative form (pre-fix) still matches after normalize.
        assert!(path_is_under_root(r"E:Sorted\a.mp4", r"E:\"));
        assert!(!path_is_under_root(r"F:\Sorted\a.mp4", r"E:\"));
    }
}
