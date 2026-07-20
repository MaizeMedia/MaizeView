//! StashDB cover art download — stored under %LOCALAPPDATA%/MaizeView/covers/.

use std::path::{Path, PathBuf};

use reqwest::Client;

pub fn covers_dir() -> Result<PathBuf, String> {
    let base =
        dirs::data_local_dir().ok_or_else(|| "could not resolve local data dir".to_string())?;
    let dir = base.join("MaizeView").join("covers");
    std::fs::create_dir_all(&dir).map_err(|e| format!("creating covers dir: {e}"))?;
    Ok(dir)
}

/// Download a remote cover image for `scene_id`. Returns the absolute path on disk.
pub async fn download_cover(scene_id: &str, url: &str) -> Result<String, String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("empty cover URL".into());
    }

    let client = Client::builder()
        .user_agent("MaizeView/0.1 (local library)")
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("cover download failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("cover HTTP {}", resp.status()));
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("reading cover body: {e}"))?;

    if bytes.is_empty() {
        return Err("cover response was empty".into());
    }

    let ext = cover_extension_from_url(url);
    let dest = covers_dir()?.join(format!("{scene_id}.{ext}"));
    write_cover_file(&dest, &bytes)?;

    Ok(dest.to_string_lossy().into_owned())
}

fn cover_extension_from_url(url: &str) -> &'static str {
    let path = url.split('?').next().unwrap_or(url);
    match Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "png",
        Some("webp") => "webp",
        Some("jpeg") | Some("jpg") => "jpg",
        _ => "jpg",
    }
}

fn write_cover_file(dest: &Path, bytes: &[u8]) -> Result<(), String> {
    if dest.exists() {
        std::fs::remove_file(dest).map_err(|e| format!("replacing cover: {e}"))?;
    }
    std::fs::write(dest, bytes).map_err(|e| format!("writing cover: {e}"))
}
