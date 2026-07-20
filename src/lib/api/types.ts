// Shared types mirroring the Rust models in src-tauri/src/models.rs and the
// command signatures in src-tauri/src/commands/. Keep these in sync.

export interface ScanPath {
  id: string;
  path: string;
  label: string | null;
  created_at: string;
  /** False when the folder is missing (drive offline, path moved, etc.). */
  accessible?: boolean;
}

export interface SceneGridRow {
  id: string;
  title: string | null;
  /** Favorite level 0..5 (0 = not favorited). */
  favorite: number;
  rating: number | null;
  play_count: number;
  created_at: string;
  // representative file:
  duration: number | null;
  width: number | null;
  height: number | null;
  thumb_path: string | null;
  thumb_sprite_path: string | null;
  file_path: string | null;
}

export interface Scene {
  id: string;
  title: string | null;
  details: string | null;
  title_source: string;
  details_source: string;
  studio_id: string | null;
  cover_path: string | null;
  cover_source: string;
  rating: number | null;
  /** Favorite level 0..5 (0 = not favorited). */
  favorite: number;
  play_count: number;
  last_played_at: string | null;
  last_position: number | null;
  stashdb_checked_at: string | null;
  stashdb_match_count: number | null;
  stashdb_applied_at: string | null;
  stashdb_ignored_at: string | null;
  stashdb_remote_id: string | null;
  created_at: string;
  updated_at: string;
}

export interface VideoFile {
  id: string;
  scene_id: string;
  path: string;
  size_bytes: number;
  modified_at: string;
  format_name: string | null;
  duration: number | null;
  width: number | null;
  height: number | null;
  codec: string | null;
  fps: number | null;
  bitrate: number | null;
  thumb_path: string | null;
  thumb_sprite_path: string | null;
  vtt_path: string | null;
  scanned_at: string;
}

export interface SceneDetail {
  scene: Scene;
  files: VideoFile[];
  performers: PerformerRow[];
  studio: StudioRow | null;
  tags: TagRow[];
}

export interface PerformerRow {
  id: string;
  name: string;
}

export interface StudioRow {
  id: string;
  name: string;
}

export interface TagRow {
  id: string;
  name: string;
  color: string | null;
}

export interface Counts {
  total: number;
  favorites: number;
}

export type ScanStatus = "running" | "completed" | "failed" | "cancelled";
export type ScanPhase = "walking" | "indexing" | "writing" | "done";

export interface ScanProgress {
  scan_run_id: string;
  status: ScanStatus;
  phase: ScanPhase;
  files_found: number;
  files_added: number;
  files_updated: number;
  files_removed: number;
  files_processed: number;
  current_path: string | null;
  skipped_paths?: string[] | null;
}
