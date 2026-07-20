//! Stash-box title-search worthiness / term extraction.
//!
//! Delegates to [`crate::filename_parse`] (shared with scan titles + path meta).

use crate::filename_parse;

/// If `raw` yields a usable stash-box text query, return that term.
pub fn usable_title_search_term(raw: &str) -> Option<String> {
    filename_parse::identify_search_term(raw)
}

/// Human-readable reason when [`usable_title_search_term`] returns `None`.
pub fn weak_title_reason(raw: &str) -> &'static str {
    filename_parse::weak_identify_reason(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(s: &str) {
        assert!(
            usable_title_search_term(s).is_some(),
            "expected ACCEPT: {s:?}"
        );
    }

    fn no(s: &str) {
        assert!(
            usable_title_search_term(s).is_none(),
            "expected REJECT: {s:?} (reason: {})",
            weak_title_reason(s)
        );
    }

    #[test]
    fn rejects_numeric_and_site_ids() {
        no("876");
        no("1147");
        no("01");
        no("18516_01_1080p");
        no("1080P_4000K_147002042");
    }

    #[test]
    fn rejects_short_or_junk() {
        no("Eva");
        no("Nancy");
        no("video");
        no("Untitled");
        no("");
    }

    #[test]
    fn accepts_product_codes_and_names() {
        ok("REAL-876");
        ok("SSIS-001");
        ok("Robin Monroe");
        ok("Aveline");
        ok("137_29.07.11_Aveline Cross");
    }

    #[test]
    fn extracts_from_noisy_real_world_stems() {
        let t = usable_title_search_term(
            "Robin Monroe - [ExampleStudio.com] - [2020] - A Generic Scene Title [Anal]",
        )
        .expect("term");
        assert!(t.to_lowercase().contains("robin"), "{t}");
    }
}
