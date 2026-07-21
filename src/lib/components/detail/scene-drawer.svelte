<script lang="ts">
  import { X, Plus, Trash2, Loader2, Film, ListPlus, Play, Search, Sparkles, Bookmark } from "@lucide/svelte";
  import {
    scenes,
    tags as tagsApi,
    performers as performersApi,
    studios as studiosApi,
    sceneMeta,
    playlists as playlistsApi,
    identify as identifyApi,
    pathMeta as pathMetaApi,
    embeddedMeta as embeddedMetaApi,
    segments as segmentsApi,
    assetUrl,
    openPlayerWindow,
    type SortBy,
    type PlaylistRow,
    type StashDbSceneMatch,
    type IdentifySceneResult,
    type PathMetaSuggestion,
    type EmbeddedMetadataSuggestion,
    type SceneSegment,
  } from "$lib/api";
  import type {
    PerformerRow,
    SceneDetail,
    StudioRow,
    TagRow,
  } from "$lib/api/types";
  import { library } from "$lib/stores/library.svelte";
  import { catalogs } from "$lib/stores/catalogs.svelte";
  import { confirm } from "@tauri-apps/plugin-dialog";
  import { stringifyError } from "$lib/utils";
  import Button from "$components/ui/button/button.svelte";
  import Input from "$components/ui/input/input.svelte";
  import Separator from "$components/ui/separator/separator.svelte";
  import FavoriteButton from "$components/favorite-button.svelte";

  let { sceneId }: { sceneId: string } = $props();

  let detail = $state<SceneDetail | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);

  // Editor scratch state
  let titleDraft = $state("");
  let detailsDraft = $state("");
  let titleEditing = $state(false);
  let detailsEditing = $state(false);

  // Add-chip autocomplete pools (shared catalogs cache — no refetch on open)
  let allTags: TagRow[] = $derived(catalogs.tags);
  let allPerformers: PerformerRow[] = $derived(catalogs.performers);
  let allStudios: StudioRow[] = $derived(catalogs.studios);
  let allPlaylists = $state<PlaylistRow[]>([]);
  let newTagName = $state("");
  let newPerformerName = $state("");
  let addedPlaylistToast = $state<string | null>(null);

  // Stash-box identify
  let identifyLoading = $state(false);
  let identifyResult = $state<IdentifySceneResult | null>(null);
  let identifyError = $state<string | null>(null);
  let applyingMatchId = $state<string | null>(null);
  let clearingIdentify = $state(false);
  let rejectingMatchId = $state<string | null>(null);
  let dismissingReview = $state(false);
  /** Editable title-search box (shown when fingerprints miss / title fallback). */
  let titleSearchDraft = $state("");
  let applyFields = $state({
    title: true,
    details: true,
    studio: true,
    performers: true,
    tags: true,
    cover: true,
  });

  let applyAnyField = $derived(
    applyFields.title ||
      applyFields.details ||
      applyFields.studio ||
      applyFields.performers ||
      applyFields.tags ||
      applyFields.cover,
  );

  // Path auto-tag (existing catalog names only)
  let pathSuggestLoading = $state(false);
  let pathSuggestions = $state<PathMetaSuggestion[]>([]);
  let pathSuggestError = $state<string | null>(null);
  let pathSelected = $state<Set<string>>(new Set());
  let pathApplying = $state(false);
  let pathSuggestRan = $state(false);

  let sceneSegments = $state<SceneSegment[]>([]);
  let deletingSegmentId = $state<string | null>(null);

  function fmtSegTime(s: number): string {
    if (!isFinite(s) || s < 0) s = 0;
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    const sec = Math.floor(s % 60);
    return h > 0
      ? `${h}:${String(m).padStart(2, "0")}:${String(sec).padStart(2, "0")}`
      : `${m}:${String(sec).padStart(2, "0")}`;
  }

  async function deleteSegment(id: string) {
    deletingSegmentId = id;
    try {
      await segmentsApi.delete(id);
      sceneSegments = sceneSegments.filter((s) => s.id !== id);
    } catch (e) {
      error = stringifyError(e);
    } finally {
      deletingSegmentId = null;
    }
  }

  async function renameSegment(seg: SceneSegment) {
    const next = window.prompt("Segment label", seg.label);
    if (next === null) return;
    const label = next.trim();
    try {
      const updated = await segmentsApi.update({ id: seg.id, label });
      sceneSegments = sceneSegments.map((s) => (s.id === seg.id ? updated : s));
    } catch (e) {
      error = stringifyError(e);
    }
  }

  // Embedded container tags
  let embeddedLoading = $state(false);
  let embedded = $state<EmbeddedMetadataSuggestion | null>(null);
  let embeddedError = $state<string | null>(null);
  let embeddedApplying = $state(false);
  let embeddedFields = $state({
    title: true,
    details: true,
    artist_as_performer: true,
  });

  // 5-heart hover state

  // Open this scene in a native player window (Phase 3). Falls back silently
  // if there's no playable file on record.
  async function play() {
    if (!detail) return;
    const file = detail.files[0];
    if (!file) return;
    try {
      await openPlayerWindow({
        sceneId: detail.scene.id,
        filePath: file.path,
        title: `MaizeView — ${detail.scene.title ?? file.path.split(/[\\/]/).pop() ?? "Player"}`,
      });
    } catch (e) {
      error = `Failed to open player: ${stringifyError(e)}`;
    }
  }

  async function load(opts: { resetIdentify?: boolean } = {}) {
    loading = true;
    error = null;
    try {
      const [d, pl, segs] = await Promise.all([
        scenes.detail(sceneId),
        playlistsApi.list(),
        segmentsApi.list(sceneId),
        catalogs.ensureLoaded(),
      ]);
      detail = d;
      allPlaylists = pl;
      sceneSegments = segs;
      titleDraft = d.scene.title ?? "";
      detailsDraft = d.scene.details ?? "";
      if (opts.resetIdentify) {
        identifyResult = null;
        identifyError = null;
        titleSearchDraft = "";
      }
      pathSuggestions = [];
      pathSuggestError = null;
      pathSelected = new Set();
      pathSuggestRan = false;
      embedded = null;
      embeddedError = null;
    } catch (e) {
      error = stringifyError(e);
    } finally {
      loading = false;
    }
  }

  function sceneNeedsReview(d: SceneDetail | null): boolean {
    if (!d) return false;
    if (d.scene.stashdb_ignored_at) return false;
    const n = d.scene.stashdb_match_count;
    return n != null && n > 1 && !d.scene.stashdb_applied_at;
  }

  function showTitleSearchBox(result: IdentifySceneResult | null): boolean {
    if (!result) return false;
    return (
      result.title_search_used ||
      !!result.title_search_term ||
      !!result.title_search_skipped_reason
    );
  }

  function fmtMatchDuration(secs: number | null | undefined): string | null {
    if (secs == null || !isFinite(secs) || secs < 0) return null;
    return fmtSegTime(secs);
  }

  function pathSuggestionKey(s: PathMetaSuggestion): string {
    return s.create_new ? `${s.kind}:new:${s.name}` : `${s.kind}:${s.id}`;
  }

  async function suggestFromPath() {
    if (!detail) return;
    pathSuggestLoading = true;
    pathSuggestError = null;
    pathSuggestRan = true;
    try {
      const result = await pathMetaApi.suggest(detail.scene.id);
      pathSuggestions = result.suggestions;
      // Pre-select existing catalog hits; leave create-new unchecked for review.
      pathSelected = new Set(
        result.suggestions
          .filter((s) => !s.already_linked && !s.create_new)
          .map(pathSuggestionKey),
      );
    } catch (e) {
      pathSuggestError = stringifyError(e);
      pathSuggestions = [];
    } finally {
      pathSuggestLoading = false;
    }
  }

  function togglePathSuggestion(s: PathMetaSuggestion) {
    const key = pathSuggestionKey(s);
    const next = new Set(pathSelected);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    pathSelected = next;
  }

  async function applyPathSuggestions() {
    if (!detail || pathSelected.size === 0) return;
    pathApplying = true;
    pathSuggestError = null;
    try {
      let studioId: string | null = null;
      let createStudioName: string | null = null;
      const performerIds: string[] = [];
      const createPerformerNames: string[] = [];
      const tagIds: string[] = [];
      const createTagNames: string[] = [];
      for (const s of pathSuggestions) {
        if (!pathSelected.has(pathSuggestionKey(s))) continue;
        if (s.kind === "studio") {
          if (s.create_new) createStudioName = s.name;
          else studioId = s.id;
        } else if (s.kind === "performer") {
          if (s.create_new) createPerformerNames.push(s.name);
          else performerIds.push(s.id);
        } else if (s.kind === "tag") {
          if (s.create_new) createTagNames.push(s.name);
          else tagIds.push(s.id);
        }
      }
      await pathMetaApi.apply(detail.scene.id, {
        studio_id: studioId,
        create_studio_name: createStudioName,
        performer_ids: performerIds,
        create_performer_names: createPerformerNames,
        tag_ids: tagIds,
        create_tag_names: createTagNames,
      });
      // Apply may have created catalog rows server-side — refresh the cache.
      await catalogs.refresh();
      await load();
      await library.refresh();
    } catch (e) {
      pathSuggestError = stringifyError(e);
    } finally {
      pathApplying = false;
    }
  }

  async function suggestEmbedded() {
    if (!detail) return;
    embeddedLoading = true;
    embeddedError = null;
    try {
      embedded = await embeddedMetaApi.suggest(detail.scene.id);
    } catch (e) {
      embeddedError = stringifyError(e);
      embedded = null;
    } finally {
      embeddedLoading = false;
    }
  }

  async function applyEmbedded() {
    if (!detail || !embedded) return;
    embeddedApplying = true;
    embeddedError = null;
    try {
      await embeddedMetaApi.apply(detail.scene.id, { ...embeddedFields }, {
        title: embedded.title,
        comment: embedded.comment,
        artist: embedded.artist,
      });
      // Artist→performer may have created a catalog row server-side.
      await catalogs.refresh();
      await load();
      await library.refresh();
    } catch (e) {
      embeddedError = stringifyError(e);
    } finally {
      embeddedApplying = false;
    }
  }

  async function addToPlaylist(playlistId: string, playlistName: string) {
    if (!detail) return;
    try {
      const inserted = await playlistsApi.add(playlistId, detail.scene.id);
      addedPlaylistToast = inserted ? `Added to “${playlistName}”` : `Already in “${playlistName}”`;
      setTimeout(() => (addedPlaylistToast = null), 2500);
      allPlaylists = await playlistsApi.list(); // refresh counts
    } catch (e) {
      error = stringifyError(e);
    }
  }

  // Inline "create new playlist" within the picker so you don't have to
  // navigate away from the scene you're viewing.
  let newPlaylistName = $state("");
  let showNewPlaylistField = $state(false);

  async function createPlaylistAndAdd() {
    if (!detail || !newPlaylistName.trim()) return;
    try {
      const row = await playlistsApi.create(newPlaylistName.trim());
      await playlistsApi.add(row.id, detail.scene.id);
      addedPlaylistToast = `Created “${row.name}” and added scene`;
      setTimeout(() => (addedPlaylistToast = null), 2500);
      allPlaylists = await playlistsApi.list();
      newPlaylistName = "";
      showNewPlaylistField = false;
    } catch (e) {
      error = stringifyError(e);
    }
  }

  // Reload whenever the open scene changes; auto-fetch candidates when needs review.
  $effect(() => {
    const id = sceneId;
    if (!id) return;
    let cancelled = false;
    void (async () => {
      await load({ resetIdentify: true });
      if (cancelled) return;
      if (sceneNeedsReview(detail) && detail?.scene.id === id) {
        await searchStashDb();
      }
    })();
    return () => {
      cancelled = true;
    };
  });

  async function setFavoriteLevel(next: number) {
    if (!detail) return;
    // Store method: optimistic update + rollback + in-memory catalog sync
    // (counts / favorites view / sort) — no catalog refetch needed.
    await library.setFavoriteLevel(detail.scene, next);
  }

  async function saveTitle() {
    if (!detail) return;
    try {
      // Treat a draft equal to the auto-derived filename as "clear the title"
      // (so the filename keeps showing) — UNLESS the user is deliberately
      // typing that name as the title. We compare to the raw filename only.
      const filename = detail.files[0]?.path.split(/[\\/]/).pop() ?? null;
      const trimmed = titleDraft.trim();
      const effective = trimmed === "" ? null : trimmed;
      await sceneMeta.setTitle(detail.scene.id, effective);
      detail.scene.title = effective;
      // If the user typed exactly the filename, keep it as the title (intentional).
      if (filename && trimmed === filename) {
        // already set above
      }
      titleEditing = false;
      await library.refresh();
    } catch (e) {
      error = stringifyError(e);
    }
  }

  function startEditTitle() {
    if (!detail) return;
    // Seed from the real title if present; otherwise from the filename so the
    // user is editing the existing text rather than starting from blank.
    titleDraft = detail.scene.title
      ?? detail.files[0]?.path.split(/[\\/]/).pop()
      ?? "";
    titleEditing = true;
  }

  async function saveDetails() {
    if (!detail) return;
    try {
      await sceneMeta.setDetails(detail.scene.id, detailsDraft.trim() || null);
      detail.scene.details = detailsDraft.trim() || null;
      detailsEditing = false;
    } catch (e) {
      error = stringifyError(e);
    }
  }

  // ─── tag assignment ──────────────────────────────────────────────────────
  let assignedTagIds = $derived(new Set(detail?.tags.map((t) => t.id) ?? []));
  let availableTags = $derived(allTags.filter((t) => !assignedTagIds.has(t.id)));

  async function addTag(tag: TagRow) {
    if (!detail) return;
    try {
      await tagsApi.addToScene(detail.scene.id, tag.id);
      detail.tags = [...detail.tags, tag].sort((a, b) => a.name.localeCompare(b.name));
    } catch (e) {
      error = stringifyError(e);
    }
  }
  async function removeTag(tag: TagRow) {
    if (!detail) return;
    try {
      await tagsApi.removeFromScene(detail.scene.id, tag.id);
      detail.tags = detail.tags.filter((t) => t.id !== tag.id);
    } catch (e) {
      error = stringifyError(e);
    }
  }
  async function createAndAddTag() {
    if (!detail || !newTagName.trim()) return;
    try {
      const t = await catalogs.createTag(newTagName.trim());
      await addTag(t);
      newTagName = "";
    } catch (e) {
      error = stringifyError(e);
    }
  }

  // ─── performer assignment ────────────────────────────────────────────────
  let assignedPerformerIds = $derived(new Set(detail?.performers.map((p) => p.id) ?? []));
  let availablePerformers = $derived(
    allPerformers.filter((p) => !assignedPerformerIds.has(p.id)),
  );
  async function addPerformer(p: PerformerRow) {
    if (!detail) return;
    try {
      await performersApi.addToScene(detail.scene.id, p.id);
      detail.performers = [...detail.performers, p].sort((a, b) =>
        a.name.localeCompare(b.name),
      );
    } catch (e) {
      error = stringifyError(e);
    }
  }
  async function removePerformer(p: PerformerRow) {
    if (!detail) return;
    try {
      await performersApi.removeFromScene(detail.scene.id, p.id);
      detail.performers = detail.performers.filter((x) => x.id !== p.id);
    } catch (e) {
      error = stringifyError(e);
    }
  }
  async function createAndAddPerformer() {
    if (!detail || !newPerformerName.trim()) return;
    try {
      const p = await catalogs.createPerformer(newPerformerName.trim());
      await addPerformer(p);
      newPerformerName = "";
    } catch (e) {
      error = stringifyError(e);
    }
  }

  // ─── studio ──────────────────────────────────────────────────────────────
  async function setStudio(studioId: string | null) {
    if (!detail) return;
    try {
      await studiosApi.setSceneStudio(detail.scene.id, studioId);
      detail.studio = studioId ? allStudios.find((s) => s.id === studioId) ?? null : null;
      detail.scene.studio_id = studioId;
    } catch (e) {
      error = stringifyError(e);
    }
  }

  async function searchStashDb(opts?: { titleTerm?: string; titleOnly?: boolean }) {
    if (!detail) return;
    const id = detail.scene.id;
    identifyLoading = true;
    identifyError = null;
    try {
      const result = await identifyApi.scene(
        id,
        opts?.titleOnly || opts?.titleTerm
          ? {
              titleTerm: opts.titleTerm ?? titleSearchDraft,
              titleOnly: !!opts.titleOnly,
            }
          : null,
      );
      if (detail?.scene.id !== id) return;
      // Refresh scene meta without wiping candidates (previous bug cleared matches).
      await load({ resetIdentify: false });
      if (detail?.scene.id !== id) return;
      identifyResult = result;
      titleSearchDraft = result.title_search_term ?? titleSearchDraft;
    } catch (e) {
      if (detail?.scene.id !== id) return;
      identifyError = stringifyError(e);
      identifyResult = null;
    } finally {
      if (detail?.scene.id === id) identifyLoading = false;
    }
  }

  async function searchTitleOnly() {
    const term = titleSearchDraft.trim();
    if (!term) {
      identifyError = "Enter a search term";
      return;
    }
    await searchStashDb({ titleTerm: term, titleOnly: true });
  }

  async function applyStashDbMatch(match: StashDbSceneMatch) {
    if (!detail || !applyAnyField) return;
    applyingMatchId = match.id;
    identifyError = null;
    try {
      await identifyApi.apply(detail.scene.id, match, { ...applyFields }, identifyResult?.provider_id);
      identifyResult = null;
      // Applied studio/performer/tag fields may create catalog rows server-side.
      await catalogs.refresh();
      await load({ resetIdentify: true });
      await library.refresh();
    } catch (e) {
      identifyError = stringifyError(e);
    } finally {
      applyingMatchId = null;
    }
  }

  async function unlinkStashDbIdentify(stripLinks = false) {
    if (!detail) return;
    const ok = await confirm(
      stripLinks
        ? "Unlink this stash-box match and REMOVE tags + performers?\n\n• Clears identify link and provider title/details/cover/studio\n• Removes ALL tags and performers from this scene — including any you added yourself (links have no provenance)\n• Blocks this remote scene from auto-applying again\n• Skips this file in future batch Identify runs"
        : "Unlink this stash-box match?\n\n• Clears identify link and provider title/details/cover/studio\n• Blocks this remote scene from auto-applying again\n• Skips this file in future batch Identify runs\n\nPerformers/tags are left for you to remove if wrong.",
      { title: stripLinks ? "Unlink + strip tags & performers" : "Unlink stash-box match", kind: "warning" },
    );
    if (!ok) return;
    clearingIdentify = true;
    identifyError = null;
    try {
      await identifyApi.clear(detail.scene.id, {
        ignore_future: true,
        clear_metadata: true,
        strip_links: stripLinks,
        reject_remote_id: detail.scene.stashdb_remote_id,
        provider_id: identifyResult?.provider_id ?? null,
      });
      identifyResult = null;
      await load({ resetIdentify: true });
      await library.refresh();
    } catch (e) {
      identifyError = stringifyError(e);
    } finally {
      clearingIdentify = false;
    }
  }

  async function allowIdentifyAgain() {
    if (!detail) return;
    clearingIdentify = true;
    identifyError = null;
    try {
      await identifyApi.clearIgnore(detail.scene.id);
      await load({ resetIdentify: false });
    } catch (e) {
      identifyError = stringifyError(e);
    } finally {
      clearingIdentify = false;
    }
  }

  async function rejectStashDbMatch(match: StashDbSceneMatch) {
    if (!detail) return;
    rejectingMatchId = match.id;
    identifyError = null;
    try {
      await identifyApi.reject(detail.scene.id, match.id, identifyResult?.provider_id);
      if (identifyResult) {
        identifyResult = {
          ...identifyResult,
          rejected_remote_ids: [...(identifyResult.rejected_remote_ids ?? []), match.id],
        };
      }
    } catch (e) {
      identifyError = stringifyError(e);
    } finally {
      rejectingMatchId = null;
    }
  }

  async function dismissNoneOfThese() {
    if (!detail || !identifyResult) return;
    dismissingReview = true;
    identifyError = null;
    try {
      const remoteIds = identifyResult.matches.map((m) => m.id);
      await identifyApi.dismissReview(
        detail.scene.id,
        remoteIds,
        identifyResult.provider_id,
      );
      identifyResult = null;
      await load({ resetIdentify: true });
      await library.refresh();
    } catch (e) {
      identifyError = stringifyError(e);
    } finally {
      dismissingReview = false;
    }
  }

  function isRejectedMatch(matchId: string): boolean {
    return (identifyResult?.rejected_remote_ids ?? []).includes(matchId);
  }

  function performerNames(match: StashDbSceneMatch): string {
    return (match.performers ?? [])
      .map((p) => p.performer.name)
      .slice(0, 4)
      .join(", ");
  }

  function matchMetaLine(match: StashDbSceneMatch): string {
    const parts: string[] = [];
    const dur = fmtMatchDuration(match.duration);
    if (dur) parts.push(dur);
    if (match.date) parts.push(match.date);
    if (match.code) parts.push(match.code);
    return parts.join(" · ");
  }

  function sourceLabel(source: string): string | null {
    if (source === "manual") return null;
    if (source === "filename") return "filename";
    return source;
  }

  function fmtDuration(s: number | null): string {
    if (s == null) return "—";
    const m = Math.floor(s / 60);
    const sec = Math.floor(s % 60);
    if (m < 60) return `${m}:${sec.toString().padStart(2, "0")}`;
    const h = Math.floor(m / 60);
    return `${h}:${(m % 60).toString().padStart(2, "0")}:${sec.toString().padStart(2, "0")}`;
  }
  function fmtSize(b: number): string {
    const units = ["B", "KB", "MB", "GB", "TB"];
    let v = b;
    let i = 0;
    while (v >= 1024 && i < units.length - 1) {
      v /= 1024;
      i++;
    }
    return `${v.toFixed(v >= 10 || i === 0 ? 0 : 1)} ${units[i]}`;
  }
</script>

<aside class="flex h-full w-[440px] shrink-0 flex-col border-l border-border bg-card shadow-2xl" data-testid="scene-drawer">
  <!-- header -->
  <header class="flex items-center justify-between border-b border-border px-4 py-3">
    <span class="text-sm font-semibold">Scene details</span>
    <Button variant="ghost" size="icon-sm" onclick={() => library.closeDetail()} aria-label="Close">
      <X class="size-4" />
    </Button>
  </header>

  {#if loading}
    <div class="flex flex-1 items-center justify-center text-muted-foreground">
      <Loader2 class="size-5 animate-spin" />
    </div>
  {:else if error || !detail}
    <div class="flex flex-1 items-center justify-center p-4 text-center text-sm text-destructive">
      {error ?? "Failed to load scene"}
    </div>
  {:else}
    <div class="flex-1 overflow-y-auto p-4">
      <!-- thumbnail + play (always shown when a file exists) -->
      {#if detail.files[0]}
        <div class="group relative mb-4 aspect-video w-full overflow-hidden rounded-md bg-secondary/60">
          {#if detail.scene.cover_path || detail.files[0].thumb_path || detail.files[0].thumb_sprite_path}
            <img
              src={assetUrl(detail.scene.cover_path ?? detail.files[0].thumb_path ?? detail.files[0].thumb_sprite_path) ?? ""}
              alt=""
              class="h-full w-full object-cover"
            />
          {:else}
            <div class="flex h-full w-full items-center justify-center text-muted-foreground/40">
              <Film class="size-12" />
            </div>
          {/if}
          <button
            type="button"
            onclick={play}
            aria-label="Play"
            class="absolute inset-0 flex items-center justify-center bg-black/0 opacity-0 transition-all duration-150 hover:bg-black/40 hover:opacity-100 group-hover:opacity-100"
          >
            <span class="flex size-14 items-center justify-center rounded-full bg-primary/90 text-primary-foreground shadow-lg transition-transform hover:scale-105">
              <Play class="size-6 translate-x-0.5" fill="currentColor" />
            </span>
          </button>
        </div>
      {/if}

      <!-- title + favorite -->
      <div class="mb-3 flex items-start justify-between gap-2">
        <div class="min-w-0 flex-1">
          {#if titleEditing}
            <div class="flex gap-2">
              <Input bind:value={titleDraft} placeholder="Title" class="h-8" />
              <Button size="sm" onclick={saveTitle}>Save</Button>
              <Button size="sm" variant="ghost" onclick={() => { titleEditing = false; titleDraft = detail.scene.title ?? ""; }} disabled={!detail}>Cancel</Button>
            </div>
          {:else}
            <button
              type="button"
              class="text-left text-lg font-semibold leading-tight hover:text-primary"
              onclick={startEditTitle}
            >
              {detail.scene.title ?? detail.files[0]?.path.split(/[\\/]/).pop() ?? "Untitled"}
              {#if sourceLabel(detail.scene.title_source)}
                <span class="ml-1 rounded bg-secondary px-1 py-0.5 text-[10px] font-normal text-muted-foreground">
                  {sourceLabel(detail.scene.title_source)}
                </span>
              {/if}
              <span class="ml-1 text-xs font-normal text-muted-foreground">edit</span>
            </button>
          {/if}
        </div>
        <FavoriteButton
          level={detail.scene.favorite}
          onChange={setFavoriteLevel}
        />
      </div>

      <!-- Stash-box identify (active provider from Settings) -->
      <div
        class="mb-3 rounded-md border p-3 {sceneNeedsReview(detail)
          ? 'border-amber-500/50 bg-amber-500/5'
          : 'border-border bg-background/40'}"
        data-testid="identify-panel"
      >
        <div class="mb-2 flex items-center justify-between gap-2">
          <div class="flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
            <Sparkles class="size-3" />
            Identify
            {#if sceneNeedsReview(detail)}
              <span class="rounded bg-amber-500/20 px-1.5 py-0.5 text-[10px] font-semibold text-amber-800 dark:text-amber-300">
                Needs review
              </span>
            {/if}
            {#if detail.scene.stashdb_ignored_at}
              <span class="rounded bg-destructive/15 px-1.5 py-0.5 text-[10px] font-semibold text-destructive">
                Identify ignored
              </span>
            {/if}
          </div>
          <div class="flex flex-wrap items-center justify-end gap-1.5">
            {#if detail.files[0]}
              <Button size="sm" variant="outline" onclick={play} title="Play local file">
                <Play class="size-3" />
                Play
              </Button>
            {/if}
            {#if detail.scene.stashdb_applied_at}
              <Button
                size="sm"
                variant="outline"
                onclick={() => unlinkStashDbIdentify(false)}
                disabled={clearingIdentify}
                title="Unlink false match and skip future batch identify"
              >
                {#if clearingIdentify}
                  <Loader2 class="size-3 animate-spin" />
                {:else}
                  Unlink
                {/if}
              </Button>
              <Button
                size="sm"
                variant="outline"
                onclick={() => unlinkStashDbIdentify(true)}
                disabled={clearingIdentify}
                title="Unlink and remove ALL tags + performers (wrong-match cleanup)"
              >
                {#if clearingIdentify}
                  <Loader2 class="size-3 animate-spin" />
                {:else}
                  Unlink + strip
                {/if}
              </Button>
            {:else if detail.scene.stashdb_ignored_at}
              <Button
                size="sm"
                variant="outline"
                onclick={allowIdentifyAgain}
                disabled={clearingIdentify}
              >
                Allow again
              </Button>
            {/if}
            <Button
              size="sm"
              variant={sceneNeedsReview(detail) ? "default" : "outline"}
              onclick={searchStashDb}
              disabled={identifyLoading || !detail.files[0]}
            >
              {#if identifyLoading}
                <Loader2 class="size-3 animate-spin" />
                Searching…
              {:else}
                <Search class="size-3" />
                {sceneNeedsReview(detail) && !identifyResult ? "Load matches" : "Search"}
              {/if}
            </Button>
          </div>
        </div>
        <p class="text-[11px] text-muted-foreground">
          Match against the active stash-box provider (Settings) by fingerprint (OSHASH + MD5 + pHash). Falls back to title search when no fingerprint hits.
        </p>
        {#if detail.files[0]?.duration != null}
          <p class="mt-1 text-[11px] text-muted-foreground">
            Local file duration: {fmtMatchDuration(detail.files[0].duration)}
          </p>
        {/if}
        {#if detail.scene.stashdb_checked_at}
          <p class="mt-1 text-[11px] text-muted-foreground">
            Last checked {new Date(detail.scene.stashdb_checked_at).toLocaleString()}
            {#if detail.scene.stashdb_match_count != null}
              · {detail.scene.stashdb_match_count} match{detail.scene.stashdb_match_count === 1 ? "" : "es"}
            {/if}
            {#if sceneNeedsReview(detail)}
              · <span class="text-amber-700 dark:text-amber-400">pick a match below</span>
            {/if}
            {#if detail.scene.stashdb_applied_at}
              · applied {new Date(detail.scene.stashdb_applied_at).toLocaleDateString()}
            {/if}
            {#if detail.scene.stashdb_ignored_at}
              · skipped in batch Identify
            {/if}
          </p>
        {/if}
        {#if identifyError}
          <p class="mt-2 text-xs text-destructive">{identifyError}</p>
        {/if}
        {#if identifyResult}
          <p class="mt-2 text-[11px] text-muted-foreground">
            Provider: {identifyResult.provider_name}
          </p>
          {#if identifyResult.md5_computed}
            <p class="mt-2 text-[11px] text-muted-foreground">MD5 fingerprint computed and saved.</p>
          {/if}
          {#if identifyResult.phash_computed}
            <p class="mt-2 text-[11px] text-muted-foreground">pHash fingerprint computed and saved.</p>
          {:else if identifyResult.fingerprints.phash}
            <p class="mt-2 text-[11px] text-muted-foreground">pHash already on file (reused).</p>
          {/if}
          {#if showTitleSearchBox(identifyResult)}
            <div class="mt-2 space-y-1.5">
              {#if identifyResult.title_search_skipped_reason && !identifyResult.title_search_used}
                <p class="text-[11px] text-muted-foreground">
                  No fingerprint matches — auto title search skipped ({identifyResult.title_search_skipped_reason}). Edit the term below.
                </p>
              {:else}
                <p class="text-[11px] text-muted-foreground">
                  No fingerprint matches — title search. Edit the term and search again.
                </p>
              {/if}
              <div class="flex gap-1.5">
                <Input
                  class="h-8 flex-1 text-xs"
                  bind:value={titleSearchDraft}
                  placeholder="Title / code / performers…"
                  onkeydown={(e) => {
                    if (e.key === "Enter") {
                      e.preventDefault();
                      void searchTitleOnly();
                    }
                  }}
                />
                <Button
                  size="sm"
                  variant="outline"
                  class="h-8 shrink-0"
                  onclick={searchTitleOnly}
                  disabled={identifyLoading || !titleSearchDraft.trim()}
                >
                  {#if identifyLoading}
                    <Loader2 class="size-3 animate-spin" />
                  {:else}
                    Search title
                  {/if}
                </Button>
              </div>
            </div>
          {/if}
          {#if identifyResult.matches.length === 0}
            <p class="mt-2 text-xs text-muted-foreground">No matches found.</p>
          {:else}
            {#if identifyResult.matches.length > 1}
              <p class="mt-2 text-[11px] text-amber-700 dark:text-amber-400">
                Multiple matches — compare duration/date/cover, then Apply. Use Play to check your file.
              </p>
            {/if}
            <div class="mt-2 flex flex-wrap items-center gap-2">
              <Button
                size="sm"
                variant="outline"
                class="h-7"
                onclick={dismissNoneOfThese}
                disabled={dismissingReview}
                title="Reject all candidates, clear needs-review, skip future batch Identify"
              >
                {#if dismissingReview}
                  <Loader2 class="size-3 animate-spin" />
                  Clearing…
                {:else}
                  None of these
                {/if}
              </Button>
              <span class="text-[11px] text-muted-foreground">
                Clears needs-review and skips this file in batch Identify.
              </span>
            </div>
            <div class="mt-2 flex flex-wrap gap-x-3 gap-y-1 text-[11px] text-muted-foreground">
              {#each [
                ["title", "Title"],
                ["details", "Details"],
                ["studio", "Studio"],
                ["performers", "Performers"],
                ["tags", "Tags"],
                ["cover", "Cover"],
              ] as [key, label]}
                <label class="flex items-center gap-1">
                  <input
                    type="checkbox"
                    class="accent-primary"
                    checked={applyFields[key as keyof typeof applyFields]}
                    onchange={(e) => {
                      applyFields = {
                        ...applyFields,
                        [key]: (e.currentTarget as HTMLInputElement).checked,
                      };
                    }}
                  />
                  {label}
                </label>
              {/each}
            </div>
            <ul class="mt-2 space-y-2">
              {#each identifyResult.matches as match (match.id)}
                <li class="rounded border border-border bg-card/60 p-2 text-xs">
                  {#if match.images?.[0]?.url}
                    <img
                      src={match.images[0].url}
                      alt=""
                      class="mb-2 max-h-24 rounded object-cover"
                      loading="lazy"
                    />
                  {/if}
                  <div class="font-medium leading-snug">
                    {match.title ?? match.code ?? "Untitled"}
                  </div>
                  {#if matchMetaLine(match)}
                    <div class="text-muted-foreground">{matchMetaLine(match)}</div>
                  {/if}
                  {#if match.studio?.name}
                    <div class="text-muted-foreground">{match.studio.name}</div>
                  {/if}
                  {#if performerNames(match)}
                    <div class="truncate text-muted-foreground">{performerNames(match)}</div>
                  {/if}
                  {#if isRejectedMatch(match.id)}
                    <p class="mt-2 text-[11px] text-destructive">Rejected — won’t auto-apply again.</p>
                  {/if}
                  <div class="mt-2 flex flex-wrap gap-1.5">
                    <Button
                      size="sm"
                      class="h-7"
                      onclick={() => applyStashDbMatch(match)}
                      disabled={applyingMatchId === match.id || !applyAnyField || isRejectedMatch(match.id)}
                    >
                      {#if applyingMatchId === match.id}
                        <Loader2 class="size-3 animate-spin" />
                        Applying…
                      {:else}
                        Apply selected fields
                      {/if}
                    </Button>
                    {#if !isRejectedMatch(match.id)}
                      <Button
                        size="sm"
                        variant="ghost"
                        class="h-7"
                        onclick={() => rejectStashDbMatch(match)}
                        disabled={rejectingMatchId === match.id}
                      >
                        {#if rejectingMatchId === match.id}
                          <Loader2 class="size-3 animate-spin" />
                        {:else}
                          Not this
                        {/if}
                      </Button>
                    {/if}
                  </div>
                </li>
              {/each}
            </ul>
          {/if}
        {/if}
      </div>

      <!-- Path auto-tag (local catalog names only; ADR-013) -->
      <div class="mb-3 rounded-md border border-border bg-background/40 p-3">
        <div class="mb-2 flex items-center justify-between gap-2">
          <div class="flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
            Path match
          </div>
          <Button
            size="sm"
            variant="outline"
            onclick={suggestFromPath}
            disabled={pathSuggestLoading || !detail.files[0]}
          >
            {#if pathSuggestLoading}
              <Loader2 class="size-3 animate-spin" />
              Matching…
            {:else}
              <Search class="size-3" />
              Match path
            {/if}
          </Button>
        </div>
        <p class="text-[11px] text-muted-foreground">
          Match catalog names in the path, suggest studios/performers from folders (skips videos/movies buckets), and tag tokens (1080p, …). Create-new chips need your check. Does not move files.
        </p>
        {#if pathSuggestError}
          <p class="mt-2 text-xs text-destructive">{pathSuggestError}</p>
        {/if}
        {#if pathSuggestions.length > 0}
          <div class="mt-2 flex flex-wrap gap-1.5">
            {#each pathSuggestions as s (pathSuggestionKey(s))}
              <button
                type="button"
                onclick={() => togglePathSuggestion(s)}
                disabled={s.already_linked}
                class="rounded-full border px-2 py-0.5 text-[11px] transition-colors
                  {s.already_linked
                  ? 'cursor-default border-border bg-muted text-muted-foreground'
                  : pathSelected.has(pathSuggestionKey(s))
                    ? 'border-primary bg-primary/15 text-primary'
                    : s.create_new
                      ? 'border-amber-500/40 bg-amber-500/5 hover:bg-amber-500/10'
                      : 'border-border bg-card hover:bg-accent'}"
              >
                <span class="opacity-60">{s.kind}</span>
                {s.name}
                {#if s.create_new}
                  · new
                {:else if s.source !== "catalog"}
                  · {s.source}
                {/if}
                {#if s.already_linked}
                  · linked
                {/if}
              </button>
            {/each}
          </div>
          <Button
            size="sm"
            class="mt-2 h-7"
            onclick={applyPathSuggestions}
            disabled={pathApplying || pathSelected.size === 0}
          >
            {#if pathApplying}
              <Loader2 class="size-3 animate-spin" />
              Applying…
            {:else}
              Apply selected
            {/if}
          </Button>
        {:else if pathSuggestRan && !pathSuggestLoading}
          <p class="mt-2 text-xs text-muted-foreground">
            No path suggestions for this file.
          </p>
        {/if}
      </div>

      <!-- Embedded container tags -->
      <div class="mb-3 rounded-md border border-border bg-background/40 p-3">
        <div class="mb-2 flex items-center justify-between gap-2">
          <div class="text-xs font-medium text-muted-foreground">File tags</div>
          <Button
            size="sm"
            variant="outline"
            onclick={suggestEmbedded}
            disabled={embeddedLoading || !detail.files[0]}
          >
            {#if embeddedLoading}
              <Loader2 class="size-3 animate-spin" />
              Reading…
            {:else}
              Read tags
            {/if}
          </Button>
        </div>
        <p class="text-[11px] text-muted-foreground">
          Title / artist / comment from the container (ffprobe). New scans prefer embedded title when present.
        </p>
        {#if embeddedError}
          <p class="mt-2 text-xs text-destructive">{embeddedError}</p>
        {/if}
        {#if embedded}
          {#if !embedded.title && !embedded.artist && !embedded.comment}
            <p class="mt-2 text-xs text-muted-foreground">No useful embedded tags found.</p>
          {:else}
            <ul class="mt-2 space-y-1 text-[11px] text-muted-foreground">
              {#if embedded.title}
                <li><span class="font-medium text-foreground">Title:</span> {embedded.title}</li>
              {/if}
              {#if embedded.artist}
                <li><span class="font-medium text-foreground">Artist:</span> {embedded.artist}</li>
              {/if}
              {#if embedded.comment}
                <li class="line-clamp-3"><span class="font-medium text-foreground">Comment:</span> {embedded.comment}</li>
              {/if}
            </ul>
            <div class="mt-2 flex flex-wrap gap-x-3 gap-y-1 text-[11px] text-muted-foreground">
              <label class="flex items-center gap-1">
                <input type="checkbox" class="accent-primary" bind:checked={embeddedFields.title} disabled={!embedded.title} />
                Title
              </label>
              <label class="flex items-center gap-1">
                <input type="checkbox" class="accent-primary" bind:checked={embeddedFields.details} disabled={!embedded.comment} />
                Details
              </label>
              <label class="flex items-center gap-1">
                <input type="checkbox" class="accent-primary" bind:checked={embeddedFields.artist_as_performer} disabled={!embedded.artist} />
                Artist → performer
              </label>
            </div>
            <Button size="sm" class="mt-2 h-7" onclick={applyEmbedded} disabled={embeddedApplying}>
              {#if embeddedApplying}
                <Loader2 class="size-3 animate-spin" />
                Applying…
              {:else}
                Apply selected
              {/if}
            </Button>
          {/if}
        {/if}
      </div>

      <!-- Timed segments -->
      <div class="mb-3 rounded-md border border-border bg-background/40 p-3">
        <div class="mb-1 flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
          <Bookmark class="size-3.5" />
          Segments
          {#if sceneSegments.length > 0}
            <span class="text-foreground/70">({sceneSegments.length})</span>
          {/if}
        </div>
        <p class="text-[11px] text-muted-foreground">
          Mark in the player with <kbd class="rounded bg-muted px-1">I</kbd>/<kbd class="rounded bg-muted px-1">O</kbd>. Open playback to jump.
        </p>
        {#if sceneSegments.length === 0}
          <p class="mt-2 text-xs text-muted-foreground">No segments on this scene yet.</p>
        {:else}
          <ul class="mt-2 space-y-1">
            {#each sceneSegments as seg (seg.id)}
              <li class="flex items-center gap-2 rounded-md px-1 py-0.5 text-xs hover:bg-muted/50">
                <span class="shrink-0 font-mono tabular-nums text-muted-foreground">
                  {fmtSegTime(seg.start_sec)}{#if seg.end_sec != null}–{fmtSegTime(seg.end_sec)}{/if}
                </span>
                <button
                  type="button"
                  class="min-w-0 flex-1 truncate text-left hover:underline"
                  title="Click to rename"
                  onclick={() => void renameSegment(seg)}
                >
                  {seg.label || "Untitled"}
                </button>
                <button
                  type="button"
                  class="rounded p-1 text-muted-foreground hover:bg-destructive/10 hover:text-destructive"
                  aria-label="Delete segment"
                  disabled={deletingSegmentId === seg.id}
                  onclick={() => void deleteSegment(seg.id)}
                >
                  {#if deletingSegmentId === seg.id}
                    <Loader2 class="size-3.5 animate-spin" />
                  {:else}
                    <Trash2 class="size-3.5" />
                  {/if}
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </div>

      <!-- studio -->
      <div class="mb-3">
        <div class="mb-1 text-xs font-medium text-muted-foreground">Studio</div>
        <select
          value={detail.scene.studio_id ?? ""}
          onchange={(e) => setStudio((e.currentTarget as HTMLSelectElement).value || null)}
          class="h-8 w-full rounded-md border border-input bg-background px-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          <option value="">— None —</option>
          {#each allStudios as s (s.id)}
            <option value={s.id}>{s.name}</option>
          {/each}
        </select>
      </div>

      <!-- tags -->
      <div class="mb-3">
        <div class="mb-1 text-xs font-medium text-muted-foreground">Tags</div>
        <div class="flex flex-wrap gap-1.5">
          {#each detail.tags as t (t.id)}
            <span class="flex items-center gap-1 rounded-full bg-secondary px-2 py-0.5 text-xs">
              {t.name}
              <button type="button" aria-label="Remove tag" onclick={() => removeTag(t)} class="text-muted-foreground hover:text-destructive">
                <X class="size-3" />
              </button>
            </span>
          {/each}
        </div>
        <div class="mt-2 flex gap-1.5">
          <select
            value=""
            onchange={(e) => { const v = (e.currentTarget as HTMLSelectElement).value; if (v) { const t = availableTags.find((x) => x.id === v); if (t) addTag(t); (e.currentTarget as HTMLSelectElement).value = ""; } }}
            class="h-8 flex-1 rounded-md border border-input bg-background px-2 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            <option value="">Add tag…</option>
            {#each availableTags as t (t.id)}
              <option value={t.id}>{t.name}</option>
            {/each}
          </select>
        </div>
        <div class="mt-1.5 flex gap-1.5">
          <Input bind:value={newTagName} placeholder="New tag…" class="h-8 text-xs" />
          <Button size="sm" variant="outline" onclick={createAndAddTag} disabled={!newTagName.trim()}>
            <Plus class="size-3" />Create
          </Button>
        </div>
      </div>

      <!-- performers -->
      <div class="mb-3">
        <div class="mb-1 text-xs font-medium text-muted-foreground">Performers</div>
        <div class="flex flex-wrap gap-1.5">
          {#each detail.performers as p (p.id)}
            <span class="flex items-center gap-1 rounded-full bg-secondary px-2 py-0.5 text-xs">
              {p.name}
              <button type="button" aria-label="Remove performer" onclick={() => removePerformer(p)} class="text-muted-foreground hover:text-destructive">
                <X class="size-3" />
              </button>
            </span>
          {/each}
        </div>
        <div class="mt-2 flex gap-1.5">
          <select
            value=""
            onchange={(e) => { const v = (e.currentTarget as HTMLSelectElement).value; if (v) { const p = availablePerformers.find((x) => x.id === v); if (p) addPerformer(p); (e.currentTarget as HTMLSelectElement).value = ""; } }}
            class="h-8 flex-1 rounded-md border border-input bg-background px-2 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            <option value="">Add performer…</option>
            {#each availablePerformers as p (p.id)}
              <option value={p.id}>{p.name}</option>
            {/each}
          </select>
        </div>
        <div class="mt-1.5 flex gap-1.5">
          <Input bind:value={newPerformerName} placeholder="New performer…" class="h-8 text-xs" />
          <Button size="sm" variant="outline" onclick={createAndAddPerformer} disabled={!newPerformerName.trim()}>
            <Plus class="size-3" />Create
          </Button>
        </div>
      </div>

      <!-- details -->
      <div class="mb-3">
        <div class="mb-1 flex items-center justify-between">
          <span class="text-xs font-medium text-muted-foreground">Details</span>
          {#if !detailsEditing}
            <button type="button" class="text-xs text-muted-foreground hover:text-primary" onclick={() => { detailsEditing = true; }}>edit</button>
          {/if}
        </div>
        {#if detailsEditing}
          <div class="space-y-1.5">
            <textarea
              bind:value={detailsDraft}
              rows="4"
              class="w-full rounded-md border border-input bg-background p-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
            ></textarea>
            <div class="flex gap-1.5">
              <Button size="sm" onclick={saveDetails}>Save</Button>
              <Button size="sm" variant="ghost" onclick={() => { detailsEditing = false; detailsDraft = detail.scene.details ?? ""; }}>Cancel</Button>
            </div>
          </div>
        {:else}
          <p class="whitespace-pre-wrap text-sm text-muted-foreground">
            {detail.scene.details ?? "—"}
          </p>
        {/if}
      </div>

      <!-- add to playlist -->
      <div class="mb-3">
        <div class="mb-1 flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
          <ListPlus class="size-3" />
          Add to playlist
        </div>
        {#if allPlaylists.length === 0 && !showNewPlaylistField}
          <p class="mb-1.5 text-xs text-muted-foreground">
            No playlists yet.
          </p>
        {:else if allPlaylists.length > 0}
          <select
            value=""
            onchange={(e) => {
              const v = (e.currentTarget as HTMLSelectElement).value;
              if (v) {
                const pl = allPlaylists.find((x) => x.id === v);
                if (pl) addToPlaylist(pl.id, pl.name);
                (e.currentTarget as HTMLSelectElement).value = "";
              }
            }}
            class="h-8 w-full rounded-md border border-input bg-background px-2 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            <option value="">Choose a playlist…</option>
            {#each allPlaylists as pl (pl.id)}
              <option value={pl.id}>{pl.name} ({pl.item_count})</option>
            {/each}
          </select>
        {/if}

        {#if showNewPlaylistField}
          <div class="mt-1.5 flex gap-1.5">
            <Input
              bind:value={newPlaylistName}
              placeholder="New playlist name"
              class="h-8 text-xs"
              onkeydown={(e) => {
                if (e.key === "Enter") createPlaylistAndAdd();
                if (e.key === "Escape") { showNewPlaylistField = false; newPlaylistName = ""; }
              }}
            />
            <Button size="sm" onclick={createPlaylistAndAdd} disabled={!newPlaylistName.trim()}>Add</Button>
            <Button size="sm" variant="ghost" onclick={() => { showNewPlaylistField = false; newPlaylistName = ""; }}>Cancel</Button>
          </div>
        {:else}
          <button
            type="button"
            class="mt-1.5 flex items-center gap-1 text-xs text-primary hover:underline"
            onclick={() => (showNewPlaylistField = true)}
          >
            <Plus class="size-3" />
            Create new playlist
          </button>
        {/if}

        {#if addedPlaylistToast}
          <p class="mt-1 text-xs text-primary">{addedPlaylistToast}</p>
        {/if}
      </div>

      <Separator class="my-4" />

      <!-- files -->
      <div>
        <div class="mb-1 text-xs font-medium text-muted-foreground">
          Files ({detail.files.length})
        </div>
        <ul class="space-y-2">
          {#each detail.files as f (f.id)}
            <li class="rounded-md border border-border bg-background/50 p-2 text-xs">
              <div class="flex items-center gap-2 truncate font-mono">
                <Film class="size-3 shrink-0 text-muted-foreground" />
                <span class="truncate">{f.path.split(/[\\/]/).pop()}</span>
              </div>
              <div class="mt-1 flex flex-wrap gap-x-3 gap-y-0.5 text-muted-foreground">
                {#if f.duration}<span>{fmtDuration(f.duration)}</span>{/if}
                {#if f.width && f.height}<span>{f.width}×{f.height}</span>{/if}
                {#if f.codec}<span class="uppercase">{f.codec}</span>{/if}
                {#if f.format_name}<span>{f.format_name}</span>{/if}
                <span>{fmtSize(f.size_bytes)}</span>
              </div>
              <div class="mt-0.5 truncate text-[10px] text-muted-foreground/70">{f.path}</div>
            </li>
          {/each}
        </ul>
      </div>

      <Separator class="my-4" />
      <div class="flex justify-between text-[10px] text-muted-foreground/70">
        <span>Plays: {detail.scene.play_count}</span>
        <span>Added: {new Date(detail.scene.created_at).toLocaleDateString()}</span>
      </div>
    </div>
  {/if}
</aside>
