//! Tauri commands — the API surface exposed to the frontend.
//!
//! Each submodule groups related commands. Signatures follow Tauri's
//! convention: `#[tauri::command] async fn name(state: State<AppState>, ...) -> Result<T, String>`.

pub mod batch_identify;
pub mod duplicates;
pub mod embedded_meta;
pub mod fingerprints;
pub mod identify;
pub mod metadata;
pub mod path_meta;
pub mod player;
pub mod playlists;
pub mod previews;
pub mod quad;
pub mod saved_filters;
pub mod scan;
pub mod scan_paths;
pub mod scenes;
pub mod segments;
pub mod settings;
pub mod stash_import;
pub mod transcode;

/// Convenience: convert anyhow errors into the String Tauri expects.
pub fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}
