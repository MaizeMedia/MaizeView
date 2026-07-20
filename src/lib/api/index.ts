// Typed wrappers around Tauri invoke() + event listening.
// The UI should import from here, never call invoke() with raw strings.

import { invoke } from "@tauri-apps/api/core";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type {
  Counts,
  PerformerRow,
  ScanPath,
  SceneDetail,
  SceneGridRow,
  ScanProgress,
  StudioRow,
  TagRow,
} from "./types";

// ─── dialog ──────────────────────────────────────────────────────────────
/** Open the native OS folder picker. Returns the chosen path or null. */
export async function pickFolder(): Promise<string | null> {
  const selected = await open({ directory: true, multiple: false });
  if (Array.isArray(selected)) return selected[0] ?? null;
  return selected;
}

/** Pick a Stash `stash-go.sqlite` (or any .sqlite) file. */
export async function pickSqliteFile(): Promise<string | null> {
  const selected = await open({
    multiple: false,
    filters: [{ name: "SQLite", extensions: ["sqlite", "db"] }],
  });
  if (Array.isArray(selected)) return selected[0] ?? null;
  return selected;
}

// ─── player (Phase 3/4, libmpv-backed) ───────────────────────────────────
export {
  openPlayerWindow,
  closeAllPlayerWindows,
  createPlayer,
  claimPlayerQueue,
  stagePlayerQueue,
  sceneFilePath,
  DEFAULT_MPV_CONFIG,
  PLAYER_OBSERVED_PROPERTIES,
  type OpenPlayerOpts,
  type PlayerHandle,
  type PlayerObservedEvent,
  type StagedQueue,
  sceneScrubPreview,
  type SceneScrubPreview,
  openQuadWindow,
  openQuadWithScenes,
  type OpenQuadOpts,
} from "./player";

// ─── assets ─────────────────────────────────────────────────────────────
/**
 * Convert an absolute filesystem path (e.g. a thumbnail sprite stored under
 * %LOCALDATA%/MaizeView/previews) into a URL the webview can load. Returns
 * null for null/empty input. Bare Windows paths (C:\...) are not loadable in
 * <img src> directly — they must go through the asset protocol.
 */
export function assetUrl(fsPath: string | null | undefined): string | null {
  if (!fsPath) return null;
  return convertFileSrc(fsPath);
}

// ─── scan paths ──────────────────────────────────────────────────────────
export const scanPaths = {
  list: () => invoke<ScanPath[]>("list_scan_paths"),
  add: (path: string, label?: string) =>
    invoke<ScanPath>("add_scan_path", { args: { path, label } }),
  remove: (id: string) => invoke<void>("remove_scan_path", { id }),
};

// ─── scenes ──────────────────────────────────────────────────────────────
export type SortBy = "created" | "favorite" | "playcount" | "title";

export interface ListScenesArgs {
  favoritesOnly?: boolean;
  minFavorite?: number; // 0..5
  search?: string;
  searchInverse?: boolean;
  searchExcludeTerms?: string[];
  sort?: SortBy;
  tagIds?: string[];
  tagMatchAny?: boolean;
  excludeTagIds?: string[];
  performerIds?: string[];
  excludePerformerIds?: string[];
  studioIds?: string[];
  excludeStudioIds?: string[];
  minDuration?: number; // seconds
  maxDuration?: number; // seconds
  unplayedOnly?: boolean;
  /** Require ≥ N tags (0/omit = off). */
  minTagCount?: number;
  /** @deprecated Prefer minTagCount: 1 */
  taggedOnly?: boolean;
  identifiedOnly?: boolean;
  /** Multiple stash-box matches, not yet applied. */
  needsReviewOnly?: boolean;
  /** Min primary-file height in pixels. */
  minHeight?: number;
  minPerformerCount?: number;
  limit?: number;
  offset?: number;
}

/** Snapshot stored in saved_filters.payload (JSON). */
export interface SavedFilterPayload {
  search: string;
  searchInverse: boolean;
  sort: SortBy;
  minFavorite: number;
  tagIds: string[];
  excludeTagIds: string[];
  tagMatchAny: boolean;
  performerIds: string[];
  excludePerformerIds: string[];
  studioIds: string[];
  excludeStudioIds: string[];
  minDurationMins: number | null;
  maxDurationMins: number | null;
  unplayedOnly: boolean;
  minTagCount: number;
  identifiedOnly: boolean;
  needsReviewOnly: boolean;
  minHeight: number | null;
  minPerformerCount: number;
}

export interface SavedFilterRow {
  id: string;
  name: string;
  payload: string;
  created_at: string;
  updated_at: string;
}

export interface SceneDeletedEvent {
  scene_id: string;
}

export interface DeleteScenesResult {
  deleted: number;
  failed: Array<{ scene_id: string; error: string }>;
}

export interface DeleteScenesResult {
  deleted: number;
  failed: Array<{ scene_id: string; error: string }>;
}

export interface ListScenesResult {
  scenes: SceneGridRow[];
  total: number;
  limit: number;
  offset: number;
}

/** Max scenes fetched per list request (local DB; thumbs load lazily in the grid). */
export const LIST_SCENES_BATCH = 10_000;

export const scenes = {
  list: (args: ListScenesArgs = {}) =>
    invoke<ListScenesResult>("list_scenes", {
      args: {
        favorites_only: args.favoritesOnly ?? false,
        min_favorite: args.minFavorite ?? null,
        search: args.search ?? null,
        search_inverse: args.searchInverse ?? false,
        search_exclude_terms: args.searchExcludeTerms ?? [],
        sort: args.sort ?? null,
        tag_ids: args.tagIds ?? [],
        tag_match_any: args.tagMatchAny ?? false,
        exclude_tag_ids: args.excludeTagIds ?? [],
        performer_ids: args.performerIds ?? [],
        exclude_performer_ids: args.excludePerformerIds ?? [],
        studio_ids: args.studioIds ?? [],
        exclude_studio_ids: args.excludeStudioIds ?? [],
        min_duration: args.minDuration ?? null,
        max_duration: args.maxDuration ?? null,
        unplayed_only: args.unplayedOnly ?? false,
        min_tag_count: args.minTagCount ?? null,
        tagged_only: args.taggedOnly ?? false,
        identified_only: args.identifiedOnly ?? false,
        needs_review_only: args.needsReviewOnly ?? false,
        min_height: args.minHeight ?? null,
        min_performer_count: args.minPerformerCount ?? null,
        limit: args.limit ?? LIST_SCENES_BATCH,
        offset: args.offset ?? 0,
      },
    }),
  counts: () => invoke<Counts>("scene_counts"),
  detail: (sceneId: string) =>
    invoke<SceneDetail>("scene_detail", { sceneId }),
  /** Set favorite level 0..5. */
  setFavorite: (sceneId: string, level: number) =>
    invoke<void>("set_favorite", { sceneId, level }),
  /** Bump play_count + last_played_at (player start). */
  recordPlay: (sceneId: string) =>
    invoke<void>("record_scene_play", { sceneId }),
  /** Favorite + last_played_at for shuffle weighting. */
  shuffleMeta: (sceneIds: string[]) =>
    invoke<{ id: string; favorite: number; last_played_at: string | null }[]>(
      "scenes_shuffle_meta",
      { sceneIds },
    ),
  /** Permanently delete scene and its files from disk. */
  delete: (sceneId: string) => invoke<void>("delete_scene", { sceneId }),
  /** Delete only the given scene IDs (deduped). */
  deleteMany: (sceneIds: string[]) =>
    invoke<DeleteScenesResult>("delete_scenes", { sceneIds }),
  /** Fired when a scene is removed (e.g. player delete). */
  onDeleted: (cb: (p: SceneDeletedEvent) => void): Promise<UnlistenFn> =>
    listen<SceneDeletedEvent>("scene://deleted", (e) => cb(e.payload)),
};

// ─── downscale / transcode ──────────────────────────────────────────────
/** What to do with the original after a successful transcode. */
export type OriginalMode = "replace" | "keep";
/** What to do with resolution tokens in filenames ("4K", "2160p", "UHD"). */
export type FilenameMode = "replace" | "remove" | "leave";
/** What to do with resolution tags on the scene. */
export type TagMode = "swap" | "remove" | "leave";

export interface DownscaleOptions {
  sceneIds: string[];
  targetHeight: number;
  originalMode: OriginalMode;
  filenameMode: FilenameMode;
  tagMode: TagMode;
}

export interface DownscalePreviewItem {
  sceneId: string;
  currentHeight: number | null;
  currentPath: string | null;
  wouldSkip: boolean;
  previewFilename: string | null;
}

export interface DownscalePreview {
  targetHeight: number;
  total: number;
  wouldTranscode: number;
  skipped: number;
  /** Bucket counts keyed by canonical resolution token ("4K", "1080p", …). */
  byResolution: Record<string, number>;
  /** Rough byte savings estimate; labelled "estimated" in the UI. */
  estimatedBytesSaved: number;
  items: DownscalePreviewItem[];
}

export interface TranscodeFailure {
  sceneId: string;
  reason: string;
}

export interface TranscodeProgress {
  done: number;
  total: number;
  currentScene: string | null;
  currentPath: string | null;
  skipped: number;
  encoder: string | null;
  /** Percent of the *current* file processed (0–100), when known. */
  filePercent: number | null;
  finished: boolean;
  failed: TranscodeFailure[];
}

export const transcode = {
  /** Non-mutating plan of what a downscale would do — feeds the dialog. */
  preview: (sceneIds: string[], targetHeight: number) =>
    invoke<DownscalePreview>("downscale_preview", { sceneIds, targetHeight }),
  /** Launch a downscale run in the background. Refuses if one is running. */
  start: (opts: DownscaleOptions) => invoke<boolean>("downscale_start", { opts }),
  /** Cancel the running transcode (no-op if none running). */
  cancel: () => invoke<boolean>("downscale_cancel"),
  /** Subscribe to transcode progress events. Returns an unlisten fn. */
  onProgress: (cb: (p: TranscodeProgress) => void): Promise<UnlistenFn> =>
    listen<TranscodeProgress>("transcode://progress", (e) => cb(e.payload)),
};

// ─── scan ────────────────────────────────────────────────────────────────
export const scan = {
  /** Starts a scan; resolves with the scan_run_id. */
  start: () => invoke<string>("start_scan"),
  /** Cancels the running scan (no-op if none running). */
  cancel: () => invoke<boolean>("cancel_scan"),

  /** Subscribe to scan progress events. Returns an unlisten fn. */
  onProgress: (cb: (p: ScanProgress) => void): Promise<UnlistenFn> =>
    listen<ScanProgress>("scan://progress", (e) => cb(e.payload)),
};

export interface PreviewProgress {
  done: number;
  total: number;
  current_path: string | null;
  finished: boolean;
  cancelled?: boolean;
}

export interface FingerprintProgress {
  done: number;
  total: number;
  current_path: string | null;
  finished: boolean;
  cancelled?: boolean;
}

export interface PhashProgress {
  done: number;
  total: number;
  current_path: string | null;
  finished: boolean;
  cancelled?: boolean;
}

export const previews = {
  /** Regenerate thumbnails for all files missing a grid thumb. */
  generate: () => invoke<void>("generate_previews"),
  cancel: () => invoke<boolean>("cancel_previews"),
  onProgress: (cb: (p: PreviewProgress) => void): Promise<UnlistenFn> =>
    listen<PreviewProgress>("preview://progress", (e) => cb(e.payload)),
};

export const fingerprints = {
  /** Compute MD5 for all files missing that fingerprint. */
  generateMd5: () => invoke<void>("generate_md5_fingerprints"),
  cancelMd5: () => invoke<boolean>("cancel_md5_fingerprints"),
  /** Compute pHash for files missing that fingerprint. Pass rebuild to wipe and recompute all. */
  generatePhash: (rebuild = false) =>
    invoke<void>("generate_phash_fingerprints", { rebuild }),
  cancelPhash: () => invoke<boolean>("cancel_phash_fingerprints"),
  /** Stop previews + MD5 + pHash (Settings or post-scan auto jobs). */
  cancelAllMediaJobs: () => invoke<boolean>("cancel_media_jobs"),
  onMd5Progress: (cb: (p: FingerprintProgress) => void): Promise<UnlistenFn> =>
    listen<FingerprintProgress>("fingerprint://progress", (e) => cb(e.payload)),
  onPhashProgress: (cb: (p: PhashProgress) => void): Promise<UnlistenFn> =>
    listen<PhashProgress>("phash://progress", (e) => cb(e.payload)),
};

export interface PlayerSettings {
  volume: number;
  muted: boolean;
  delete_in_player_enabled: boolean;
}

export const playerSettings = {
  get: () => invoke<PlayerSettings>("get_player_settings"),
  set: (volume: number, muted: boolean, deleteInPlayerEnabled?: boolean) =>
    invoke<PlayerSettings>("set_player_settings", {
      volume,
      muted,
      deleteInPlayerEnabled: deleteInPlayerEnabled ?? null,
    }),
};

export interface AppearanceSettings {
  accent_preset: string;
}

export const appearanceSettings = {
  get: () => invoke<AppearanceSettings>("get_appearance_settings"),
  set: (accentPreset: string) =>
    invoke<AppearanceSettings>("set_appearance_settings", { accentPreset }),
};

/** Scan indexing + preview / pHash / MD5 concurrency. `workers_max` 0 = auto. */
export interface JobSettings {
  workers_max: number;
  effective_workers: number;
  cpu_count: number;
}

export const jobSettings = {
  get: () => invoke<JobSettings>("get_job_settings"),
  set: (workersMax: number) =>
    invoke<JobSettings>("set_job_settings", { workersMax }),
};

export interface StashBoxPreset {
  id: string;
  name: string;
  endpoint: string;
  account_url: string;
  api_key_set: boolean;
}

export interface StashDbSettings {
  active_id: string;
  api_key_set: boolean;
  endpoint: string;
  waterfall: boolean;
  presets: StashBoxPreset[];
}

export interface StashDbTestResult {
  username: string;
}

export interface StashDbSceneMatch {
  id: string;
  title: string | null;
  code: string | null;
  details: string | null;
  duration: number | null;
  date: string | null;
  studio: { name: string } | null;
  tags: { name: string }[] | null;
  performers: { performer: { name: string } }[] | null;
  images?: { url: string; width?: number | null; height?: number | null }[] | null;
}

export interface IdentifySceneResult {
  fingerprints: {
    file_id: string;
    oshash: string | null;
    md5: string | null;
    phash: string | null;
    duration_secs: number | null;
  };
  matches: StashDbSceneMatch[];
  md5_computed: boolean;
  phash_computed: boolean;
  title_search_used: boolean;
  title_search_term: string | null;
  /** Fingerprints missed and title/stem was too weak to text-search. */
  title_search_skipped_reason: string | null;
  provider_id: string;
  provider_name: string;
  /** Remote ids rejected for this scene (won't auto-apply). */
  rejected_remote_ids: string[];
}

export interface IdentifySceneOpts {
  /** Override / custom title-search term. */
  titleTerm?: string | null;
  /** Skip fingerprints; search by title only. */
  titleOnly?: boolean;
}

export interface ClearStashDbIdentifyInput {
  ignore_future?: boolean;
  clear_metadata?: boolean;
  reject_remote_id?: string | null;
  provider_id?: string | null;
}

export interface ApplyStashDbFields {
  title: boolean;
  details: boolean;
  studio: boolean;
  performers: boolean;
  tags: boolean;
  cover: boolean;
}

export const stashdbSettings = {
  get: () => invoke<StashDbSettings>("get_stashdb_settings"),
  set: (
    apiKey: string | null,
    endpoint: string | null,
    activeId?: string | null,
    waterfall?: boolean | null,
  ) =>
    invoke<StashDbSettings>("set_stashdb_settings", {
      apiKey,
      endpoint,
      activeId: activeId ?? null,
      waterfall: waterfall ?? null,
    }),
  test: (apiKey?: string | null, endpoint?: string | null) =>
    invoke<StashDbTestResult>("test_stashdb_connection", {
      apiKey: apiKey ?? null,
      endpoint: endpoint ?? null,
    }),
};

export const identify = {
  scene: (sceneId: string, opts?: IdentifySceneOpts | null) =>
    invoke<IdentifySceneResult>("identify_scene", {
      sceneId,
      opts: opts
        ? {
            titleTerm: opts.titleTerm ?? null,
            titleOnly: opts.titleOnly ?? false,
          }
        : null,
    }),
  apply: (
    sceneId: string,
    stashdbScene: StashDbSceneMatch,
    fields: ApplyStashDbFields,
    providerId?: string | null,
  ) =>
    invoke<void>("apply_stashdb_match", {
      sceneId,
      stashdbScene,
      fields,
      providerId: providerId ?? null,
    }),
  /** Unlink false-positive apply; ignore future batch + reject remote id. */
  clear: (sceneId: string, opts?: ClearStashDbIdentifyInput) =>
    invoke<void>("clear_stashdb_identify", { sceneId, opts: opts ?? null }),
  clearIgnore: (sceneId: string) =>
    invoke<void>("clear_stashdb_ignore", { sceneId }),
  reject: (sceneId: string, remoteId: string, providerId?: string | null) =>
    invoke<void>("reject_stashdb_match", {
      sceneId,
      remoteId,
      providerId: providerId ?? null,
    }),
  /** None of these — reject remotes, clear needs-review, skip batch. */
  dismissReview: (
    sceneId: string,
    remoteIds: string[],
    providerId?: string | null,
  ) =>
    invoke<void>("dismiss_stashdb_review", {
      sceneId,
      remoteIds,
      providerId: providerId ?? null,
    }),
  batch: (sceneIds: string[], autoApply: boolean) =>
    invoke<void>("batch_identify_scenes", { sceneIds, autoApply }),
  /** Batch set/clear the identify-ignore flag on a selection. */
  setIgnore: (sceneIds: string[], ignored: boolean) =>
    invoke<number>("batch_set_stashdb_ignore", { sceneIds, ignored }),
  batchLibrary: (options: BatchIdentifyLibraryOptions) =>
    invoke<StashDbIdentifyStats>("batch_identify_library", { options }),
  stats: (skipWithinDays: number, forceRescan: boolean) =>
    invoke<StashDbIdentifyStats>("stashdb_identify_stats", {
      skipWithinDays,
      forceRescan,
    }),
  onBatchProgress: (cb: (p: BatchIdentifyProgress) => void): Promise<UnlistenFn> =>
    listen<BatchIdentifyProgress>("identify://progress", (e) => cb(e.payload)),
};

export interface PathMetaSuggestion {
  kind: "studio" | "performer" | "tag" | string;
  id: string;
  name: string;
  already_linked: boolean;
  create_new: boolean;
  source: "catalog" | "folder" | "token" | "bracket" | string;
}

export interface SuggestPathMetadataResult {
  file_path: string;
  suggestions: PathMetaSuggestion[];
}

export interface ApplyPathMetadataInput {
  studio_id?: string | null;
  create_studio_name?: string | null;
  performer_ids?: string[];
  create_performer_names?: string[];
  tag_ids?: string[];
  create_tag_names?: string[];
}

export interface BatchPathMetadataResult {
  scenes_scanned: number;
  scenes_with_hits: number;
  studios_linked: number;
  performers_linked: number;
  tags_linked: number;
}

/** Path → catalog suggestions (ADR-013 — no file moves). */
export const pathMeta = {
  suggest: (sceneId: string) =>
    invoke<SuggestPathMetadataResult>("suggest_path_metadata", { sceneId }),
  apply: (sceneId: string, fields: ApplyPathMetadataInput) =>
    invoke<void>("apply_path_metadata", { sceneId, fields }),
  /** Link existing catalog names found in paths across the library (no creates). */
  batchApply: () => invoke<BatchPathMetadataResult>("batch_apply_path_metadata"),
};

export interface EmbeddedMetadataSuggestion {
  title: string | null;
  artist: string | null;
  comment: string | null;
  current_title: string | null;
  current_details: string | null;
}

export interface ApplyEmbeddedMetadataInput {
  title: boolean;
  details: boolean;
  artist_as_performer: boolean;
}

export const embeddedMeta = {
  suggest: (sceneId: string) =>
    invoke<EmbeddedMetadataSuggestion>("suggest_embedded_metadata", { sceneId }),
  apply: (
    sceneId: string,
    fields: ApplyEmbeddedMetadataInput,
    values: { title?: string | null; comment?: string | null; artist?: string | null },
  ) =>
    invoke<void>("apply_embedded_metadata", {
      sceneId,
      fields,
      title: values.title ?? null,
      comment: values.comment ?? null,
      artist: values.artist ?? null,
    }),
};

export interface ImportStashResult {
  matched: number;
  updated: number;
  skipped: number;
  errors: number;
  last_error: string | null;
}

/** Import metadata from a local Stash DB by fingerprint match (ADR-013 — no file moves). */
export const stashImport = {
  run: (stashDbPath: string) =>
    invoke<ImportStashResult>("import_stash_metadata", { stashDbPath }),
};

/** Timed segment / bookmark on a scene timeline. */
export interface SceneSegment {
  id: string;
  scene_id: string;
  start_sec: number;
  end_sec: number | null;
  label: string;
  tag_id: string | null;
  performer_id: string | null;
  created_at: string;
  updated_at: string;
}

export interface CreateSegmentArgs {
  scene_id: string;
  start_sec: number;
  end_sec?: number | null;
  label?: string | null;
  tag_id?: string | null;
  performer_id?: string | null;
}

export const segments = {
  list: (sceneId: string) =>
    invoke<SceneSegment[]>("list_scene_segments", { sceneId }),
  create: (args: CreateSegmentArgs) =>
    invoke<SceneSegment>("create_scene_segment", { args }),
  update: (args: {
    id: string;
    start_sec?: number;
    end_sec?: number;
    label?: string;
    tag_id?: string;
    performer_id?: string;
  }) => invoke<SceneSegment>("update_scene_segment", { args }),
  delete: (id: string) => invoke<void>("delete_scene_segment", { id }),
};

export interface DuplicateSceneEntry {
  scene_id: string;
  title: string | null;
  file_path: string | null;
  phash: string;
  thumb_path: string | null;
  favorite: number;
}

export interface DuplicateGroup {
  scenes: DuplicateSceneEntry[];
  max_distance: number;
}

export interface BatchIdentifyProgress {
  done: number;
  total: number;
  skipped: number;
  scene_id: string | null;
  matched: number;
  applied: number;
  needs_review: number;
  errors: number;
  finished: boolean;
  last_error: string | null;
}

export interface BatchIdentifyLibraryOptions {
  auto_apply: boolean;
  skip_within_days: number;
  force_rescan: boolean;
}

export interface StashDbIdentifyStats {
  total_scenes: number;
  never_checked: number;
  checked_recently: number;
  pending: number;
  needs_review: number;
}

export const DEFAULT_BATCH_IDENTIFY_LIBRARY_OPTIONS: BatchIdentifyLibraryOptions = {
  auto_apply: true,
  skip_within_days: 30,
  force_rescan: false,
};

export const duplicates = {
  findGroups: (threshold?: number) =>
    invoke<DuplicateGroup[]>("find_duplicate_groups", { threshold: threshold ?? null }),
  resolveGroup: (keeperSceneId: string, deleteSceneIds: string[]) =>
    invoke<number>("resolve_duplicate_group", { keeperSceneId, deleteSceneIds }),
};

// ─── metadata: tags / performers / studios + scene-field edits ───────────
export interface TagWithCount {
  id: string;
  name: string;
  color: string | null;
  scene_count: number;
}

export const tags = {
  list: () => invoke<TagRow[]>("list_tags"),
  listWithCounts: () => invoke<TagWithCount[]>("list_tags_with_counts"),
  create: (name: string) => invoke<TagRow>("create_tag", { name }),
  delete: (id: string) => invoke<void>("delete_tag", { id }),
  addToScene: (sceneId: string, tagId: string) =>
    invoke<void>("add_tag_to_scene", { sceneId, tagId }),
  removeFromScene: (sceneId: string, tagId: string) =>
    invoke<void>("remove_tag_from_scene", { sceneId, tagId }),
};

export const performers = {
  list: () => invoke<PerformerRow[]>("list_performers"),
  create: (name: string) => invoke<PerformerRow>("create_performer", { name }),
  delete: (id: string) => invoke<void>("delete_performer", { id }),
  addToScene: (sceneId: string, performerId: string) =>
    invoke<void>("add_performer_to_scene", { sceneId, performerId }),
  removeFromScene: (sceneId: string, performerId: string) =>
    invoke<void>("remove_performer_from_scene", { sceneId, performerId }),
};

export const studios = {
  list: () => invoke<StudioRow[]>("list_studios"),
  create: (name: string) => invoke<StudioRow>("create_studio", { name }),
  setSceneStudio: (sceneId: string, studioId: string | null) =>
    invoke<void>("set_scene_studio", { sceneId, studioId }),
};

export const sceneMeta = {
  setTitle: (sceneId: string, title: string | null) =>
    invoke<void>("set_scene_title", { sceneId, title }),
  setDetails: (sceneId: string, details: string | null) =>
    invoke<void>("set_scene_details", { sceneId, details }),
};

// ─── playlists (CRUD + items + weighted shuffle) ─────────────────────────
export interface PlaylistRow {
  id: string;
  name: string;
  shuffle_by_default: boolean;
  created_at: string;
  updated_at: string;
  item_count: number;
}

export interface ShuffleEntry extends SceneGridRow {
  weight: number;
}

/** Payload of the `playlist://changed` event emitted after any playlist mutation. */
export interface PlaylistChangedEvent {
  playlist_id: string;
}

export const playlists = {
  list: () => invoke<PlaylistRow[]>("list_playlists"),
  create: (name: string) => invoke<PlaylistRow>("create_playlist", { name }),
  rename: (id: string, name: string) => invoke<void>("rename_playlist", { id, name }),
  setShuffleDefault: (id: string, shuffleByDefault: boolean) =>
    invoke<void>("set_playlist_shuffle_default", { id, shuffleByDefault }),
  delete: (id: string) => invoke<void>("delete_playlist", { id }),
  items: (playlistId: string) => invoke<SceneGridRow[]>("playlist_items", { playlistId }),
  /** Returns true when the row was inserted, false when the scene was already in the playlist. */
  add: (playlistId: string, sceneId: string) =>
    invoke<boolean>("add_to_playlist", { playlistId, sceneId }),
  remove: (playlistId: string, sceneId: string) =>
    invoke<void>("remove_from_playlist", { playlistId, sceneId }),
  reorder: (playlistId: string, sceneIdsInOrder: string[]) =>
    invoke<void>("reorder_playlist", { playlistId, sceneIdsInOrder }),
  /** Weighted shuffle by favorite level. */
  shuffle: (playlistId: string) => invoke<ShuffleEntry[]>("shuffle_playlist", { playlistId }),
  /** Fired after any playlist mutation (create/rename/delete/add/remove/reorder). */
  onChanged: (cb: (p: PlaylistChangedEvent) => void): Promise<UnlistenFn> =>
    listen<PlaylistChangedEvent>("playlist://changed", (e) => cb(e.payload)),
};

export const savedFilters = {
  list: () => invoke<SavedFilterRow[]>("list_saved_filters"),
  create: (name: string, payload: SavedFilterPayload | string) =>
    invoke<SavedFilterRow>("create_saved_filter", {
      name,
      payload: typeof payload === "string" ? payload : JSON.stringify(payload),
    }),
  delete: (id: string) => invoke<void>("delete_saved_filter", { id }),
  rename: (id: string, name: string) => invoke<void>("rename_saved_filter", { id, name }),
};


// ─── updates (manual check only — no background network calls) ──────────

export interface UpdateCheck {
  current: string;
  latest: string;
  url: string;
  update_available: boolean;
}

export const updates = {
  /** Runs ONLY when the user clicks "Check for updates" — nothing automatic. */
  check: () => invoke<UpdateCheck>("check_for_updates"),
};
