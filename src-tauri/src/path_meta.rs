//! Local path → catalog metadata matching (ADR-013: parse only, never move files).
//!
//! Stash-style auto-tag: match **existing** performers / studios / tags against the
//! file path and filename. Does not invent new entities from folder names.

use std::path::{Component, Path};

/// Kind of catalog entity suggested from a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathMetaKind {
    Studio,
    Performer,
    Tag,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathMetaHit {
    pub kind: PathMetaKind,
    pub id: String,
    pub name: String,
}

/// Normalize a path or entity name for substring matching.
pub fn normalize_for_match(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_space = true;
    for ch in s.chars() {
        let c = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else {
            ' '
        };
        if c == ' ' {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(c);
            last_space = false;
        }
    }
    out.trim().to_string()
}

/// Build the searchable haystack from an absolute file path.
/// Includes parent folder names and the file stem (not the extension).
pub fn path_haystack(file_path: &Path) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(parent) = file_path.parent() {
        for c in parent.components() {
            if let Component::Normal(os) = c {
                if let Some(s) = os.to_str() {
                    let n = normalize_for_match(s);
                    if !n.is_empty() {
                        parts.push(n);
                    }
                }
            }
        }
    }
    if let Some(stem) = file_path.file_stem().and_then(|s| s.to_str()) {
        let n = normalize_for_match(stem);
        if !n.is_empty() {
            parts.push(n);
        }
    }
    parts.join(" ")
}

fn name_matches_haystack(haystack: &str, name_norm: &str) -> bool {
    if name_norm.is_empty() || name_norm.len() < 3 {
        return false;
    }
    // Whole-name token containment: surround with spaces for crude word boundaries.
    let padded = format!(" {haystack} ");
    let needle = format!(" {name_norm} ");
    padded.contains(&needle)
}

/// Match catalog names against a path. Longer names win first so short names
/// nested inside longer ones are less likely to false-positive alone.
pub fn match_catalog_against_path(
    file_path: &Path,
    studios: &[(String, String)],
    performers: &[(String, String)],
    tags: &[(String, String)],
) -> Vec<PathMetaHit> {
    let haystack = path_haystack(file_path);
    if haystack.is_empty() {
        return Vec::new();
    }

    let mut candidates: Vec<(usize, PathMetaHit)> = Vec::new();

    for (id, name) in studios {
        let nn = normalize_for_match(name);
        if name_matches_haystack(&haystack, &nn) {
            candidates.push((
                nn.len(),
                PathMetaHit {
                    kind: PathMetaKind::Studio,
                    id: id.clone(),
                    name: name.clone(),
                },
            ));
        }
    }
    for (id, name) in performers {
        let nn = normalize_for_match(name);
        if name_matches_haystack(&haystack, &nn) {
            candidates.push((
                nn.len(),
                PathMetaHit {
                    kind: PathMetaKind::Performer,
                    id: id.clone(),
                    name: name.clone(),
                },
            ));
        }
    }
    for (id, name) in tags {
        let nn = normalize_for_match(name);
        if name_matches_haystack(&haystack, &nn) {
            candidates.push((
                nn.len(),
                PathMetaHit {
                    kind: PathMetaKind::Tag,
                    id: id.clone(),
                    name: name.clone(),
                },
            ));
        }
    }

    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));

    // Drop hits whose normalized name is fully contained in a longer accepted hit
    // of the same kind (e.g. "Rob" inside "Robin Monroe").
    let mut accepted: Vec<PathMetaHit> = Vec::new();
    let mut accepted_norms: Vec<(PathMetaKind, String)> = Vec::new();
    for (_, hit) in candidates {
        let nn = normalize_for_match(&hit.name);
        let subsumed = accepted_norms
            .iter()
            .any(|(k, longer)| *k == hit.kind && longer != &nn && longer.contains(&nn));
        if subsumed {
            continue;
        }
        accepted_norms.push((hit.kind, nn));
        accepted.push(hit);
    }
    accepted
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn normalize_collapses_separators() {
        assert_eq!(
            normalize_for_match("Robin.Monroe-Scene_01"),
            "robin monroe scene 01"
        );
    }

    #[test]
    fn matches_studio_and_performer_in_path() {
        let path = PathBuf::from(r"D:\Videos\Lustre\Robin Monroe\scene.mp4");
        let studios = vec![("s1".into(), "Lustre".into())];
        let performers = vec![("p1".into(), "Robin Monroe".into())];
        let tags: Vec<(String, String)> = vec![];
        let hits = match_catalog_against_path(&path, &studios, &performers, &tags);
        assert!(hits
            .iter()
            .any(|h| h.kind == PathMetaKind::Studio && h.name == "Lustre"));
        assert!(hits
            .iter()
            .any(|h| h.kind == PathMetaKind::Performer && h.name == "Robin Monroe"));
    }

    #[test]
    fn matches_dotted_filename() {
        let path = PathBuf::from(r"E:\lib\lustre.robin.monroe.oil.mp4");
        let studios = vec![("s1".into(), "Lustre".into())];
        let performers = vec![("p1".into(), "Robin Monroe".into())];
        let hits = match_catalog_against_path(&path, &studios, &performers, &[]);
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn prefers_longer_name_over_substring() {
        let path = PathBuf::from(r"D:\Videos\Robin Monroe scene.mp4");
        let performers = vec![
            ("p_short".into(), "Rob".into()),
            ("p_long".into(), "Robin Monroe".into()),
        ];
        let hits = match_catalog_against_path(&path, &[], &performers, &[]);
        assert!(hits.iter().any(|h| h.id == "p_long"));
        assert!(!hits.iter().any(|h| h.id == "p_short"));
    }

    #[test]
    fn ignores_names_shorter_than_three() {
        let path = PathBuf::from(r"D:\Videos\Al scene.mp4");
        let performers = vec![("p1".into(), "Al".into())];
        let hits = match_catalog_against_path(&path, &[], &performers, &[]);
        assert!(hits.is_empty());
    }
}
