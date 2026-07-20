//! Stash-box / StashDB GraphQL client (Phase 2 identify).
//!
//! Works with any stash-box instance (StashDB, ThePornDB, FansDB, JAVStash, …)
//! — same GraphQL schema and ApiKey header.

use serde::{Deserialize, Serialize};

const DEFAULT_ENDPOINT: &str = "https://stashdb.org/graphql";

/// Built-in stash-box endpoints the user can switch between in Settings.
#[derive(Debug, Clone, Serialize)]
pub struct StashBoxPreset {
    pub id: &'static str,
    pub name: &'static str,
    pub endpoint: &'static str,
    pub account_url: &'static str,
}

pub const STASH_BOX_PRESETS: &[StashBoxPreset] = &[
    StashBoxPreset {
        id: "stashdb",
        name: "StashDB",
        endpoint: "https://stashdb.org/graphql",
        account_url: "https://stashdb.org/users/me",
    },
    StashBoxPreset {
        id: "tpdb",
        name: "ThePornDB",
        endpoint: "https://theporndb.net/graphql",
        account_url: "https://theporndb.net/user/api-tokens",
    },
    StashBoxPreset {
        id: "fansdb",
        name: "FansDB",
        endpoint: "https://fansdb.cc/graphql",
        account_url: "https://fansdb.cc/",
    },
    StashBoxPreset {
        id: "javstash",
        name: "JAVStash",
        endpoint: "https://javstash.org/graphql",
        account_url: "https://javstash.org/",
    },
];

pub fn preset_by_id(id: &str) -> Option<&'static StashBoxPreset> {
    STASH_BOX_PRESETS.iter().find(|p| p.id == id)
}

pub fn provenance_for_endpoint(endpoint: &str) -> &'static str {
    let ep = endpoint.to_ascii_lowercase();
    for p in STASH_BOX_PRESETS {
        if ep.contains(&p.endpoint.replace("https://", "").replace("/graphql", "")) {
            return p.id;
        }
    }
    "stashbox"
}

const ME_QUERY: &str = r#"
query Me {
  me {
    name
  }
}
"#;

const FIND_BY_FP_QUERY: &str = r#"
query FindScenesBySceneFingerprints($fingerprints: [[FingerprintQueryInput!]!]!) {
  findScenesBySceneFingerprints(fingerprints: $fingerprints) {
    id
    title
    code
    details
    duration
    date
    studio { name }
    tags { name }
    performers { performer { name } }
    images { url width height }
  }
}
"#;

const SEARCH_SCENE_QUERY: &str = r#"
query SearchScene($term: String!) {
  searchScene(term: $term) {
    id
    title
    code
    details
    duration
    date
    studio { name }
    tags { name }
    performers { performer { name } }
    images { url width height }
  }
}
"#;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum FingerprintAlgorithm {
    Md5,
    Oshash,
    Phash,
}

#[derive(Debug, Clone, Serialize)]
pub struct FingerprintQueryInput {
    pub algorithm: FingerprintAlgorithm,
    pub hash: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StashDbImage {
    pub url: String,
    pub width: Option<i64>,
    pub height: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StashDbSceneMatch {
    pub id: String,
    pub title: Option<String>,
    pub code: Option<String>,
    pub details: Option<String>,
    pub duration: Option<i64>,
    pub date: Option<String>,
    pub studio: Option<StashDbStudio>,
    pub tags: Option<Vec<StashDbTag>>,
    pub performers: Option<Vec<StashDbPerformerAppearance>>,
    pub images: Option<Vec<StashDbImage>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StashDbStudio {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StashDbTag {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StashDbPerformerAppearance {
    pub performer: StashDbPerformer,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StashDbPerformer {
    pub name: String,
}

#[derive(Debug, Deserialize)]
struct GraphQlResponse {
    data: Option<FindData>,
    // Deserialized to keep the API shape honest; surfaced via data.find* only.
    #[allow(dead_code)]
    errors: Option<Vec<GraphQlError>>,
}

#[derive(Debug, Deserialize)]
struct FindData {
    #[serde(rename = "findScenesBySceneFingerprints")]
    find_scenes: Option<Vec<Vec<StashDbSceneMatch>>>,
}

#[derive(Debug, Deserialize)]
struct GraphQlError {
    message: String,
}

fn normalize_endpoint(endpoint: &str) -> &str {
    if endpoint.trim().is_empty() {
        DEFAULT_ENDPOINT
    } else {
        endpoint.trim()
    }
}

async fn post_graphql(
    endpoint: &str,
    api_key: &str,
    query: &str,
    variables: serde_json::Value,
) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .user_agent("MaizeView/0.1 (local library)")
        .build()
        .map_err(|e| e.to_string())?;

    let body = serde_json::json!({ "query": query, "variables": variables });

    let resp = client
        .post(normalize_endpoint(endpoint))
        .header("Content-Type", "application/json")
        .header("ApiKey", api_key.trim())
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("StashDB request failed: {e}"))?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("reading StashDB response: {e}"))?;

    if !status.is_success() {
        return Err(format!("StashDB HTTP {status}: {text}"));
    }

    Ok(text)
}

fn graphql_errors(text: &str) -> Result<(), String> {
    #[derive(Debug, Deserialize)]
    struct ErrResp {
        errors: Option<Vec<GraphQlError>>,
    }
    let parsed: ErrResp =
        serde_json::from_str(text).map_err(|e| format!("invalid StashDB JSON: {e}"))?;
    if let Some(errors) = parsed.errors {
        if !errors.is_empty() {
            let msg = errors
                .iter()
                .map(|e| e.message.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            return Err(format!("StashDB error: {msg}"));
        }
    }
    Ok(())
}

fn dedupe_scenes(mut scenes: Vec<StashDbSceneMatch>) -> Vec<StashDbSceneMatch> {
    let mut seen = std::collections::HashSet::new();
    scenes.retain(|s| seen.insert(s.id.clone()));
    scenes
}

/// Pick the largest StashDB image URL for use as a cover.
pub fn best_cover_url(scene: &StashDbSceneMatch) -> Option<String> {
    scene
        .images
        .as_ref()?
        .iter()
        .filter(|i| !i.url.trim().is_empty())
        .max_by_key(|i| i.width.unwrap_or(0))
        .map(|i| i.url.clone())
}

pub fn default_endpoint() -> &'static str {
    DEFAULT_ENDPOINT
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct StashDbTestResult {
    pub username: String,
}

#[derive(Debug, Deserialize)]
struct MeData {
    me: Option<MeUser>,
}

#[derive(Debug, Deserialize)]
struct MeUser {
    name: String,
}

/// Verify API key + endpoint by calling the authenticated `me` query.
pub async fn test_connection(endpoint: &str, api_key: &str) -> Result<StashDbTestResult, String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err("API key is required".into());
    }

    let endpoint = if endpoint.trim().is_empty() {
        DEFAULT_ENDPOINT
    } else {
        endpoint.trim()
    };

    let text = post_graphql(endpoint, key, ME_QUERY, serde_json::json!({})).await?;
    graphql_errors(&text)?;
    #[derive(Debug, Deserialize)]
    struct MeResponse {
        data: Option<MeData>,
        errors: Option<Vec<GraphQlError>>,
    }

    let me_parsed: MeResponse =
        serde_json::from_str(&text).map_err(|e| format!("invalid StashDB JSON: {e}"))?;

    if let Some(errors) = me_parsed.errors {
        if !errors.is_empty() {
            let msg = errors
                .iter()
                .map(|e| e.message.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            return Err(format!("StashDB error: {msg}"));
        }
    }

    let username = me_parsed
        .data
        .and_then(|d| d.me)
        .map(|u| u.name)
        .filter(|n| !n.trim().is_empty())
        .ok_or_else(|| "StashDB returned no user (check API key)".to_string())?;

    Ok(StashDbTestResult { username })
}

/// Query StashDB for scenes matching the given fingerprints (OSHASH / MD5).
pub async fn find_scenes_by_fingerprints(
    endpoint: &str,
    api_key: &str,
    fingerprints: Vec<FingerprintQueryInput>,
) -> Result<Vec<StashDbSceneMatch>, String> {
    if fingerprints.is_empty() {
        return Ok(vec![]);
    }

    let endpoint = normalize_endpoint(endpoint);

    let text = post_graphql(
        endpoint,
        api_key,
        FIND_BY_FP_QUERY,
        serde_json::json!({ "fingerprints": [fingerprints] }),
    )
    .await?;
    graphql_errors(&text)?;

    let parsed: GraphQlResponse =
        serde_json::from_str(&text).map_err(|e| format!("invalid StashDB JSON: {e}"))?;

    let scenes = parsed
        .data
        .and_then(|d| d.find_scenes)
        .unwrap_or_default()
        .into_iter()
        .flatten()
        .collect();

    Ok(dedupe_scenes(scenes))
}

/// Title/code search fallback when fingerprint matching finds nothing.
pub async fn search_scenes_by_term(
    endpoint: &str,
    api_key: &str,
    term: &str,
    limit: usize,
) -> Result<Vec<StashDbSceneMatch>, String> {
    let term = term.trim();
    if term.is_empty() {
        return Ok(vec![]);
    }

    #[derive(Debug, Deserialize)]
    struct SearchData {
        #[serde(rename = "searchScene")]
        search_scene: Option<Vec<StashDbSceneMatch>>,
    }
    #[derive(Debug, Deserialize)]
    struct SearchResp {
        data: Option<SearchData>,
    }

    let text = post_graphql(
        endpoint,
        api_key,
        SEARCH_SCENE_QUERY,
        serde_json::json!({ "term": term }),
    )
    .await?;
    graphql_errors(&text)?;

    let parsed: SearchResp =
        serde_json::from_str(&text).map_err(|e| format!("invalid StashDB JSON: {e}"))?;

    let mut scenes = parsed.data.and_then(|d| d.search_scene).unwrap_or_default();
    scenes.truncate(limit);
    Ok(dedupe_scenes(scenes))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NESTED_FP_RESPONSE: &str = r#"{"data":{"findScenesBySceneFingerprints":[[{"id":"abc123","title":"Test Scene","studio":{"name":"Studio A"},"tags":[{"name":"Tag1"}],"performers":[{"performer":{"name":"Actor"}}],"images":[{"url":"http://example/cover.jpg","width":640,"height":360}]}]]}}"#;

    #[test]
    fn parses_nested_fingerprint_match_response() {
        let parsed: GraphQlResponse =
            serde_json::from_str(NESTED_FP_RESPONSE).expect("nested StashDB response should parse");
        let groups = parsed.data.and_then(|d| d.find_scenes).unwrap_or_default();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 1);
        assert_eq!(groups[0][0].id, "abc123");
        assert_eq!(groups[0][0].title.as_deref(), Some("Test Scene"));
    }

    #[test]
    fn flat_vec_fails_on_nested_response() {
        #[derive(Debug, Deserialize)]
        struct BadFindData {
            #[serde(rename = "findScenesBySceneFingerprints")]
            find_scenes: Option<Vec<StashDbSceneMatch>>,
        }
        #[derive(Debug, Deserialize)]
        struct BadResp {
            data: Option<BadFindData>,
        }
        let err = serde_json::from_str::<BadResp>(NESTED_FP_RESPONSE).unwrap_err();
        assert!(
            err.to_string().contains("expected a string"),
            "old flat parser should fail: {err}"
        );
    }
}
