//! ffprobe wrapper — extracts container/codec/duration/dimensions + format tags.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;

use crate::media_tools;

/// Cleaned probe result we persist into the `files` row (media fields)
/// plus optional embedded tags used for local enrichment.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct ProbeSummary {
    pub format_name: Option<String>,
    pub duration: Option<f64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub codec: Option<String>,
    pub fps: Option<f64>,
    pub bitrate: Option<i64>,
    /// Embedded `title` / `TITLE` from the container.
    pub title: Option<String>,
    /// Embedded `artist` / `ARTIST` (often performer names).
    pub artist: Option<String>,
    /// Embedded `comment` / `description` / `DESCRIPTION`.
    pub comment: Option<String>,
}

/// Subset of `ffprobe -show_format -show_streams` we actually use.
#[derive(Debug, Clone, serde::Deserialize)]
struct ProbeOutput {
    format: ProbeFormat,
    #[serde(default)]
    streams: Vec<ProbeStream>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct ProbeFormat {
    format_name: Option<String>,
    duration: Option<String>,
    bit_rate: Option<String>,
    #[serde(default)]
    tags: HashMap<String, String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct ProbeStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<i64>,
    height: Option<i64>,
    avg_frame_rate: Option<String>,
}

fn tag_ci<'a>(tags: &'a HashMap<String, String>, keys: &[&str]) -> Option<&'a str> {
    for want in keys {
        for (k, v) in tags {
            if k.eq_ignore_ascii_case(want) {
                let t = v.trim();
                if !t.is_empty() {
                    return Some(t);
                }
            }
        }
    }
    None
}

/// Probe a single video file. Returns an empty summary (not an error) if ffprobe
/// can't parse it — we still want to catalog such files.
pub fn probe(path: &Path) -> Result<ProbeSummary> {
    let bin = match media_tools::ffprobe_binary() {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(error = %e, "ffprobe unavailable; cataloging without media metadata");
            return Ok(ProbeSummary::default());
        }
    };

    let output = match media_tools::silent_output(
        std::process::Command::new(&bin)
            .args([
                "-v",
                "error",
                "-print_format",
                "json",
                "-show_format",
                "-show_streams",
            ])
            .arg(path),
    ) {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "ffprobe invocation failed");
            return Ok(ProbeSummary::default());
        }
    };

    if !output.status.success() {
        tracing::warn!(
            path = %path.display(),
            code = ?output.status.code(),
            stderr = String::from_utf8_lossy(&output.stderr).trim(),
            "ffprobe reported failure; cataloging without media metadata"
        );
        return Ok(ProbeSummary::default());
    }

    let parsed: ProbeOutput = match serde_json::from_slice(&output.stdout) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "ffprobe JSON parse failed");
            return Ok(ProbeSummary::default());
        }
    };

    let vstream = parsed
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("video"));

    let tags = &parsed.format.tags;
    Ok(ProbeSummary {
        format_name: parsed.format.format_name,
        duration: parsed.format.duration.and_then(|s| s.parse().ok()),
        width: vstream.and_then(|s| s.width),
        height: vstream.and_then(|s| s.height),
        codec: vstream.and_then(|s| s.codec_name.clone()),
        fps: vstream.and_then(|s| parse_fraction(&s.avg_frame_rate.clone().unwrap_or_default())),
        bitrate: parsed.format.bit_rate.and_then(|s| s.parse().ok()),
        title: tag_ci(tags, &["title"]).map(|s| s.to_string()),
        artist: tag_ci(tags, &["artist", "author", "performer"]).map(|s| s.to_string()),
        comment: tag_ci(tags, &["comment", "description", "synopsis"]).map(|s| s.to_string()),
    })
}

fn parse_fraction(s: &str) -> Option<f64> {
    let (num, den) = s.split_once('/')?;
    let num: f64 = num.parse().ok()?;
    let den: f64 = den.parse().ok()?;
    if den == 0.0 {
        return None;
    }
    Some(num / den)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_ci_finds_case_insensitive() {
        let mut tags = HashMap::new();
        tags.insert("TITLE".into(), "  Hello  ".into());
        assert_eq!(tag_ci(&tags, &["title"]), Some("Hello"));
    }
}
