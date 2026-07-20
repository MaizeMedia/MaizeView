<script lang="ts">
  import { onMount } from "svelte";
  import { Loader2, CheckSquare, ListPlus, Plus, Play, Sparkles, Trash2, Zap, X, Grid2x2 } from "@lucide/svelte";
  import VirtualSceneGrid from "./virtual-scene-grid.svelte";
  import EmptyState from "$components/empty-state/empty-state.svelte";
  import ConvertDialog from "./convert-dialog.svelte";
  import { library } from "$lib/stores/library.svelte";
  import Button from "$components/ui/button/button.svelte";
  import Input from "$components/ui/input/input.svelte";
  import Separator from "$components/ui/separator/separator.svelte";
  import { confirm } from "@tauri-apps/plugin-dialog";
  import { stringifyError } from "$lib/utils";
  import {
    playlists as playlistsApi,
    scanPaths,
    scenes as scenesApi,
    openPlayerWindow,
    openQuadWithScenes,
    closeAllPlayerWindows,
    type PlaylistRow,
  } from "$lib/api";

  let hasPaths = $state(true);
  let allPlaylists = $state<PlaylistRow[]>([]);
  let multiToast = $state<string | null>(null);
  let deletingSelection = $state(false);
  let closingPlayers = $state(false);

  // Convert / downscale dialog (progress banner lives in App via library store).
  let showConvert = $state(false);
  let convertIds = $state<string[]>([]);

  async function checkPaths() {
    try {
      const p = await scanPaths.list();
      hasPaths = p.length > 0;
    } catch {
      hasPaths = false;
    }
  }

  async function refreshPlaylists() {
    try {
      allPlaylists = await playlistsApi.list();
    } catch {
      allPlaylists = [];
    }
  }

  function goToSettings() {
    library.view = "settings";
  }

  async function enterSelectMode() {
    await refreshPlaylists();
    library.enterSelectionMode();
    multiToast = null;
  }

  function cancelSelect() {
    library.exitSelectionMode();
    showNewPlaylistField = false;
    newPlaylistName = "";
  }

  function showToast(msg: string) {
    multiToast = msg;
    setTimeout(() => (multiToast = null), 3500);
  }

  async function addToTargetPlaylist(playlistId: string) {
    if (!playlistId || library.selectionCount === 0) return;
    const ids = [...library.selectedIds];
    const pl = allPlaylists.find((p) => p.id === playlistId);
    const name = pl?.name ?? "playlist";
    try {
      let inserted = 0;
      let already = 0;
      for (const sceneId of ids) {
        if (await playlistsApi.add(playlistId, sceneId)) inserted += 1;
        else already += 1;
      }
      showToast(
        already === 0
          ? `Added ${inserted} scene${inserted === 1 ? "" : "s"} to “${name}”`
          : inserted === 0
            ? `${already} scene${already === 1 ? "" : "s"} already in “${name}”`
            : `${inserted} added, ${already} already in “${name}”`,
      );
      cancelSelect();
      await refreshPlaylists();
    } catch (e) {
      showToast(stringifyError(e));
    }
  }

  async function playSelection() {
    if (library.selectionCount === 0) return;
    const ids = [...library.selectedIds];
    try {
      await openPlayerWindow({
        sceneId: ids[0],
        sceneIds: ids,
        // Always open a fresh window so the staged queue isn't discarded when
        // a window for the first scene already exists.
        forceNewWindow: true,
        title: `MaizeView — ${ids.length} scenes`,
      });
      cancelSelect();
    } catch (e) {
      showToast(`Failed to open player: ${stringifyError(e)}`);
    }
  }

  async function playSelectionQuad() {
    if (library.selectionCount === 0) return;
    const ids = [...library.selectedIds];
    try {
      await openQuadWithScenes(ids);
      cancelSelect();
    } catch (e) {
      showToast(`Failed to open 4Play: ${stringifyError(e)}`);
    }
  }

  async function closeAllPlayers() {
    if (closingPlayers) return;
    closingPlayers = true;
    try {
      const n = await closeAllPlayerWindows();
      showToast(n === 0 ? "No player windows open" : `Closed ${n} player window${n === 1 ? "" : "s"}`);
    } catch (e) {
      showToast(`Failed to close players: ${stringifyError(e)}`);
    } finally {
      closingPlayers = false;
    }
  }

  let newPlaylistName = $state("");
  let showNewPlaylistField = $state(false);

  async function createAndAddSelection() {
    if (!newPlaylistName.trim() || library.selectionCount === 0) return;
    try {
      const row = await playlistsApi.create(newPlaylistName.trim());
      for (const sceneId of [...library.selectedIds]) {
        await playlistsApi.add(row.id, sceneId);
      }
      showToast(`Created “${row.name}” with ${library.selectionCount} scene${library.selectionCount === 1 ? "" : "s"}`);
      newPlaylistName = "";
      showNewPlaylistField = false;
      cancelSelect();
      await refreshPlaylists();
    } catch (e) {
      showToast(stringifyError(e));
    }
  }

  async function deleteSelection() {
    if (library.selectionCount === 0 || deletingSelection) return;
    const ids = [...library.selectedIds];
    const labels = library.selectedSceneLabels();
    const preview = labels.slice(0, 8).map((l) => `• ${l}`).join("\n");
    const overflow = labels.length > 8 ? `\n…and ${labels.length - 8} more` : "";
    const ok = await confirm(
      `Delete ${ids.length} scene${ids.length === 1 ? "" : "s"} permanently?\n\n${preview}${overflow}\n\nThis removes the video file(s) from disk and from your library.`,
      { title: "Delete from library", kind: "warning", okLabel: "Delete", cancelLabel: "Cancel" },
    );
    if (!ok) return;

    deletingSelection = true;
    library.suppressDeleteRefresh = true;
    try {
      const result = await scenesApi.deleteMany(ids);
      library.selectedIds = new Set();
      await library.refresh();
      if (result.failed.length === 0) {
        showToast(`Deleted ${result.deleted} scene${result.deleted === 1 ? "" : "s"}`);
        cancelSelect();
      } else if (result.deleted > 0) {
        showToast(
          `Deleted ${result.deleted}/${ids.length}. ${result.failed.length} failed: ${result.failed[0]?.error ?? "unknown error"}`,
        );
      } else {
        showToast(result.failed[0]?.error ?? "Delete failed");
      }
    } catch (e) {
      showToast(stringifyError(e));
    } finally {
      library.suppressDeleteRefresh = false;
      deletingSelection = false;
    }
  }

  function openConvert() {
    if (library.selectionCount === 0) return;
    convertIds = [...library.selectedIds];
    showConvert = true;
  }

  onMount(() => {
    void library.refresh();
    void library.ensureProgressListener();
    void library.ensureBatchIdentifyListener();
    void library.ensureTranscodeListener();
    void checkPaths();
  });
</script>

<div class="relative flex h-full min-h-0 flex-col" data-testid="library-grid">
  {#if multiToast}
    <div class="mb-3 rounded-md border border-primary/30 bg-primary/10 px-4 py-2.5 text-sm text-foreground">
      {multiToast}
    </div>
  {/if}

  {#if library.scenes.length > 0 || library.sceneTotal > 0 || library.search.trim() || library.hasFilters}
    {#if library.selectionMode}
      <!-- Selection bar: full-width, pinned above the grid -->
      <div
        class="sticky top-0 z-20 -mx-2 mb-4 rounded-lg border border-primary/25 bg-card px-4 py-3 shadow-sm"
      >
        <div class="flex flex-wrap items-center gap-x-4 gap-y-2">
          <div class="flex items-center gap-2">
            <CheckSquare class="size-5 shrink-0 text-primary" />
            <span class="text-sm font-semibold">
              {library.selectionCount} selected
            </span>
            {#if library.selectionCount > 0 && library.selectionCount <= 4}
              <span class="hidden text-xs text-muted-foreground sm:inline">
                — {library.selectedSceneLabels().join(", ")}
              </span>
            {/if}
          </div>

          <div class="flex flex-wrap items-center gap-1.5">
            <Button variant="ghost" size="sm" onclick={() => void library.selectAll()}>
              Select all ({library.scenesFullyLoaded ? library.sceneTotal.toLocaleString() : `${library.scenes.length.toLocaleString()}…`})
            </Button>
            <Button variant="ghost" size="sm" onclick={() => library.selectNone()}>Clear</Button>
          </div>

          <Separator orientation="vertical" class="hidden h-6 sm:block" />

          <div class="flex flex-wrap items-center gap-1.5">
            <Button size="sm" onclick={playSelection} disabled={library.selectionCount === 0} title="Play selected in a new window">
              <Play class="size-3.5" fill="currentColor" />
              Play
            </Button>
            <Button
              size="sm"
              variant="outline"
              onclick={() => void playSelectionQuad()}
              disabled={library.selectionCount === 0}
              title="4Play: watch up to 4 selected scenes at once in a quadrant window (rotates through the rest on EOF)"
            >
              <Grid2x2 class="size-3.5" />
              4Play
            </Button>
            <Button
              size="sm"
              variant="outline"
              onclick={() => void closeAllPlayers()}
              disabled={closingPlayers}
              title="Close all open player windows"
            >
              {#if closingPlayers}
                <Loader2 class="size-3.5 animate-spin" />
              {:else}
                <X class="size-3.5" />
              {/if}
              Close all
            </Button>
            <Button
              size="sm"
              variant="outline"
              onclick={() => library.batchIdentifySelected(true)}
              disabled={library.selectionCount === 0 || library.batchIdentifying}
              title="Search StashDB for selected scenes; auto-apply when exactly one match"
            >
              {#if library.batchIdentifying}
                <Loader2 class="size-3.5 animate-spin" />
              {:else}
                <Sparkles class="size-3.5" />
              {/if}
              Identify
            </Button>
            <Button
              size="sm"
              variant="outline"
              onclick={openConvert}
              disabled={library.selectionCount === 0}
              title="Downscale selected videos to save space"
            >
              <Zap class="size-3.5" />
              Convert…
            </Button>
            <Button
              size="sm"
              variant="outline"
              class="border-destructive/40 text-destructive hover:bg-destructive/10"
              onclick={deleteSelection}
              disabled={library.selectionCount === 0 || deletingSelection}
              title="Delete video files from disk and remove from library"
            >
              {#if deletingSelection}
                <Loader2 class="size-3.5 animate-spin" />
              {:else}
                <Trash2 class="size-3.5" />
              {/if}
              Delete
            </Button>
          </div>

          <div class="ml-auto">
            <Button variant="ghost" size="sm" onclick={cancelSelect}>Done</Button>
          </div>
        </div>

        <div class="mt-3 flex flex-wrap items-center gap-2 border-t border-border/70 pt-3">
          <div class="flex items-center gap-1.5 text-sm font-medium text-foreground">
            <ListPlus class="size-4 text-primary" />
            Add to playlist
          </div>

          {#if allPlaylists.length > 0}
            <select
              value=""
              disabled={library.selectionCount === 0}
              onchange={(e) => {
                const v = (e.currentTarget as HTMLSelectElement).value;
                if (v) addToTargetPlaylist(v);
                (e.currentTarget as HTMLSelectElement).value = "";
              }}
              class="h-9 min-w-[12rem] flex-1 rounded-md border border-input bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50 sm:max-w-xs sm:flex-none"
            >
              <option value="">Choose a playlist…</option>
              {#each allPlaylists as pl (pl.id)}
                <option value={pl.id}>{pl.name} ({pl.item_count})</option>
              {/each}
            </select>
          {:else if !showNewPlaylistField}
            <span class="text-sm text-muted-foreground">No playlists yet — create one below.</span>
          {/if}

          {#if showNewPlaylistField}
            <Input
              bind:value={newPlaylistName}
              placeholder="New playlist name"
              class="h-9 min-w-[10rem] flex-1 sm:max-w-xs"
              onkeydown={(e: KeyboardEvent) => {
                if (e.key === "Enter") createAndAddSelection();
                if (e.key === "Escape") {
                  showNewPlaylistField = false;
                  newPlaylistName = "";
                }
              }}
            />
            <Button size="sm" onclick={createAndAddSelection} disabled={!newPlaylistName.trim() || library.selectionCount === 0}>
              Create & add
            </Button>
            <Button
              size="sm"
              variant="ghost"
              onclick={() => {
                showNewPlaylistField = false;
                newPlaylistName = "";
              }}
            >
              Cancel
            </Button>
          {:else}
            <Button
              variant="outline"
              size="sm"
              onclick={() => (showNewPlaylistField = true)}
              disabled={library.selectionCount === 0}
            >
              <Plus class="size-3.5" />
              New playlist
            </Button>
          {/if}
        </div>
      </div>
    {:else}
      <div class="mb-3 flex items-center justify-between gap-2">
        <div class="text-xs text-muted-foreground" data-testid="scene-count">
          {#if library.hasFilters || library.search.trim()}
            {#if library.loadingMore}
              {library.scenes.length.toLocaleString()} / {library.sceneTotal.toLocaleString()} loaded
            {:else}
              {library.sceneTotal.toLocaleString()} match{library.sceneTotal === 1 ? "" : "es"}
            {/if}
          {:else if library.loadingMore}
            {library.scenes.length.toLocaleString()} / {library.sceneTotal.toLocaleString()} scenes loaded
          {:else}
            {library.sceneTotal.toLocaleString()} scene{library.sceneTotal === 1 ? "" : "s"}
          {/if}
        </div>
        <div class="flex items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            onclick={() => void closeAllPlayers()}
            disabled={closingPlayers}
            title="Close all open player windows"
          >
            {#if closingPlayers}
              <Loader2 class="size-4 animate-spin" />
            {:else}
              <X class="size-4" />
            {/if}
            Close all
          </Button>
          <Button
            variant="outline"
            size="sm"
            onclick={() => library.batchIdentifyLibrary()}
            disabled={library.batchIdentifying}
            data-testid="stashdb-batch"
            title="Query StashDB for all scenes; auto-apply when exactly one match"
          >
            {#if library.batchIdentifying}
              <Loader2 class="size-4 animate-spin" />
            {:else}
              <Sparkles class="size-4" />
            {/if}
            Tag from StashDB
          </Button>
          <Button variant="outline" size="sm" onclick={enterSelectMode} data-testid="select-mode">
            <CheckSquare class="size-4" />
            Select
          </Button>
        </div>
      </div>
    {/if}
  {/if}

  <div class="min-h-0 flex-1">
    {#if library.loading && library.scenes.length === 0}
      <div class="flex h-full items-center justify-center text-muted-foreground">
        <Loader2 class="size-6 animate-spin" />
      </div>
    {:else if library.scenes.length === 0}
      {#if !hasPaths}
        <EmptyState
          title="No library folders yet"
          hint="Add a folder in Settings, then run a scan."
          actionLabel="Open Settings"
          onAction={goToSettings}
        />
      {:else if library.search.trim() || library.hasFilters}
        <EmptyState
          title="No matches"
          hint="Try clearing search or filters."
        />
      {:else}
        <EmptyState
          title="No scenes found"
          hint="Try a scan to populate your library."
          actionLabel="Scan library"
          onAction={() => library.startScan()}
        />
      {/if}
    {:else}
      <div class="h-full min-h-0" data-testid="scene-grid-viewport">
        <VirtualSceneGrid scenes={library.scenes} />
      </div>
    {/if}
  </div>

  {#if library.error}
    <div class="mt-4 rounded-md border border-destructive/40 bg-destructive/10 px-4 py-3 text-sm text-destructive">
      {library.error}
    </div>
  {/if}

  {#if showConvert}
    <ConvertDialog sceneIds={convertIds} onclose={() => (showConvert = false)} />
  {/if}
</div>
