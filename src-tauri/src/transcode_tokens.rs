//! Resolution-token rewriting for the downscale/convert feature.
//!
//! When a user transcodes a set of scenes to a lower resolution, the catalog
//! metadata that *described* the old resolution becomes stale: filenames like
//! `Scene 4K.mp4`, scene titles, and tags named `4K`/`2160p`/`UHD` no longer
//! match the file on disk. This module rewrites those tokens.
//!
//! It is deliberately pure (no I/O, no DB) so it can be unit-tested directly
//! and shared by the filename, title, and tag paths in `transcode_job`.
//!
//! Design notes:
//!   * Tokens are matched on whole-word boundaries (so `2160` in a performer
//!     name like `Sasha2160` is left alone unless it's a standalone token).
//!   * `Replace` swaps to the canonical token for the target height;
//!     `Remove` strips the token and tidies separators; `Leave` is a no-op.
//!   * Token↔height tables mirror the `4K+`/`2160` mapping already used by the
//!     search resolution filter (`filter-panel.svelte` HEIGHT_PRESETS).

use std::collections::BTreeMap;

/// How to treat a resolution token found in a filename/title/tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RewriteMode {
    /// Swap the token to the canonical form for the target resolution
    /// (e.g. `4K` → `1080p`, `UHD` → `1080p`).
    Replace,
    /// Strip the token and collapse leftover separators
    /// (e.g. `Scene 4K.mp4` → `Scene.mp4`).
    Remove,
    /// Leave the text untouched.
    Leave,
}

/// A recognized resolution token and the height (in pixels) it denotes.
struct TokenSpec {
    /// Lowercase matcher; may be a word (`4k`) or numeric (`2160p`, `2160`).
    token: &'static str,
    /// Height this token implies. `2160p`/`4K`/`UHD` → 2160, etc.
    height: u32,
}

/// All tokens we recognize, in priority order. Order matters: longer/more
/// specific tokens (`2160p`) are listed before their numeric substrings
/// (`2160`) so we match the tightest token first.
const TOKENS: &[TokenSpec] = &[
    // 8K / 4320p
    TokenSpec {
        token: "4320p",
        height: 4320,
    },
    TokenSpec {
        token: "4320",
        height: 4320,
    },
    TokenSpec {
        token: "8k",
        height: 4320,
    },
    TokenSpec {
        token: "uhd8k",
        height: 4320,
    },
    // 4K / 2160p / UHD
    TokenSpec {
        token: "2160p",
        height: 2160,
    },
    TokenSpec {
        token: "2160",
        height: 2160,
    },
    TokenSpec {
        token: "4k",
        height: 2160,
    },
    TokenSpec {
        token: "uhd",
        height: 2160,
    },
    // 1440p / 2K
    TokenSpec {
        token: "1440p",
        height: 1440,
    },
    TokenSpec {
        token: "1440",
        height: 1440,
    },
    TokenSpec {
        token: "2k",
        height: 1440,
    },
    TokenSpec {
        token: "qhd",
        height: 1440,
    },
    // 1080p / Full HD / FHD
    TokenSpec {
        token: "1080p",
        height: 1080,
    },
    TokenSpec {
        token: "1080",
        height: 1080,
    },
    TokenSpec {
        token: "fhd",
        height: 1080,
    },
    // 720p / HD
    TokenSpec {
        token: "720p",
        height: 720,
    },
    TokenSpec {
        token: "720",
        height: 720,
    },
    // 480p / SD
    TokenSpec {
        token: "480p",
        height: 480,
    },
    TokenSpec {
        token: "480",
        height: 480,
    },
    TokenSpec {
        token: "sd",
        height: 480,
    },
];

/// Canonical display token for a given target height (the replacement text).
/// Returns the token without separators; callers embed it appropriately.
pub fn canonical_token(height: u32) -> &'static str {
    match height {
        4320 => "4320p",
        2160 => "4K",
        1440 => "1440p",
        1080 => "1080p",
        720 => "720p",
        480 => "480p",
        _ => "downscaled",
    }
}

/// True if `s` is exactly one of the recognized resolution tokens (case-
/// insensitive), used to decide whether a *tag name* is a resolution tag.
pub fn is_resolution_token(s: &str) -> bool {
    let lower = s.trim().to_ascii_lowercase();
    TOKENS.iter().any(|t| t.token == lower)
}

/// The height implied by a standalone token string, if it is one.
pub fn token_height(s: &str) -> Option<u32> {
    let lower = s.trim().to_ascii_lowercase();
    TOKENS.iter().find(|t| t.token == lower).map(|t| t.height)
}

/// Split a filename into `(stem, extension)` at the last `.`. If there is no
/// extension (or the only dot is a leading hidden-file dot), the extension is
/// empty. Used so we rewrite tokens in the stem but never the extension.
fn split_name(name: &str) -> (String, String) {
    match name.rfind('.') {
        Some(pos) if pos > 0 => (name[..pos].to_string(), name[pos..].to_string()),
        _ => (name.to_string(), String::new()),
    }
}

/// Walk the string char-by-char, classifying each as word or delimiter. We
/// build the output by appending either a (possibly rewritten) word or the
/// raw delimiter slice. Indexing is over `char` positions, then mapped back to
/// byte offsets via the precomputed char vector so slicing stays on UTF-8
/// boundaries.
fn process_with_words<F: FnMut(&str) -> Option<String>>(input: &str, mut on_word: F) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_alphanumeric() {
            // Gather a maximal alphanumeric run.
            let start = i;
            while i < chars.len() && chars[i].is_ascii_alphanumeric() {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            match on_word(&word) {
                Some(replacement) => out.push_str(&replacement),
                None => out.push_str(&word),
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Find the resolution token (as a lowercased word) within `name`'s stem, if
/// any. Returns the height it implies. Used by the downscale preview to tell
/// the user how many of the selected filenames actually carry a token.
pub fn detect_token_height(name: &str) -> Option<u32> {
    let (stem, _ext) = split_name(name);
    let mut found: Option<u32> = None;
    process_with_words(&stem, |w| {
        let lower = w.to_ascii_lowercase();
        for spec in TOKENS {
            if spec.token == lower {
                found = Some(spec.height);
                return None; // leave the word as-is during detection
            }
        }
        None
    });
    found
}

/// Rewrite resolution tokens in `name` according to `mode`, targeting
/// `target_height`. Works on filenames, scene titles, and tag names.
///
/// For filenames the extension is preserved. `Replace` keeps the same separator
/// style as the matched token's surrounding; `Remove` collapses runs of
/// separators/whitespace left behind.
pub fn rewrite_resolution_token(name: &str, mode: RewriteMode, target_height: u32) -> String {
    if matches!(mode, RewriteMode::Leave) {
        return name.to_string();
    }

    let (stem, ext) = split_name(name);
    let replacement_token = canonical_token(target_height);

    let rewritten = process_with_words(&stem, |word| {
        let lower = word.to_ascii_lowercase();
        let matched = TOKENS.iter().find(|t| t.token == lower);
        match (mode, matched) {
            (RewriteMode::Replace, Some(_)) => {
                // Preserve original case style loosely: only uppercase the
                // replacement when the matched token is all-caps *alphabetic*
                // (e.g. "UHD", "FHD"). Mixed tokens like "4K" or "2160p" keep
                // the canonical lowercase-p form.
                let repl = if word.chars().all(|c| c.is_ascii_uppercase()) {
                    replacement_token.to_ascii_uppercase()
                } else {
                    replacement_token.to_string()
                };
                Some(repl)
            }
            (RewriteMode::Remove, Some(_)) => Some(String::new()),
            _ => None,
        }
    });

    let cleaned = match mode {
        RewriteMode::Remove => collapse_separators(&rewritten),
        _ => rewritten,
    };

    if ext.is_empty() {
        cleaned
    } else {
        format!("{cleaned}{ext}")
    }
}

/// After a token is removed, tidy leftover separators. Collapses runs of
/// `-_. ` into a single space, trims leading/trailing separators, and removes
/// `() [] {}` that became empty.
fn collapse_separators(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    let mut prev_was_sep = false;

    for (i, &c) in chars.iter().enumerate() {
        let is_sep = matches!(c, ' ' | '-' | '_' | '.' | '[' | ']' | '(' | ')' | '{' | '}');
        if is_sep {
            if !prev_was_sep {
                out.push(' ');
                prev_was_sep = true;
            }
            // Skip adjacent separators.
            let _ = i;
        } else {
            out.push(c);
            prev_was_sep = false;
        }
    }

    // Trim trailing separator space.
    while out.last() == Some(&' ') {
        out.pop();
    }
    // Trim leading separator space.
    while out.first() == Some(&' ') {
        out.remove(0);
    }

    let result: String = out.into_iter().collect();

    // Drop empty bracket pairs that the collapse left as "()" or "[]".
    result
        .replace("( )", "")
        .replace("[ ]", "")
        .replace("{ }", "")
}

/// Map each selected scene's current resolution token to the count of scenes
/// at that token, for the dialog's breakdown ("847 at 2160p, 12 at 1440p").
/// Input is an iterator of `(scene_id, optional height)`; scenes with no known
/// height are bucketed under "unknown".
pub fn bucket_by_token<S>(heights: &[(S, Option<u32>)]) -> BTreeMap<String, u64>
where
    S: AsRef<str>,
{
    let mut buckets: BTreeMap<String, u64> = BTreeMap::new();
    for (_id, h) in heights {
        let key = h
            .map(canonical_token)
            .map(str::to_string)
            .unwrap_or_else(|| "unknown".to_string());
        *buckets.entry(key).or_default() += 1;
    }
    buckets
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- canonical_token / detection --------------------------------------

    #[test]
    fn canonical_tokens() {
        assert_eq!(canonical_token(2160), "4K");
        assert_eq!(canonical_token(1080), "1080p");
        assert_eq!(canonical_token(720), "720p");
        assert_eq!(canonical_token(1440), "1440p");
    }

    #[test]
    fn detects_token_in_filename() {
        assert_eq!(detect_token_height("Scene 4K.mp4"), Some(2160));
        assert_eq!(detect_token_height("Movie.2160p.UHD.mkv"), Some(2160));
        assert_eq!(detect_token_height("Plain Title.mp4"), None);
        assert_eq!(detect_token_height("Trailer-720p.mp4"), Some(720));
    }

    #[test]
    fn is_token_predicate() {
        assert!(is_resolution_token("4K"));
        assert!(is_resolution_token("uhd"));
        assert!(is_resolution_token("2160p"));
        assert!(!is_resolution_token("4K Movies")); // not standalone
        assert!(!is_resolution_token("threesome"));
    }

    // --- Replace mode ------------------------------------------------------

    #[test]
    fn replace_basic_4k_to_1080p() {
        assert_eq!(
            rewrite_resolution_token("Scene 4K.mp4", RewriteMode::Replace, 1080),
            "Scene 1080p.mp4"
        );
    }

    #[test]
    fn replace_preserves_uppercase_style() {
        // "UHD" is all-caps → replacement becomes all-caps token form.
        assert_eq!(
            rewrite_resolution_token("UHD - Title.mp4", RewriteMode::Replace, 1080),
            "1080P - Title.mp4"
        );
    }

    #[test]
    fn replace_2160p_token() {
        assert_eq!(
            rewrite_resolution_token("Movie.2160p.WEB.mkv", RewriteMode::Replace, 1080),
            "Movie.1080p.WEB.mkv"
        );
    }

    #[test]
    fn replace_preserves_extension_and_multiple_tokens() {
        // Both 4K and UHD refer to 2160; both get replaced.
        let out = rewrite_resolution_token("Clip [4K] UHD.mp4", RewriteMode::Replace, 1080);
        assert_eq!(out, "Clip [1080p] 1080P.mp4");
    }

    // --- Remove mode -------------------------------------------------------

    #[test]
    fn remove_strips_and_collapses() {
        assert_eq!(
            rewrite_resolution_token("Scene 4K.mp4", RewriteMode::Remove, 1080),
            "Scene.mp4"
        );
    }

    #[test]
    fn remove_dotted_token() {
        assert_eq!(
            rewrite_resolution_token("Movie.2160p.WEB.mkv", RewriteMode::Remove, 1080),
            "Movie WEB.mkv"
        );
    }

    #[test]
    fn remove_leaves_extension_intact() {
        let out = rewrite_resolution_token("Trailer-720p.mp4", RewriteMode::Remove, 480);
        assert_eq!(out, "Trailer.mp4");
    }

    // --- Leave mode + edge cases ------------------------------------------

    #[test]
    fn leave_is_noop() {
        assert_eq!(
            rewrite_resolution_token("Scene 4K.mp4", RewriteMode::Leave, 1080),
            "Scene 4K.mp4"
        );
    }

    #[test]
    fn no_extension_handled() {
        assert_eq!(
            rewrite_resolution_token("Scene 4K", RewriteMode::Replace, 1080),
            "Scene 1080p"
        );
    }

    #[test]
    fn no_token_passes_through_unchanged() {
        assert_eq!(
            rewrite_resolution_token("Plain Title.mp4", RewriteMode::Replace, 1080),
            "Plain Title.mp4"
        );
        assert_eq!(
            rewrite_resolution_token("Plain Title.mp4", RewriteMode::Remove, 1080),
            "Plain Title.mp4"
        );
    }

    #[test]
    fn idempotent_after_replace() {
        // Replacing on something already at target should not double-process
        // because there is no higher-res token to match.
        let once = rewrite_resolution_token("Scene 1080p.mp4", RewriteMode::Replace, 1080);
        let twice = rewrite_resolution_token(&once, RewriteMode::Replace, 1080);
        assert_eq!(once, twice);
        assert_eq!(once, "Scene 1080p.mp4");
    }

    #[test]
    fn performer_name_with_digits_not_mangled() {
        // "Sasha2160" is a single alphanumeric word that is NOT in the token
        // table (the table has "2160" but as a standalone word), so it stays.
        assert_eq!(
            rewrite_resolution_token("Sasha2160 - Scene.mp4", RewriteMode::Replace, 1080),
            "Sasha2160 - Scene.mp4"
        );
    }
}
