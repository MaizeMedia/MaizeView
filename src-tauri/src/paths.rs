//! Windows path normalization for library roots and stored file paths.
//!
//! Bare drive roots like `E:` are *drive-relative* on Windows (current directory
//! on that volume). Joining / walking them yields broken paths such as
//! `E:Sorted\foo.mp4` instead of `E:\Sorted\foo.mp4`. MaizeView always wants
//! absolute `X:\...` forms.

/// Insert `\` after `X:` when missing; normalize `/` → `\`.
///
/// - `E:` → `E:\`
/// - `E:Sorted\a.mp4` → `E:\Sorted\a.mp4`
/// - `E:\Sorted\a.mp4` → unchanged (aside from `/` → `\`)
pub fn normalize_windows_path(path: &str) -> String {
    let mut s = path.trim().replace('/', "\\");
    let bytes = s.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        if bytes.len() == 2 {
            s.push('\\');
        } else if bytes[2] != b'\\' {
            s.insert(2, '\\');
        }
    }
    s
}

/// Scan-folder root: normalize drive form, then trim trailing separators
/// except for a bare drive root (`E:\`).
pub fn normalize_scan_root(path: &str) -> String {
    let mut s = normalize_windows_path(path);
    while s.len() > 3 && s.ends_with('\\') {
        s.pop();
    }
    s
}

/// Fix scan roots + file paths stored as drive-relative (`E:foo` / bare `E:`).
/// Safe to run on every startup (no-op when already normalized).
pub async fn repair_drive_relative_paths(pool: &sqlx::SqlitePool) -> Result<u64, sqlx::Error> {
    let mut fixed = 0u64;

    let roots: Vec<(String, String)> = sqlx::query_as("SELECT id, path FROM scan_paths")
        .fetch_all(pool)
        .await?;
    for (id, path) in roots {
        let next = normalize_scan_root(&path);
        if next != path {
            sqlx::query("UPDATE scan_paths SET path = ? WHERE id = ?")
                .bind(&next)
                .bind(&id)
                .execute(pool)
                .await?;
            fixed += 1;
            tracing::info!(from = %path, to = %next, "normalized scan root");
        }
    }

    // Cheap probe before loading every (id, path) on each launch: a
    // drive-relative path is exactly `X:` followed by a non-`\` character or
    // end-of-string (see normalize_windows_path), i.e. `_` matches any single
    // drive letter and LIKE's case-insensitivity is irrelevant. When the probe
    // misses, the loop below would fix nothing, so skip the full-table read.
    let suspect = sqlx::query(
        r#"SELECT 1 FROM files WHERE path LIKE '_:%' AND path NOT LIKE '_:\%' LIMIT 1"#,
    )
    .fetch_optional(pool)
    .await?;

    if suspect.is_some() {
        let files: Vec<(String, String)> = sqlx::query_as("SELECT id, path FROM files")
            .fetch_all(pool)
            .await?;
        for (id, path) in files {
            let next = normalize_windows_path(&path);
            if next == path {
                continue;
            }
            // Skip if the corrected path already exists (rare collision).
            let conflict: Option<(String,)> =
                sqlx::query_as("SELECT id FROM files WHERE path = ? AND id != ?")
                    .bind(&next)
                    .bind(&id)
                    .fetch_optional(pool)
                    .await?;
            if conflict.is_some() {
                tracing::warn!(
                    from = %path,
                    to = %next,
                    "skip path repair — target already exists"
                );
                continue;
            }
            sqlx::query("UPDATE files SET path = ? WHERE id = ?")
                .bind(&next)
                .bind(&id)
                .execute(pool)
                .await?;
            fixed += 1;
        }
    }

    if fixed > 0 {
        tracing::info!(fixed, "repaired drive-relative Windows paths");
    }
    Ok(fixed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_drive_becomes_root() {
        assert_eq!(normalize_windows_path("E:"), r"E:\");
        assert_eq!(normalize_windows_path("e:"), r"e:\");
    }

    #[test]
    fn drive_relative_gets_slash() {
        assert_eq!(
            normalize_windows_path(r"E:Sorted\foo.mp4"),
            r"E:\Sorted\foo.mp4"
        );
        assert_eq!(
            normalize_windows_path("G:Example Series PL\\880.mp4"),
            r"G:\Example Series PL\880.mp4"
        );
    }

    #[test]
    fn already_absolute_unchanged() {
        assert_eq!(normalize_windows_path(r"C:\Media\a.mp4"), r"C:\Media\a.mp4");
        assert_eq!(normalize_windows_path(r"E:\Sorted"), r"E:\Sorted");
    }

    #[test]
    fn forward_slashes_normalized() {
        assert_eq!(
            normalize_windows_path("E:/Sorted/a.mp4"),
            r"E:\Sorted\a.mp4"
        );
    }

    #[test]
    fn scan_root_keeps_drive_slash() {
        assert_eq!(normalize_scan_root(r"E:\"), r"E:\");
        assert_eq!(normalize_scan_root("E:"), r"E:\");
        assert_eq!(normalize_scan_root(r"C:\Media\"), r"C:\Media");
        assert_eq!(normalize_scan_root(r"C:\Media"), r"C:\Media");
    }
}
