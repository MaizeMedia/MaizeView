//! Shared filename / path parsing for titles, identify text search, and path meta.
//!
//! Tuned against real-world filename samples (2026-07). Parse only — never moves files (ADR-013).
//!
//! Kind-specific token rules:
//! - Quality / encode tokens are stripped from **titles** and identify queries.
//! - Act / grouping / resolution tokens (`anal`, `1on1`, `1080p`, …) are **tag candidates**.
//! - They must not be treated as studio names.

use std::path::{Component, Path};

/// Encode / container noise — never useful as title or studio.
const TITLE_NOISE: &[&str] = &[
    "1080p", "720p", "540p", "480p", "360p", "2160p", "1440p", "4320p", "4k", "8k", "uhd", "fhd",
    "hd", "sd", "hq", "lq", "hdr", "sdr", "h264", "h265", "x264", "x265", "hevc", "avc", "webrip",
    "webdl", "bluray", "bdrip", "dvdrip", "hdtv", "remux", "proper", "repack", "internal", "xxx",
    "adult", "porn", "mp4", "mkv", "avi", "wmv", "mov", "m4v", "h265", "xvid", "aac", "ac3",
];

/// Useful as tags; strip from titles/identify; never treat as studio.
const TAG_CANDIDATE_TOKENS: &[&str] = &[
    "1080p",
    "720p",
    "2160p",
    "1440p",
    "4k",
    "8k",
    "uhd",
    "hdr",
    "anal",
    "rim",
    "dp",
    "dap",
    "atm",
    "a2m",
    "gape",
    "creampie",
    "facial",
    "blowjob",
    "handjob",
    "pov",
    "hardcore",
    "softcore",
    "lesbian",
    "solo",
    "threesome",
    "foursome",
    "gangbang",
    "orgy",
    "bj",
    "hj",
    "1on1",
    "2on1",
    "3on1",
    "ffm",
    "mmf",
    "ff",
    "mm",
];

/// Folder segments that are media-type buckets, not studio/performer names.
const MEDIA_BUCKETS: &[&str] = &[
    "videos",
    "video",
    "movies",
    "movie",
    "clips",
    "clip",
    "media",
    "files",
    "film",
    "films",
    "scenes",
    "scene",
    "downloads",
    "download",
    "torrent",
    "torrents",
    "new",
    "misc",
    "other",
    "temp",
    "tmp",
    "incoming",
    "complete",
    "sorted",
    "unsorted",
    "library",
    "lib",
    "collection",
    "collections",
];

/// Bracket contents that are not studio names (pixel sizes, bare years, acts).
const BRACKET_NOT_STUDIO: &[&str] = &[
    "anal",
    "rim",
    "facial",
    "creampie",
    "dp",
    "pov",
    "hardcore",
    "softcore",
    "lesbian",
    "solo",
    "threesome",
    "1on1",
    "2on1",
    "3on1",
    "ffm",
    "mmf",
    "xxx",
    "hd",
    "sd",
    "4k",
    "uhd",
    "720p",
    "1080p",
    "2160p",
];

const JUNK_TITLES: &[&str] = &[
    "video",
    "movie",
    "untitled",
    "scene",
    "file",
    "download",
    "new",
    "temp",
    "copy",
    "image",
    "screen",
    "recording",
    "sample",
    "trailer",
    "clip",
    "full",
    "part",
    "complete",
    "final",
    "output",
    "export",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderEntityKind {
    Studio,
    Performer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderEntityHint {
    pub kind: FolderEntityKind,
    pub name: String,
}

/// Clean display title from a raw stem / title string. `None` if too weak to show.
pub fn cleaned_display_title(raw: &str) -> Option<String> {
    let extracted = extract_title_text(raw)?;
    if is_worth_as_title(&extracted) {
        Some(extracted)
    } else {
        None
    }
}

/// Best title for a video path: cleaned stem, else ancestor folder (+ stem if any).
pub fn scene_title_from_path(path: &Path) -> Option<String> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;

    if let Some(t) = cleaned_display_title(stem) {
        return Some(t);
    }

    // Weak stem (e.g. "876"): folder + original stem so peers stay distinguishable
    // ("Example Series PL - 876", not 100× the same folder title).
    for folder in ancestor_folder_names(path).into_iter().rev() {
        if is_media_bucket(&folder) {
            continue;
        }
        if looks_like_entity_folder(&folder) {
            if stem_is_pure_id(stem) {
                return Some(format!("{folder} - {stem}"));
            }
            let soft = soft_title(stem);
            if !soft.is_empty() {
                return Some(format!("{folder} - {soft}"));
            }
            return Some(format!("{folder} - {stem}"));
        }
    }
    None
}

/// Extract a stash-box text-search term from a raw title or filename stem.
pub fn identify_search_term(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Short / leading product codes first (REAL-876) before title cleanup mangles them.
    if let Some(code) = extract_product_code(trimmed) {
        let upper = trimmed.to_uppercase().replace(' ', "-");
        if upper == code
            || upper.starts_with(&(code.clone() + "-"))
            || trimmed.len() <= code.len() + 2
        {
            return Some(code);
        }
    }

    // Prefer a "Name - …" segment (performer) over later noisy title segments.
    if let Some(best) = best_dash_segment(trimmed) {
        if is_worth_as_search(&best) {
            return Some(best);
        }
    }

    if let Some(text) = extract_title_text(trimmed) {
        if is_worth_as_search(&text) {
            return Some(text);
        }
    }

    None
}

pub fn weak_identify_reason(raw: &str) -> &'static str {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "empty title";
    }
    if stem_is_pure_id(trimmed) {
        return "numeric/id-only — not specific enough for text search";
    }
    if identify_search_term(trimmed).is_some() {
        return "not specific enough for text search";
    }
    "title too weak for text search (needs a product code, multiple words, or a longer name)"
}

/// Tag-like tokens found in a stem (for path-meta suggestions).
pub fn tag_candidates_from_stem(stem: &str) -> Vec<String> {
    let mut out = Vec::new();
    let lower = stem.to_lowercase();
    for tok in tokenize_alnum(&lower) {
        let t = normalize_tag_token(&tok);
        if TAG_CANDIDATE_TOKENS.contains(&t.as_str()) && !out.iter().any(|x| x == &t) {
            out.push(t);
        }
    }
    // Bracket acts: [Anal], [Anal, Facial]
    for b in extract_brackets(stem) {
        for part in b.split(|c| c == ',' || c == '/' || c == ';') {
            let n = normalize_tag_token(part.trim());
            if n.is_empty() {
                continue;
            }
            if (TAG_CANDIDATE_TOKENS.contains(&n.as_str()) || is_act_like_tag(&n))
                && !out.iter().any(|x| x == &n)
            {
                out.push(n);
            }
        }
    }
    out
}

/// Ancestor folder names that look like studio/performer (skips media buckets).
pub fn folder_entity_hints(path: &Path) -> Vec<FolderEntityHint> {
    let mut out = Vec::new();
    for name in ancestor_folder_names(path) {
        if is_media_bucket(&name) {
            continue;
        }
        if !looks_like_entity_folder(&name) {
            continue;
        }
        // Skip drive / root-ish single letters
        if name.len() < 3 {
            continue;
        }
        let kind = infer_folder_kind(&name);
        if out
            .iter()
            .any(|h: &FolderEntityHint| h.name.eq_ignore_ascii_case(&name) && h.kind == kind)
        {
            continue;
        }
        out.push(FolderEntityHint { kind, name });
    }
    out
}

pub fn is_media_bucket(name: &str) -> bool {
    let n = normalize_for_tokens(name);
    MEDIA_BUCKETS.contains(&n.as_str())
}

fn infer_folder_kind(name: &str) -> FolderEntityKind {
    let words: Vec<&str> = name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| w.len() >= 2)
        .collect();
    // "First Last" / "Jada Fire" → performer; single token or 3+ → studio-ish.
    if words.len() == 2
        && words
            .iter()
            .all(|w| w.chars().next().is_some_and(|c| c.is_ascii_alphabetic()))
        && words
            .iter()
            .all(|w| w.chars().all(|c| c.is_ascii_alphabetic()))
    {
        FolderEntityKind::Performer
    } else {
        FolderEntityKind::Studio
    }
}

fn looks_like_entity_folder(name: &str) -> bool {
    let n = normalize_for_tokens(name);
    if n.is_empty() || is_media_bucket(&n) {
        return false;
    }
    if JUNK_TITLES.contains(&n.as_str()) {
        return false;
    }
    // Pure year / pure digits
    if n.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let letters = n.chars().filter(|c| c.is_ascii_alphabetic()).count();
    letters >= 3
}

fn ancestor_folder_names(path: &Path) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(parent) = path.parent() {
        for c in parent.components() {
            if let Component::Normal(os) = c {
                if let Some(s) = os.to_str() {
                    let t = s.trim();
                    if !t.is_empty() {
                        names.push(t.to_string());
                    }
                }
            }
        }
    }
    // Nearest parent first for title fallback; for hints we want all (outer→inner).
    // folder_entity_hints iterates as collected (root → leaf). Reverse so nearest first
    // for title_from_path; for hints keep root→leaf so studio tree works.
    names
}

fn stem_is_pure_id(raw: &str) -> bool {
    let tokens = tokenize_alnum(&raw.to_lowercase())
        .into_iter()
        .filter(|t| !is_title_noise(t) && !is_bitrate_token(t))
        .collect::<Vec<_>>();
    !tokens.is_empty()
        && tokens
            .iter()
            .all(|t| t.chars().all(|c| c.is_ascii_digit()) || is_title_noise(t))
}

fn soft_title(raw: &str) -> String {
    tokenize_alnum(raw)
        .into_iter()
        .filter(|t| !is_title_noise(t) && !is_bitrate_token(t))
        .collect::<Vec<_>>()
        .join(" ")
}

fn extract_title_text(raw: &str) -> Option<String> {
    let mut s = raw.to_string();
    // Drop bracket groups from title text (kept separately for tags/sites).
    for b in extract_brackets(raw) {
        let pattern = format!("[{b}]");
        s = s.replace(&pattern, " ");
        s = s.replace(&format!("[{}]", b.to_uppercase()), " ");
    }
    // Also remove any remaining [...] via char scan
    s = strip_brackets(&s);

    let tokens: Vec<String> = tokenize_alnum(&s.to_lowercase())
        .into_iter()
        .filter(|t| !is_title_noise(t) && !is_bitrate_token(t))
        .filter(|t| !is_date_token(t))
        .collect();

    if tokens.is_empty() {
        return None;
    }

    // Rebuild with original-ish casing from soft split of stripped string
    let stripped = strip_brackets(raw);
    let words: Vec<&str> = stripped
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .filter(|w| {
            let l = w.to_lowercase();
            !is_title_noise(&l) && !is_bitrate_token(&l) && !is_date_token(&l)
        })
        .collect();

    let joined = words.join(" ");
    let trimmed = joined.split_whitespace().collect::<Vec<_>>().join(" ");
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn best_dash_segment(raw: &str) -> Option<String> {
    let parts: Vec<&str> = raw
        .split(" - ")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() < 2 {
        return None;
    }
    // Score each segment; skip pure bracket / date / site.com segments.
    // Prefer early digit-free name segments ("Robin Monroe") over later titles
    // that embed scene IDs (GP1458, …).
    let mut best: Option<(usize, String)> = None;
    for (idx, p) in parts.iter().enumerate() {
        if p.starts_with('[') && p.ends_with(']') {
            continue;
        }
        if looks_like_site(p) {
            continue;
        }
        if let Some(text) = extract_title_text(p) {
            let score = search_score(&text, idx);
            if score > 0 && best.as_ref().map(|(s, _)| score > *s).unwrap_or(true) {
                best = Some((score, text));
            }
        }
    }
    best.map(|(_, t)| t)
}

fn search_score(text: &str, segment_index: usize) -> usize {
    let words: Vec<&str> = text
        .split_whitespace()
        .filter(|w| w.chars().all(|c| c.is_ascii_alphabetic()) && w.len() >= 3)
        .collect();
    if words.is_empty() {
        return 0;
    }
    let letter_count = text.chars().filter(|c| c.is_ascii_alphabetic()).count();
    let has_digits = text.chars().any(|c| c.is_ascii_digit());
    let mut score = words.len() * 10 + letter_count;
    if !has_digits {
        score += 80;
    }
    // Two-word person names
    if words.len() == 2 {
        score += 40;
    }
    // Prefer earlier segments
    score += (8usize.saturating_sub(segment_index)) * 5;
    score
}

fn is_worth_as_title(text: &str) -> bool {
    is_worth_as_search(text)
}

fn is_worth_as_search(text: &str) -> bool {
    let lower = text.to_lowercase();
    if JUNK_TITLES.contains(&lower.as_str()) {
        return false;
    }
    if extract_product_code(text).is_some() {
        return true;
    }
    let letter_words: Vec<&str> = text
        .split_whitespace()
        .filter(|w| w.chars().all(|c| c.is_ascii_alphabetic()) && w.len() >= 3)
        .collect();
    let letter_count = text.chars().filter(|c| c.is_ascii_alphabetic()).count();
    let digit_count = text.chars().filter(|c| c.is_ascii_digit()).count();
    if letter_count == 0 {
        return false;
    }
    if letter_words.len() >= 2 {
        return true;
    }
    if let Some(w) = letter_words.first() {
        return w.len() >= 6 && letter_count >= digit_count && letter_count >= 6;
    }
    false
}

fn extract_product_code(raw: &str) -> Option<String> {
    let tokens = tokenize_alnum(&raw.to_lowercase());
    for t in &tokens {
        if is_product_code_token(t) {
            return Some(t.to_uppercase());
        }
    }
    for w in tokens.windows(2) {
        if (2..=8).contains(&w[0].len())
            && w[0].chars().all(|c| c.is_ascii_alphabetic())
            && (2..=6).contains(&w[1].len())
            && w[1].chars().all(|c| c.is_ascii_digit())
        {
            return Some(format!("{}-{}", w[0].to_uppercase(), w[1]));
        }
    }
    None
}

fn is_product_code_token(t: &str) -> bool {
    let bytes = t.as_bytes();
    if bytes.len() < 4 {
        return false;
    }
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
        i += 1;
    }
    let letters = i;
    if !(2..=8).contains(&letters) {
        return false;
    }
    let digits = bytes.len() - i;
    if !(2..=6).contains(&digits) {
        return false;
    }
    bytes[i..].iter().all(|b| b.is_ascii_digit())
}

fn is_title_noise(t: &str) -> bool {
    // Acts like "anal" stay in titles ("Anal Loving…"); they are tag candidates separately.
    TITLE_NOISE.contains(&t)
}

fn is_bitrate_token(t: &str) -> bool {
    t.len() >= 4 && t.ends_with('k') && t[..t.len() - 1].chars().all(|c| c.is_ascii_digit())
}

fn is_date_token(t: &str) -> bool {
    // 2016, 2024, 160912-ish short dates as lone tokens
    if t.len() == 4 && t.chars().all(|c| c.is_ascii_digit()) {
        let y: u32 = t.parse().unwrap_or(0);
        return (1970..=2100).contains(&y);
    }
    if t.len() == 6 && t.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    false
}

fn looks_like_site(s: &str) -> bool {
    let l = s.to_lowercase();
    l.contains(".com") || l.contains(".net") || l.contains(".xxx") || l.starts_with("www")
}

fn is_act_like_tag(n: &str) -> bool {
    TAG_CANDIDATE_TOKENS.contains(&n) || BRACKET_NOT_STUDIO.contains(&n)
}

fn normalize_tag_token(t: &str) -> String {
    t.to_lowercase().replace([' ', '_', '-'], "")
}

fn normalize_for_tokens(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn tokenize_alnum(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            cur.push(ch.to_ascii_lowercase());
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn extract_brackets(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            if let Some(end) = raw[i + 1..].find(']') {
                let inner = raw[i + 1..i + 1 + end].trim();
                if !inner.is_empty() {
                    out.push(inner.to_string());
                }
                i += 1 + end + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn strip_brackets(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut depth = 0i32;
    for ch in raw.chars() {
        match ch {
            '[' => depth += 1,
            ']' => depth = (depth - 1).max(0),
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    out
}

/// Bracket content that may be a studio site name (not act/resolution/year).
pub fn bracket_studio_candidates(stem: &str) -> Vec<String> {
    let mut out = Vec::new();
    for b in extract_brackets(stem) {
        let base = b.split('-').next().unwrap_or(&b).trim();
        let key = normalize_tag_token(base);
        if key.is_empty() {
            continue;
        }
        if BRACKET_NOT_STUDIO.contains(&key.as_str())
            || TAG_CANDIDATE_TOKENS.contains(&key.as_str())
        {
            continue;
        }
        if key.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        // pixel size 1920x1080
        if key.contains('x')
            && key
                .split('x')
                .all(|p| p.chars().all(|c| c.is_ascii_digit()))
        {
            continue;
        }
        if key.chars().filter(|c| c.is_ascii_alphabetic()).count() < 3 {
            continue;
        }
        // Prefer readable name: strip .com
        let display = base
            .trim()
            .trim_end_matches(".com")
            .trim_end_matches(".net")
            .trim()
            .to_string();
        if !display.is_empty()
            && !out
                .iter()
                .any(|x: &String| x.eq_ignore_ascii_case(&display))
        {
            out.push(display);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn cleans_real_world_titles() {
        assert_eq!(
            cleaned_display_title(
                "Robin Monroe - [ExampleStudio.com] - [2019] - Anal Loving Example Title - 4K"
            )
            .as_deref(),
            Some("Robin Monroe Anal Loving Example Title")
        );
        // "Anal" stays in the title; 4K stripped as encode noise.
        assert!(cleaned_display_title("876").is_none());
        assert!(cleaned_display_title("18516_01_1080p").is_none());
        assert_eq!(
            cleaned_display_title("Robin Monroe").as_deref(),
            Some("Robin Monroe")
        );
    }

    #[test]
    fn title_from_path_uses_ancestor_for_numeric_stem() {
        let p = PathBuf::from(r"G:\Example Series PL\videos\876.mp4");
        let t = scene_title_from_path(&p).expect("title");
        assert_eq!(t, "Example Series PL - 876");
    }

    #[test]
    fn identify_extracts_performer_segment() {
        let t = identify_search_term(
            "Robin Monroe - [ExampleStudio.com] - [2020] - GP1458, A Generic Scene Title [Anal]",
        )
        .expect("term");
        assert!(t.to_lowercase().contains("robin"), "{t}");
    }

    #[test]
    fn identify_product_code() {
        assert_eq!(
            identify_search_term("REAL-876").as_deref(),
            Some("REAL-876")
        );
        assert!(identify_search_term("876").is_none());
    }

    #[test]
    fn tag_candidates_include_acts_and_resolution() {
        let tags = tag_candidates_from_stem("[ExampleStudio] - scene [Anal, Facial] 1080p");
        assert!(tags.iter().any(|t| t == "anal"));
        assert!(tags.iter().any(|t| t == "facial" || t == "1080p"));
    }

    #[test]
    fn folder_hints_skip_videos_bucket() {
        let p = PathBuf::from(r"E:\Sorted\Robin Monroe\videos\scene.mp4");
        let hints = folder_entity_hints(&p);
        assert!(
            hints
                .iter()
                .any(|h| h.name == "Robin Monroe" && h.kind == FolderEntityKind::Performer),
            "{hints:?}"
        );
        assert!(!hints.iter().any(|h| h.name.eq_ignore_ascii_case("videos")));
    }

    #[test]
    fn folder_hints_studio_single_token() {
        let p = PathBuf::from(r"E:\Sorted\Lustre\some scene.mp4");
        let hints = folder_entity_hints(&p);
        assert!(hints
            .iter()
            .any(|h| h.name == "Lustre" && h.kind == FolderEntityKind::Studio));
    }

    #[test]
    fn bracket_studio_not_anal() {
        let s = bracket_studio_candidates("[Anal] The Flying Nurses");
        assert!(!s.iter().any(|x| x.eq_ignore_ascii_case("Anal")));
        let s2 = bracket_studio_candidates("[Lustre] - Robin Monroe");
        assert!(s2.iter().any(|x| x == "Lustre"));
    }
}
