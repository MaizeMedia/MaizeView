// Tiny reactive store for catalog UI state. Using Svelte 5 runes ($state) in
// a .svelte.ts module — the supported way to share reactive state outside
// components.

import {
  scenes as scenesApi,
  scan,
  identify as identifyApi,
  transcode as transcodeApi,
  LIST_SCENES_BATCH,
  type ListScenesArgs,
  type SortBy,
  type BatchIdentifyProgress,
  type BatchIdentifyLibraryOptions,
  type SavedFilterPayload,
  type TranscodeProgress,
  DEFAULT_BATCH_IDENTIFY_LIBRARY_OPTIONS,
} from "$lib/api";
import type { Counts, ScanProgress, SceneGridRow } from "$lib/api/types";
import { parseSearchQuery } from "$lib/search";
import { stringifyError } from "$lib/utils";

export type View = "library" | "favorites" | "playlists" | "tags" | "duplicates" | "settings";

class LibraryStore {
  view = $state<View>("library");
  search = $state("");
  searchInverse = $state(false);
  sort = $state<SortBy>("created");
  minFavorite = $state<number>(0); // 0 = no min-favorite filter
  tagIds = $state<string[]>([]);
  excludeTagIds = $state<string[]>([]);
  tagMatchAny = $state(false);
  performerIds = $state<string[]>([]);
  excludePerformerIds = $state<string[]>([]);
  studioIds = $state<string[]>([]);
  excludeStudioIds = $state<string[]>([]);
  minDurationMins = $state<number | null>(null);
  maxDurationMins = $state<number | null>(null);
  unplayedOnly = $state(false);
  /** 0 = off; ≥1 requires that many scene tags. */
  minTagCount = $state(0);
  identifiedOnly = $state(false);
  /** Multiple stash-box matches, not yet applied. */
  needsReviewOnly = $state(false);
  /** null = off; e.g. 720 / 1080. */
  minHeight = $state<number | null>(null);
  minPerformerCount = $state(0);
  scenes = $state<SceneGridRow[]>([]);
  /** Total scenes matching current filters (full DB, not just loaded page). */
  sceneTotal = $state(0);
  /** Background fetch still pulling remaining pages after the first batch. */
  loadingMore = $state(false);
  /** All matching scene rows are in memory (safe for Select all). */
  scenesFullyLoaded = $state(true);
  counts = $state<Counts>({ total: 0, favorites: 0 });
  loading = $state(false);
  error = $state<string | null>(null);

  scanning = $state(false);
  lastProgress = $state<ScanProgress | null>(null);
  /** Bumps when scan paths are added/removed so banners re-check offline folders. */
  scanPathsEpoch = $state(0);

  bumpScanPaths() {
    this.scanPathsEpoch += 1;
  }

  batchIdentifying = $state(false);
  batchIdentifyProgress = $state<BatchIdentifyProgress | null>(null);

  /** Convert / downscale — lives on the store so the banner survives view changes. */
  transcoding = $state(false);
  transcodeProgress = $state<TranscodeProgress | null>(null);
  transcodeCancelRequested = $state(false);

  selectedSceneId = $state<string | null>(null);

  private unlistenProgress: (() => void) | null = null;
  private unlistenBatchIdentify: (() => void) | null = null;
  private unlistenSceneDeleted: (() => void) | null = null;
  private unlistenTranscode: (() => void) | null = null;
  /** Bumps on each refresh so stale background fetches abort. */
  private refreshGeneration = 0;
  /** Skip per-delete catalog refresh while a multiselect delete runs. */
  suppressDeleteRefresh = $state(false);

  get favoritesOnly(): boolean {
    return this.view === "favorites";
  }

  private buildListArgs(offset = 0): ListScenesArgs {
    const parsed = parseSearchQuery(this.search);
    return {
      favoritesOnly: this.favoritesOnly,
      minFavorite: this.minFavorite || undefined,
      search: parsed.include || undefined,
      searchInverse: this.searchInverse || undefined,
      searchExcludeTerms: parsed.excludeTerms.length ? parsed.excludeTerms : undefined,
      sort: this.sort,
      tagIds: this.tagIds.length ? this.tagIds : undefined,
      tagMatchAny: this.tagMatchAny || undefined,
      excludeTagIds: this.excludeTagIds.length ? this.excludeTagIds : undefined,
      performerIds: this.performerIds.length ? this.performerIds : undefined,
      excludePerformerIds: this.excludePerformerIds.length ? this.excludePerformerIds : undefined,
      studioIds: this.studioIds.length ? this.studioIds : undefined,
      excludeStudioIds: this.excludeStudioIds.length ? this.excludeStudioIds : undefined,
      minDuration: this.minDurationMins ? this.minDurationMins * 60 : undefined,
      maxDuration: this.maxDurationMins ? this.maxDurationMins * 60 : undefined,
      unplayedOnly: this.unplayedOnly || undefined,
      minTagCount: this.minTagCount > 0 ? this.minTagCount : undefined,
      identifiedOnly: this.identifiedOnly || undefined,
      needsReviewOnly: this.needsReviewOnly || undefined,
      minHeight: this.minHeight && this.minHeight > 0 ? this.minHeight : undefined,
      minPerformerCount: this.minPerformerCount > 0 ? this.minPerformerCount : undefined,
      limit: LIST_SCENES_BATCH,
      offset,
    };
  }

  snapshotFilterPayload(): SavedFilterPayload {
    return {
      search: this.search,
      searchInverse: this.searchInverse,
      sort: this.sort,
      minFavorite: this.minFavorite,
      tagIds: [...this.tagIds],
      excludeTagIds: [...this.excludeTagIds],
      tagMatchAny: this.tagMatchAny,
      performerIds: [...this.performerIds],
      excludePerformerIds: [...this.excludePerformerIds],
      studioIds: [...this.studioIds],
      excludeStudioIds: [...this.excludeStudioIds],
      minDurationMins: this.minDurationMins,
      maxDurationMins: this.maxDurationMins,
      unplayedOnly: this.unplayedOnly,
      minTagCount: this.minTagCount,
      identifiedOnly: this.identifiedOnly,
      needsReviewOnly: this.needsReviewOnly,
      minHeight: this.minHeight,
      minPerformerCount: this.minPerformerCount,
    };
  }

  applyFilterPayload(payload: SavedFilterPayload) {
    this.search = payload.search ?? "";
    this.searchInverse = !!payload.searchInverse;
    this.sort = payload.sort ?? "created";
    this.minFavorite = payload.minFavorite ?? 0;
    this.tagIds = payload.tagIds ?? [];
    this.excludeTagIds = payload.excludeTagIds ?? [];
    this.tagMatchAny = !!payload.tagMatchAny;
    this.performerIds = payload.performerIds ?? [];
    this.excludePerformerIds = payload.excludePerformerIds ?? [];
    this.studioIds = payload.studioIds ?? [];
    this.excludeStudioIds = payload.excludeStudioIds ?? [];
    this.minDurationMins = payload.minDurationMins ?? null;
    this.maxDurationMins = payload.maxDurationMins ?? null;
    this.unplayedOnly = !!payload.unplayedOnly;
    this.minTagCount = payload.minTagCount ?? 0;
    this.identifiedOnly = !!payload.identifiedOnly;
    this.needsReviewOnly = !!payload.needsReviewOnly;
    this.minHeight = payload.minHeight ?? null;
    this.minPerformerCount = payload.minPerformerCount ?? 0;
    void this.refresh();
  }

  /** Fetch first page immediately; remaining pages load in the background. */
  private startMatchingScenesFetch(generation: number) {
    void (async () => {
      try {
        const first = await scenesApi.list(this.buildListArgs(0));
        if (generation !== this.refreshGeneration) return;

        let all = first.scenes;
        const total = first.total;
        this.scenes = all;
        this.sceneTotal = total;
        this.scenesFullyLoaded = all.length >= total;
        this.loading = false;
        if (this.selectionMode && this.selectedIds.size > 0) {
          const visible = new Set(all.map((r) => r.id));
          const pruned = new Set([...this.selectedIds].filter((id) => visible.has(id)));
          if (pruned.size !== this.selectedIds.size) {
            this.selectedIds = pruned;
          }
        }

        if (all.length >= total) return;

        this.loadingMore = true;
        while (all.length < total) {
          if (generation !== this.refreshGeneration) return;
          const page = await scenesApi.list(this.buildListArgs(all.length));
          if (generation !== this.refreshGeneration) return;
          if (page.scenes.length === 0) break;
          all = [...all, ...page.scenes];
          this.scenes = all;
        }
        if (generation === this.refreshGeneration) {
          this.scenesFullyLoaded = true;
        }
      } catch (e) {
        if (generation === this.refreshGeneration) {
          this.error = stringifyError(e);
          this.loading = false;
        }
      } finally {
        if (generation === this.refreshGeneration) {
          this.loadingMore = false;
        }
      }
    })();
  }

  /** Wait until the current filter's full scene list is in memory. */
  async ensureFullyLoaded() {
    while (this.loading || this.loadingMore) {
      await new Promise((r) => setTimeout(r, 50));
    }
  }

  async refresh() {
    const generation = ++this.refreshGeneration;
    this.loading = true;
    this.loadingMore = false;
    this.scenesFullyLoaded = false;
    this.error = null;
    this.scenes = [];
    try {
      const counts = await scenesApi.counts();
      if (generation !== this.refreshGeneration) return;
      this.counts = counts;
      this.startMatchingScenesFetch(generation);
    } catch (e) {
      if (generation === this.refreshGeneration) {
        this.error = stringifyError(e);
        this.loading = false;
      }
    }
  }

  async ensureProgressListener() {
    if (this.unlistenProgress) return;
    this.unlistenProgress = await scan.onProgress((p) => {
      this.lastProgress = p;
      this.scanning = p.status === "running";
      if (p.status !== "running") void this.refresh();
    });
  }

  async ensureBatchIdentifyListener() {
    if (this.unlistenBatchIdentify) return;
    this.unlistenBatchIdentify = await identifyApi.onBatchProgress((p) => {
      this.batchIdentifyProgress = p;
      this.batchIdentifying = !p.finished;
      if (p.finished) void this.refresh();
    });
  }

  async ensureSceneDeletedListener() {
    if (this.unlistenSceneDeleted) return;
    this.unlistenSceneDeleted = await scenesApi.onDeleted((p) => {
      if (this.selectedSceneId === p.scene_id) this.closeDetail();
      this.scenes = this.scenes.filter((s) => s.id !== p.scene_id);
      this.selectedIds.delete(p.scene_id);
      this.selectedIds = new Set(this.selectedIds);
      if (!this.suppressDeleteRefresh) void this.refresh();
    });
  }

  async ensureTranscodeListener() {
    if (this.unlistenTranscode) return;
    this.unlistenTranscode = await transcodeApi.onProgress((p) => {
      if (p.total > 0 && p.done === 0 && !p.finished) {
        this.transcodeCancelRequested = false;
      }
      this.transcodeProgress = p;
      this.transcoding = !p.finished;
      if (p.finished) void this.refresh();
    });
  }

  async cancelTranscode() {
    this.transcodeCancelRequested = true;
    try {
      await transcodeApi.cancel();
    } catch (e) {
      this.error = stringifyError(e);
      this.transcodeCancelRequested = false;
    }
  }

  async batchIdentifySelected(autoApply = true) {
    if (this.selectedIds.size === 0) return;
    await this.ensureBatchIdentifyListener();
    this.batchIdentifying = true;
    try {
      await identifyApi.batch([...this.selectedIds], autoApply);
    } catch (e) {
      this.error = stringifyError(e);
      this.batchIdentifying = false;
    }
  }

  async batchIdentifyLibrary(options: BatchIdentifyLibraryOptions = DEFAULT_BATCH_IDENTIFY_LIBRARY_OPTIONS) {
    await this.ensureBatchIdentifyListener();
    this.batchIdentifying = true;
    try {
      const stats = await identifyApi.batchLibrary(options);
      if (stats.pending === 0) {
        this.batchIdentifying = false;
        this.batchIdentifyProgress = {
          done: 0,
          total: 0,
          skipped: stats.checked_recently,
          scene_id: null,
          matched: 0,
          applied: 0,
          needs_review: 0,
          errors: 0,
          finished: true,
          last_error: null,
        };
      }
    } catch (e) {
      this.error = stringifyError(e);
      this.batchIdentifying = false;
    }
  }

  async startScan() {
    try {
      this.scanning = true;
      await scan.start();
    } catch (e) {
      this.error = stringifyError(e);
      this.scanning = false;
    }
  }

  async cancelScan() {
    try {
      await scan.cancel();
    } catch (e) {
      this.error = stringifyError(e);
    }
  }

  async setFavoriteLevel(scene: Pick<SceneGridRow, "id" | "favorite">, level: number) {
    const prev = scene.favorite;
    scene.favorite = level;
    try {
      await scenesApi.setFavorite(scene.id, level);
      // No catalog refetch — the optimistic update above is the whole change
      // (set_favorite returns nothing). Sync derived in-memory state instead.
      if ((prev === 0) !== (level === 0)) {
        // Topbar badge counts scenes with favorite > 0.
        this.counts = {
          ...this.counts,
          favorites: Math.max(0, this.counts.favorites + (level === 0 ? -1 : 1)),
        };
      }
      if (this.favoritesOnly && level === 0) {
        // Unfavorited scenes drop out of the Favorites view, as a refresh would do.
        this.scenes = this.scenes.filter((s) => s.id !== scene.id);
        this.sceneTotal = Math.max(0, this.sceneTotal - 1);
      } else if (this.sort === "favorite") {
        // Backend orders by `favorite DESC, created_at DESC` — mirror it.
        this.scenes = [...this.scenes].sort(
          (a, b) => b.favorite - a.favorite || b.created_at.localeCompare(a.created_at),
        );
      }
    } catch (e) {
      scene.favorite = prev;
      this.error = stringifyError(e);
    }
  }

  private removeFromOpposite(id: string, list: string[]): string[] {
    return list.filter((x) => x !== id);
  }

  toggleTag(id: string) {
    this.excludeTagIds = this.removeFromOpposite(id, this.excludeTagIds);
    this.tagIds = this.tagIds.includes(id)
      ? this.tagIds.filter((t) => t !== id)
      : [...this.tagIds, id];
    void this.refresh();
  }

  toggleExcludeTag(id: string) {
    this.tagIds = this.removeFromOpposite(id, this.tagIds);
    this.excludeTagIds = this.excludeTagIds.includes(id)
      ? this.excludeTagIds.filter((t) => t !== id)
      : [...this.excludeTagIds, id];
    void this.refresh();
  }

  togglePerformer(id: string) {
    this.excludePerformerIds = this.removeFromOpposite(id, this.excludePerformerIds);
    this.performerIds = this.performerIds.includes(id)
      ? this.performerIds.filter((p) => p !== id)
      : [...this.performerIds, id];
    void this.refresh();
  }

  toggleExcludePerformer(id: string) {
    this.performerIds = this.removeFromOpposite(id, this.performerIds);
    this.excludePerformerIds = this.excludePerformerIds.includes(id)
      ? this.excludePerformerIds.filter((p) => p !== id)
      : [...this.excludePerformerIds, id];
    void this.refresh();
  }

  setTagMatchAny(value: boolean) {
    this.tagMatchAny = value;
    void this.refresh();
  }

  setSearchInverse(value: boolean) {
    this.searchInverse = value;
    void this.refresh();
  }

  setUnplayedOnly(value: boolean) {
    this.unplayedOnly = value;
    void this.refresh();
  }

  setMinTagCount(value: number) {
    this.minTagCount = Math.max(0, Math.floor(value));
    void this.refresh();
  }

  setIdentifiedOnly(value: boolean) {
    this.identifiedOnly = value;
    if (value) this.needsReviewOnly = false;
    void this.refresh();
  }

  setNeedsReviewOnly(value: boolean) {
    this.needsReviewOnly = value;
    if (value) this.identifiedOnly = false;
    void this.refresh();
  }

  /** Jump to library filtered to stash-box multi-match scenes awaiting a pick. */
  showNeedsReview() {
    this.view = "library";
    this.needsReviewOnly = true;
    this.identifiedOnly = false;
    void this.refresh();
  }

  setMinHeight(value: number | null) {
    this.minHeight = value != null && value > 0 ? value : null;
    void this.refresh();
  }

  setMinPerformerCount(value: number) {
    this.minPerformerCount = Math.max(0, Math.floor(value));
    void this.refresh();
  }

  toggleStudio(id: string) {
    this.excludeStudioIds = this.removeFromOpposite(id, this.excludeStudioIds);
    this.studioIds = this.studioIds.includes(id)
      ? this.studioIds.filter((s) => s !== id)
      : [...this.studioIds, id];
    void this.refresh();
  }

  toggleExcludeStudio(id: string) {
    this.studioIds = this.removeFromOpposite(id, this.studioIds);
    this.excludeStudioIds = this.excludeStudioIds.includes(id)
      ? this.excludeStudioIds.filter((s) => s !== id)
      : [...this.excludeStudioIds, id];
    void this.refresh();
  }

  setMinDurationMins(value: number | null) {
    this.minDurationMins = value;
    void this.refresh();
  }

  setMaxDurationMins(value: number | null) {
    this.maxDurationMins = value;
    void this.refresh();
  }

  clearDurationFilters() {
    this.minDurationMins = null;
    this.maxDurationMins = null;
    void this.refresh();
  }

  clearFilters() {
    this.tagIds = [];
    this.excludeTagIds = [];
    this.tagMatchAny = false;
    this.performerIds = [];
    this.excludePerformerIds = [];
    this.studioIds = [];
    this.excludeStudioIds = [];
    this.minFavorite = 0;
    this.searchInverse = false;
    this.minDurationMins = null;
    this.maxDurationMins = null;
    this.unplayedOnly = false;
    this.minTagCount = 0;
    this.identifiedOnly = false;
    this.needsReviewOnly = false;
    this.minHeight = null;
    this.minPerformerCount = 0;
    void this.refresh();
  }

  get hasFilters(): boolean {
    return (
      this.tagIds.length > 0 ||
      this.excludeTagIds.length > 0 ||
      this.performerIds.length > 0 ||
      this.excludePerformerIds.length > 0 ||
      this.studioIds.length > 0 ||
      this.excludeStudioIds.length > 0 ||
      this.minFavorite > 0 ||
      this.searchInverse ||
      this.minDurationMins != null ||
      this.maxDurationMins != null ||
      this.unplayedOnly ||
      this.minTagCount > 0 ||
      this.identifiedOnly ||
      this.needsReviewOnly ||
      this.minHeight != null ||
      this.minPerformerCount > 0
    );
  }

  get activeFilterCount(): number {
    return (
      this.tagIds.length +
      this.excludeTagIds.length +
      this.performerIds.length +
      this.excludePerformerIds.length +
      this.studioIds.length +
      this.excludeStudioIds.length +
      (this.minFavorite > 0 ? 1 : 0) +
      (this.searchInverse ? 1 : 0) +
      (this.minDurationMins != null ? 1 : 0) +
      (this.maxDurationMins != null ? 1 : 0) +
      (this.unplayedOnly ? 1 : 0) +
      (this.minTagCount > 0 ? 1 : 0) +
      (this.identifiedOnly ? 1 : 0) +
      (this.needsReviewOnly ? 1 : 0) +
      (this.minHeight != null ? 1 : 0) +
      (this.minPerformerCount > 0 ? 1 : 0)
    );
  }

  openDetail(sceneId: string) {
    this.selectedSceneId = sceneId;
  }

  closeDetail() {
    this.selectedSceneId = null;
  }

  selectionMode = $state(false);
  selectedIds = $state<Set<string>>(new Set());

  enterSelectionMode() {
    this.selectionMode = true;
    this.selectedIds = new Set();
  }

  exitSelectionMode() {
    this.selectionMode = false;
    this.selectedIds = new Set();
  }

  toggleSelected(sceneId: string) {
    const next = new Set(this.selectedIds);
    if (next.has(sceneId)) next.delete(sceneId);
    else next.add(sceneId);
    this.selectedIds = next;
  }

  async selectAll() {
    await this.ensureFullyLoaded();
    this.selectedIds = new Set(this.scenes.map((s) => s.id));
  }

  /** Labels for currently selected scenes (visible grid only after refresh prune). */
  selectedSceneLabels(): string[] {
    return [...this.selectedIds].map((id) => {
      const row = this.scenes.find((s) => s.id === id);
      const label =
        row?.title?.trim() ||
        row?.file_path?.split(/[\\/]/).pop() ||
        `Scene ${id.slice(0, 8)}`;
      return label;
    });
  }

  selectNone() {
    this.selectedIds = new Set();
  }

  get selectionCount(): number {
    return this.selectedIds.size;
  }
}

export const library = new LibraryStore();
