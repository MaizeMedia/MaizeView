<script lang="ts">
  import { onMount } from "svelte";
  import { Plus, Shuffle, Trash2, ChevronLeft, ListMusic, GripVertical, Loader2, Play, CheckSquare, X, Pencil, Grid2x2 } from "@lucide/svelte";
  import { confirm } from "@tauri-apps/plugin-dialog";
  import { stringifyError } from "$lib/utils";
  import Button from "$components/ui/button/button.svelte";
  import Input from "$components/ui/input/input.svelte";
  import EmptyState from "$components/empty-state/empty-state.svelte";
  import SceneCard from "$components/library/scene-card.svelte";
  import VirtualSceneGrid from "$components/library/virtual-scene-grid.svelte";
  import { shuffleWeight, weightedPickId, type ShuffleMeta } from "$lib/shuffle";
  import { playlists as playlistsApi, openPlayerWindow, openQuadWithScenes, closeAllPlayerWindows, scenes, type PlaylistRow, type SceneGridRow } from "$lib/api";

  let all = $state<PlaylistRow[]>([]);
  let activeId = $state<string | null>(null);
  let items = $state<SceneGridRow[]>([]);
  let loading = $state(false);
  let loadingItems = $state(false);
  let creating = $state(false);
  let newName = $state("");
  let error = $state<string | null>(null);

  const OPEN_WINDOW_LIMIT = 16;
  let selectMode = $state(false);
  let selectedIds = $state<Set<string>>(new Set());
  let openingSelected = $state(false);
  let closingPlayers = $state(false);

  async function loadAll() {
    loading = true;
    try {
      all = await playlistsApi.list();
    } catch (e) {
      error = stringifyError(e);
    } finally {
      loading = false;
    }
  }

  async function open(id: string) {
    activeId = id;
    selectMode = false;
    selectedIds = new Set();
    items = [];
    loadingItems = true;
    error = null;
    try {
      items = await playlistsApi.items(id);
    } catch (e) {
      error = stringifyError(e);
    } finally {
      loadingItems = false;
    }
  }

  function back() {
    activeId = null;
    selectMode = false;
    selectedIds = new Set();
    void loadAll();
  }

  function exitSelectMode() {
    selectMode = false;
    selectedIds = new Set();
    error = null;
  }

  function togglePlaylistSelect(sceneId: string) {
    const next = new Set(selectedIds);
    if (next.has(sceneId)) {
      next.delete(sceneId);
    } else if (next.size >= OPEN_WINDOW_LIMIT) {
      error = `Select up to ${OPEN_WINDOW_LIMIT} scenes to open at once.`;
      return;
    } else {
      next.add(sceneId);
    }
    selectedIds = next;
    error = null;
  }

  /** Open up to OPEN_WINDOW_LIMIT selected scenes, each in its own player window. */
  async function openSelectedInWindows() {
    const toOpen = items.filter((s) => selectedIds.has(s.id) && s.file_path);
    if (toOpen.length === 0) {
      error = "Selected scenes have no playable file path.";
      return;
    }
    const pl = all.find((p) => p.id === activeId);
    openingSelected = true;
    error = null;
    let opened = 0;
    for (const scene of toOpen.slice(0, OPEN_WINDOW_LIMIT)) {
      try {
        await openPlayerWindow({
          sceneId: scene.id,
          filePath: scene.file_path!,
          // Always a new window so Open selected works repeatedly.
          forceNewWindow: true,
          title: `MaizeView — ${scene.title ?? pl?.name ?? "Player"}`,
        });
        opened += 1;
        await new Promise((r) => setTimeout(r, 250));
      } catch (e) {
        error = `Stopped after ${opened} window${opened === 1 ? "" : "s"}: ${stringifyError(e)}`;
        openingSelected = false;
        return;
      }
    }
    openingSelected = false;
    exitSelectMode();
  }

  // Set by every local mutation so the playlist://changed listener can skip
  // the echo of our own writes (they're already applied to local state).
  let lastLocalMutationAt = 0;

  async function removePlaylist() {
    if (!activeId) return;
    const pl = all.find((p) => p.id === activeId);
    const name = pl?.name ?? "playlist";
    const ok = await confirm(`Delete playlist “${name}”? Scenes stay in the library.`, {
      title: "Delete playlist",
      kind: "warning",
    });
    if (!ok) return;
    lastLocalMutationAt = Date.now();
    try {
      await playlistsApi.delete(activeId);
      activeId = null;
      items = [];
      selectMode = false;
      selectedIds = new Set();
      await loadAll();
    } catch (e) {
      error = stringifyError(e);
    }
  }

  async function create() {
    if (!newName.trim()) return;
    lastLocalMutationAt = Date.now();
    try {
      const row = await playlistsApi.create(newName.trim());
      newName = "";
      creating = false;
      await loadAll();
      await open(row.id);
    } catch (e) {
      error = stringifyError(e);
    }
  }

  // Inline rename of the open playlist (detail header pencil).
  let renaming = $state(false);
  let renameDraft = $state("");
  let renameInput = $state<HTMLInputElement | null>(null);

  function startRename() {
    const active = all.find((p) => p.id === activeId);
    if (!active) return;
    renameDraft = active.name;
    renaming = true;
  }

  function cancelRename() {
    renaming = false;
    renameDraft = "";
  }

  async function commitRename() {
    const active = all.find((p) => p.id === activeId);
    const name = renameDraft.trim();
    renaming = false;
    renameDraft = "";
    // Blur after Escape, emptied input, or unchanged name: nothing to do.
    if (!active || !activeId || !name || name === active.name) return;
    lastLocalMutationAt = Date.now();
    try {
      await playlistsApi.rename(activeId, name);
      all = all.map((p) => (p.id === activeId ? { ...p, name } : p));
    } catch (e) {
      error = stringifyError(e);
    }
  }

  // Close the rename input whenever the open playlist changes (or closes).
  $effect(() => {
    void activeId;
    renaming = false;
  });

  // Focus + select the rename input when it appears.
  $effect(() => {
    if (renaming && renameInput) {
      renameInput.focus();
      renameInput.select();
    }
  });

  async function remove(sceneId: string) {
    if (!activeId) return;
    lastLocalMutationAt = Date.now();
    try {
      await playlistsApi.remove(activeId, sceneId);
      items = items.filter((s) => s.id !== sceneId);
      await loadAll();
    } catch (e) {
      error = stringifyError(e);
    }
  }

  // Play the whole playlist in one window (queue mode, ADR-011).
  async function pickWeightedStart(candidates: SceneGridRow[]): Promise<SceneGridRow> {
    const meta: Record<string, ShuffleMeta> = {};
    try {
      const rows = await scenes.shuffleMeta(candidates.map((s) => s.id));
      for (const row of rows) {
        meta[row.id] = {
          favorite: row.favorite ?? 0,
          lastPlayedAt: row.last_played_at,
        };
      }
    } catch {
      for (const s of candidates) {
        meta[s.id] = { favorite: s.favorite ?? 0, lastPlayedAt: null };
      }
    }
    const id = weightedPickId(candidates.map((s) => s.id), (sid) =>
      shuffleWeight(meta[sid] ?? { favorite: 0, lastPlayedAt: null }),
    );
    return candidates.find((s) => s.id === id) ?? candidates[0];
  }

  async function playPlaylist() {
    if (items.length === 0 || !activeId) return;
    const pl = all.find((p) => p.id === activeId);
    if (!pl) return;
    const playable = items.filter((s) => s.file_path);
    if (playable.length === 0) {
      error = "No playable files in this playlist (offline or missing paths).";
      return;
    }
    const sceneIds = items.map((s) => s.id);
    const start = pl.shuffle_by_default
      ? await pickWeightedStart(playable)
      : playable[0];
    try {
      await openPlayerWindow({
        sceneId: start.id,
        filePath: start.file_path ?? undefined,
        sceneIds,
        shuffleByDefault: pl.shuffle_by_default,
        // Always open a fresh window so Play can be used repeatedly.
        forceNewWindow: true,
        title: `MaizeView — ${pl.name}`,
      });
    } catch (e) {
      error = `Failed to open player: ${stringifyError(e)}`;
    }
  }

  // 4Play: one window playing up to 4 quadrant videos, rotating through the
  // rest of the playable list on EOF. Needs ≥4 playable items to enable.
  const playableQuadCount = $derived(items.filter((s) => s.file_path).length);

  async function openQuad() {
    const playable = items.filter((s) => s.file_path);
    if (playable.length < 4) return;
    const pl = all.find((p) => p.id === activeId);
    try {
      await openQuadWithScenes(playable.map((s) => s.id), pl?.shuffle_by_default ?? false);
    } catch (e) {
      error = `Failed to open 4Play: ${stringifyError(e)}`;
    }
  }

  async function closeAllPlayers() {
    if (closingPlayers) return;
    closingPlayers = true;
    error = null;
    try {
      const n = await closeAllPlayerWindows();
      // Surface result so a silent ACL/no-op isn't mistaken for success.
      if (n === 0) {
        error = "No player windows were open.";
        setTimeout(() => {
          if (error === "No player windows were open.") error = null;
        }, 2500);
      }
    } catch (e) {
      error = `Failed to close players: ${stringifyError(e)}`;
    } finally {
      closingPlayers = false;
    }
  }

  // Drag-to-reorder (HTML5 DnD, no library).
  let dragSrc = $state<string | null>(null);
  function onDragStart(e: DragEvent, sceneId: string) {
    dragSrc = sceneId;
    e.dataTransfer?.setData("text/plain", sceneId);
  }
  function onDragOver(e: DragEvent) {
    e.preventDefault();
  }
  async function onDrop(e: DragEvent, _target: string) {
    e.preventDefault();
    const src = dragSrc ?? e.dataTransfer?.getData("text/plain");
    if (!src || src === _target || !activeId) return;
    const order = items.map((s) => s.id);
    const from = order.indexOf(src);
    const to = order.indexOf(_target);
    if (from < 0 || to < 0) return;
    order.splice(from, 1);
    order.splice(to, 0, src);
    // Optimistic reorder.
    const map = new Map(items.map((s) => [s.id, s] as const));
    items = order.map((id) => map.get(id)!);
    dragSrc = null;
    lastLocalMutationAt = Date.now();
    try {
      await playlistsApi.reorder(activeId, order);
    } catch (err) {
      error = stringifyError(err);
      await open(activeId); // re-sync on failure
    }
  }

  // playlist://changed — fired by every playlist mutation in ANY window
  // (player add, drawer add, this view's own writes). Always refresh the list
  // (counts change); reload the open detail pane's items unless it's the echo
  // of our own recent mutation or a drag-reorder is in flight.
  async function onPlaylistChanged(playlistId: string) {
    await loadAll();
    if (!activeId || activeId !== playlistId) return;
    if (!all.some((p) => p.id === playlistId)) {
      // The open playlist was deleted (e.g. from another window) — close the pane.
      activeId = null;
      items = [];
      selectMode = false;
      selectedIds = new Set();
      return;
    }
    if (dragSrc) return;
    if (Date.now() - lastLocalMutationAt < 750) return;
    try {
      items = await playlistsApi.items(playlistId);
    } catch {
      // Playlist vanished between the list refresh and the items load.
      activeId = null;
      items = [];
      selectMode = false;
      selectedIds = new Set();
    }
  }

  onMount(() => {
    void loadAll();
    let unlistenDeleted: (() => void) | null = null;
    let unlistenChanged: (() => void) | null = null;
    void scenes.onDeleted((p) => {
      items = items.filter((s) => s.id !== p.scene_id);
      void loadAll();
    }).then((fn) => {
      unlistenDeleted = fn;
    });
    void playlistsApi.onChanged((p) => {
      void onPlaylistChanged(p.playlist_id);
    }).then((fn) => {
      unlistenChanged = fn;
    });
    return () => {
      unlistenDeleted?.();
      unlistenChanged?.();
    };
  });
</script>

<section class="mx-auto flex h-full w-full max-w-6xl min-h-0 flex-col gap-4">
  {#if error}
    <div class="shrink-0 rounded-md border border-destructive/40 bg-destructive/10 px-4 py-3 text-sm text-destructive">
      {error}
    </div>
  {/if}

  {#if !activeId}
    <!-- Playlists list -->
    <div class="min-h-0 flex-1 space-y-6 overflow-y-auto">
    <header class="flex items-center justify-between">
      <h1 class="text-2xl font-semibold tracking-tight">Playlists</h1>
      <Button variant="outline" size="sm" onclick={() => (creating = !creating)}>
        <Plus class="size-4" />
        New playlist
      </Button>
    </header>

    {#if creating}
      <div class="flex gap-2">
        <Input bind:value={newName} placeholder="Playlist name" onkeydown={(e) => e.key === "Enter" && create()} />
        <Button onclick={create} disabled={!newName.trim()}>Create</Button>
        <Button variant="ghost" onclick={() => { creating = false; newName = ""; }}>Cancel</Button>
      </div>
    {/if}

    {#if loading}
      <div class="flex justify-center py-12 text-muted-foreground">
        <Loader2 class="size-6 animate-spin" />
      </div>
    {:else if all.length === 0}
      <EmptyState title="No playlists yet" hint="Create a playlist, then add scenes from the detail drawer." />
    {:else}
      <ul class="grid grid-cols-1 gap-2 sm:grid-cols-2 lg:grid-cols-3">
        {#each all as p (p.id)}
          <li>
            <button
              type="button"
              onclick={() => open(p.id)}
              class="flex w-full items-center gap-3 rounded-lg border border-border bg-card px-4 py-3 text-left transition-colors hover:border-primary/50 hover:bg-accent"
            >
              <ListMusic class="size-5 text-muted-foreground" />
              <div class="min-w-0 flex-1">
                <div class="truncate text-sm font-medium">{p.name}</div>
                <div class="text-xs text-muted-foreground">{p.item_count} scenes</div>
              </div>
            </button>
          </li>
        {/each}
      </ul>
    {/if}
    </div>
  {:else}
    <!-- Single playlist view -->
    {@const active = all.find((p) => p.id === activeId)}
    <header class="flex shrink-0 flex-wrap items-center gap-3">
      <Button variant="ghost" size="icon-sm" onclick={back} aria-label="Back">
        <ChevronLeft class="size-5" />
      </Button>
      {#if renaming}
        <Input
          bind:ref={renameInput}
          bind:value={renameDraft}
          class="h-9 min-w-0 flex-1 text-lg font-semibold"
          aria-label="Rename playlist"
          onkeydown={(e: KeyboardEvent) => {
            if (e.key === "Enter") void commitRename();
            else if (e.key === "Escape") cancelRename();
          }}
          onblur={() => void commitRename()}
        />
      {:else}
        <h1 class="min-w-0 flex-1 text-2xl font-semibold tracking-tight">
          {active?.name ?? "Playlist"}
          {#if items.length > 0}
            <span class="ml-2 text-base font-normal text-muted-foreground">{items.length}</span>
          {/if}
        </h1>
        <Button
          variant="ghost"
          size="icon-sm"
          onclick={startRename}
          aria-label="Rename playlist"
          title="Rename playlist"
        >
          <Pencil class="size-4" />
        </Button>
      {/if}

      <Button
        size="sm"
        data-testid="playlist-play"
        title={active?.shuffle_by_default
          ? "Open a new player at a random scene (weighted by favorite). Click again for another."
          : "Play playlist from the first scene (queue). Click again for another window."}
        disabled={items.length === 0 || loadingItems}
        onclick={() => void playPlaylist()}
      >
        <Play class="size-4" fill="currentColor" />
        Play
      </Button>

      <!-- 4Play: one window, up to 4 quadrant videos, rotating through the
           rest of the playable list on EOF. Needs ≥4 playable items. -->
      <Button
        size="sm"
        variant="outline"
        data-testid="playlist-4play"
        title="4Play: one window with 4 quadrant videos — plays the playable list 4 at a time, rotating on EOF"
        disabled={playableQuadCount < 4 || loadingItems}
        onclick={() => void openQuad()}
      >
        <Grid2x2 class="size-4" />
        4Play
      </Button>

      <Button
        size="sm"
        variant="outline"
        data-testid="playlist-close-all"
        title="Close all open player windows"
        disabled={closingPlayers}
        onclick={() => void closeAllPlayers()}
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
        data-testid="playlist-delete"
        title="Delete this playlist"
        onclick={() => void removePlaylist()}
      >
        <Trash2 class="size-4" />
        Delete
      </Button>

      {#if selectMode}
        <Button
          variant="default"
          size="sm"
          title={`Open up to ${OPEN_WINDOW_LIMIT} selected scenes in separate windows`}
          disabled={selectedIds.size === 0 || openingSelected}
          onclick={() => void openSelectedInWindows()}
        >
          {#if openingSelected}
            <Loader2 class="size-4 animate-spin" />
          {:else}
            <Play class="size-4" />
          {/if}
          Open selected ({selectedIds.size})
        </Button>
        <Button variant="ghost" size="sm" onclick={exitSelectMode}>
          <X class="size-4" />
          Done
        </Button>
      {:else}
        <Button
          variant="outline"
          size="sm"
          title={`Pick up to ${OPEN_WINDOW_LIMIT} scenes to open in separate windows`}
          disabled={items.length === 0 || loadingItems}
          onclick={() => { selectMode = true; selectedIds = new Set(); error = null; }}
        >
          <CheckSquare class="size-4" />
          Select to open
        </Button>
      {/if}

      <!-- Shuffle toggle. This is the default state playback windows inherit
           when they start from this playlist (ADR-011: shuffle is per-window;
           this flag is the default). Shuffling itself happens at the player. -->
      <Button
        variant={active?.shuffle_by_default ? "default" : "outline"}
        size="sm"
        title="Shuffle on/off — sets the default for playback windows"
        disabled={!activeId}
        onclick={async () => {
          if (!activeId || !active) return;
          const next = !active.shuffle_by_default;
          all = all.map((p) => p.id === activeId ? { ...p, shuffle_by_default: next } : p);
          try { await playlistsApi.setShuffleDefault(activeId, next); } catch (err) { error = stringifyError(err); await loadAll(); }
        }}
      >
        <Shuffle class="size-4" />
        Shuffle
      </Button>
    </header>

    {#if selectMode}
      <p class="shrink-0 text-xs text-muted-foreground">
        Select up to {OPEN_WINDOW_LIMIT} scenes, then click Open selected. Use Play for the full playlist in one window.
      </p>
    {:else if active?.shuffle_by_default}
      <p class="shrink-0 text-xs text-muted-foreground">
        Shuffle is on — Play opens a new window at a random scene (favorites weighted higher). Play again anytime for another pick.
      </p>
    {/if}

    {#if loadingItems}
      <div class="flex min-h-0 flex-1 items-center justify-center text-muted-foreground">
        <Loader2 class="size-6 animate-spin" />
      </div>
    {:else if items.length === 0}
      <EmptyState title="This playlist is empty" hint="Add scenes from the library using the detail drawer." />
    {:else}
      <div class="min-h-0 flex-1" data-testid="playlist-grid-viewport">
        <VirtualSceneGrid scenes={items} metaHeight={88}>
          {#snippet item(scene)}
            <div
              class="group relative"
              draggable={!selectMode}
              ondragstart={(e) => onDragStart(e, scene.id)}
              ondragover={onDragOver}
              ondrop={(e) => onDrop(e, scene.id)}
              role="listitem"
            >
              {#if !selectMode}
                <div class="absolute left-1 top-1 z-10 cursor-grab rounded bg-black/40 p-1 opacity-0 transition-opacity group-hover:opacity-100" title="Drag to reorder">
                  <GripVertical class="size-3.5 text-white" />
                </div>
              {/if}
              <SceneCard
                {scene}
                selectable={selectMode}
                selected={selectedIds.has(scene.id)}
                onToggleSelect={() => togglePlaylistSelect(scene.id)}
              />
              {#if !selectMode}
                <button
                  type="button"
                  class="absolute right-1 top-1 z-10 flex size-6 items-center justify-center rounded-full bg-black/60 text-white/80 opacity-0 transition-opacity hover:bg-destructive hover:text-white group-hover:opacity-100"
                  aria-label="Remove from playlist"
                  onclick={() => remove(scene.id)}
                >
                  <Trash2 class="size-3.5" />
                </button>
              {/if}
            </div>
          {/snippet}
        </VirtualSceneGrid>
      </div>
    {/if}
  {/if}
</section>
