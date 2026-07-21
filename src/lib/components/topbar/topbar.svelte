<script lang="ts">
  import { Search, ArrowDownUp, Filter, X, Bookmark } from "@lucide/svelte";
  import Input from "$components/ui/input/input.svelte";
  import Button from "$components/ui/button/button.svelte";
  import Separator from "$components/ui/separator/separator.svelte";
  import FilterPanel from "./filter-panel.svelte";
  import { library } from "$lib/stores/library.svelte";
  import { stringifyError } from "$lib/utils";
  import {
    savedFilters as savedFiltersApi,
    type SavedFilterPayload,
    type SavedFilterRow,
  } from "$lib/api";
  import type { TagRow, PerformerRow, StudioRow } from "$lib/api/types";
  import type { SortBy } from "$lib/api";
  import { catalogs } from "$lib/stores/catalogs.svelte";
  import { onMount } from "svelte";

  // Debounced search → refresh.
  let searchTimer: ReturnType<typeof setTimeout> | null = null;

  $effect(() => {
    const _q = library.search;
    const _v = library.view;
    if (searchTimer) clearTimeout(searchTimer);
    searchTimer = setTimeout(() => {
      if (_v === "library" || _v === "favorites") {
        void library.refresh();
      }
    }, 250);
    void _v;
  });

  const sortOptions: { value: SortBy; label: string }[] = [
    { value: "created", label: "Newest" },
    { value: "favorite", label: "Favorite level" },
    { value: "playcount", label: "Most played" },
    { value: "title", label: "Title A–Z" },
  ];

  function onSortChange(e: Event) {
    library.sort = (e.currentTarget as HTMLSelectElement).value as SortBy;
    void library.refresh();
  }

  function onMinFavChange(e: Event) {
    library.minFavorite = Number((e.currentTarget as HTMLSelectElement).value);
    void library.refresh();
  }

  let filterOpen = $state(false);
  let savedOpen = $state(false);
  let allTags: TagRow[] = $derived(catalogs.tags);
  let allPerformers: PerformerRow[] = $derived(catalogs.performers);
  let allStudios: StudioRow[] = $derived(catalogs.studios);
  let savedList = $state<SavedFilterRow[]>([]);
  let saveName = $state("");
  let saveBusy = $state(false);

  async function refreshSaved() {
    savedList = await savedFiltersApi.list();
  }

  onMount(async () => {
    await Promise.all([catalogs.ensureLoaded(), refreshSaved()]);
  });

  $effect(() => {
    if (savedOpen) void refreshSaved();
  });

  function tagName(id: string): string {
    return allTags.find((t) => t.id === id)?.name ?? id.slice(0, 6);
  }
  function performerName(id: string): string {
    return allPerformers.find((p) => p.id === id)?.name ?? id.slice(0, 6);
  }
  function studioName(id: string): string {
    return allStudios.find((s) => s.id === id)?.name ?? id.slice(0, 6);
  }

  async function saveCurrent() {
    const name = saveName.trim();
    if (!name) return;
    saveBusy = true;
    try {
      await savedFiltersApi.create(name, library.snapshotFilterPayload());
      saveName = "";
      await refreshSaved();
    } catch (e) {
      library.error = stringifyError(e);
    } finally {
      saveBusy = false;
    }
  }

  function applySaved(row: SavedFilterRow) {
    try {
      const payload = JSON.parse(row.payload) as SavedFilterPayload;
      library.applyFilterPayload(payload);
      savedOpen = false;
    } catch (e) {
      library.error = stringifyError(e);
    }
  }

  async function deleteSaved(id: string) {
    try {
      await savedFiltersApi.delete(id);
      await refreshSaved();
    } catch (e) {
      library.error = stringifyError(e);
    }
  }
</script>

<header class="relative z-50 flex h-14 items-center gap-3 border-b border-border bg-background/80 px-4 backdrop-blur">
  <div class="relative max-w-xl flex-1">
    <Search class="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
    <Input
      bind:value={library.search}
      placeholder="Search… use -term to exclude (e.g. 1080p -wmv)"
      class="pl-9"
      data-testid="catalog-search"
    />
  </div>

  <div class="ml-auto flex items-center gap-2">
    <!-- Saved filters -->
    <div class="relative">
      <Button
        variant="outline"
        size="sm"
        data-testid="saved-filters-toggle"
        onclick={() => {
          savedOpen = !savedOpen;
          filterOpen = false;
        }}
      >
        <Bookmark class="size-4" />
        Saved
      </Button>
      {#if savedOpen}
        <button
          type="button"
          class="fixed inset-0 z-[90] cursor-default"
          aria-label="Close saved filters"
          onclick={() => (savedOpen = false)}
        ></button>
        <div
          class="absolute right-0 top-10 z-[100] w-80 space-y-3 rounded-lg border border-border bg-popover p-3 shadow-xl"
          data-testid="saved-filters-panel"
        >
          <div class="text-sm font-medium">Saved filters</div>
          <div class="flex gap-1.5">
            <input
              bind:value={saveName}
              placeholder="Name current filters…"
              class="h-8 flex-1 rounded-md border border-input bg-background px-2 text-xs"
              data-testid="saved-filter-name"
              onkeydown={(e) => {
                if (e.key === "Enter") void saveCurrent();
              }}
            />
            <Button
              size="sm"
              disabled={!saveName.trim() || saveBusy}
              onclick={() => void saveCurrent()}
              data-testid="saved-filter-save"
            >
              Save
            </Button>
          </div>
          {#if savedList.length === 0}
            <p class="text-xs text-muted-foreground">No saved filters yet.</p>
          {:else}
            <ul class="max-h-56 space-y-1 overflow-y-auto">
              {#each savedList as row (row.id)}
                <li class="flex items-center gap-1 rounded-md border border-border/60 px-2 py-1.5">
                  <button
                    type="button"
                    class="min-w-0 flex-1 truncate text-left text-xs hover:text-primary"
                    onclick={() => applySaved(row)}
                  >
                    {row.name}
                  </button>
                  <button
                    type="button"
                    class="text-muted-foreground hover:text-destructive"
                    aria-label={`Delete ${row.name}`}
                    onclick={() => void deleteSaved(row.id)}
                  >
                    <X class="size-3.5" />
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      {/if}
    </div>

    <!-- Filter -->
    <div class="relative">
      <Button
        variant={library.hasFilters ? "default" : "outline"}
        size="sm"
        data-testid="filter-toggle"
        onclick={() => {
          filterOpen = !filterOpen;
          savedOpen = false;
        }}
      >
        <Filter class="size-4" />
        Filter
        {#if library.activeFilterCount > 0}
          <span class="ml-1 rounded-full bg-primary-foreground/20 px-1.5 text-[10px] tabular-nums">
            {library.activeFilterCount}
          </span>
        {/if}
      </Button>
      {#if filterOpen}
        <button
          type="button"
          class="fixed inset-0 z-[90] cursor-default"
          aria-label="Close filter panel"
          onclick={() => (filterOpen = false)}
        ></button>
        <div class="absolute right-0 top-10 z-[100] w-96 rounded-lg border border-border bg-popover p-4 shadow-xl">
          <div class="mb-2 flex items-center justify-between">
            <span class="text-sm font-medium">Filters</span>
            <Button variant="ghost" size="icon-xs" onclick={() => (filterOpen = false)} aria-label="Close">
              <X class="size-3.5" />
            </Button>
          </div>
          <FilterPanel />
        </div>
      {/if}
    </div>

    <!-- Min-favorite filter -->
    <div class="flex items-center gap-1.5">
      <label for="minfav" class="text-xs text-muted-foreground">Min ♥</label>
      <select
        id="minfav"
        value={library.minFavorite}
        onchange={onMinFavChange}
        class="h-8 rounded-md border border-input bg-background px-2 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        {#each [0, 1, 2, 3, 4, 5] as n (n)}
          <option value={n}>{n === 0 ? "Any" : `${n}+`}</option>
        {/each}
      </select>
    </div>

    <!-- Sort -->
    <div class="flex items-center gap-1.5">
      <ArrowDownUp class="size-3.5 text-muted-foreground" />
      <select
        value={library.sort}
        onchange={onSortChange}
        data-testid="sort-select"
        class="h-8 rounded-md border border-input bg-background px-2 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        {#each sortOptions as opt (opt.value)}
          <option value={opt.value}>{opt.label}</option>
        {/each}
      </select>
    </div>

    <Separator orientation="vertical" class="h-6" />
    <div class="flex items-center gap-3 px-2 text-xs text-muted-foreground">
      <span class="tabular-nums">{library.counts.total} scenes</span>
      <span class="flex items-center gap-1 tabular-nums">
        <span class="text-primary">♥</span>
        {library.counts.favorites}
      </span>
    </div>
  </div>
</header>

<!-- Active filter chips row (only in library/favorites views) -->
{#if (library.view === "library" || library.view === "favorites") && library.hasFilters}
  <div class="flex flex-wrap items-center gap-1.5 border-b border-border bg-card/40 px-4 py-1.5">
    {#each library.tagIds as tid (tid)}
      <button
        type="button"
        class="flex items-center gap-1 rounded-full bg-primary/15 px-2 py-0.5 text-xs text-primary hover:bg-primary/25"
        onclick={() => library.toggleTag(tid)}
      >
        +#{tagName(tid)}
        <X class="size-3" />
      </button>
    {/each}
    {#each library.excludeTagIds as tid (tid)}
      <button
        type="button"
        class="flex items-center gap-1 rounded-full bg-destructive/15 px-2 py-0.5 text-xs text-destructive hover:bg-destructive/25"
        onclick={() => library.toggleExcludeTag(tid)}
      >
        −#{tagName(tid)}
        <X class="size-3" />
      </button>
    {/each}
    {#each library.performerIds as pid (pid)}
      <button
        type="button"
        class="flex items-center gap-1 rounded-full bg-primary/15 px-2 py-0.5 text-xs text-primary hover:bg-primary/25"
        onclick={() => library.togglePerformer(pid)}
      >
        +{performerName(pid)}
        <X class="size-3" />
      </button>
    {/each}
    {#each library.excludePerformerIds as pid (pid)}
      <button
        type="button"
        class="flex items-center gap-1 rounded-full bg-destructive/15 px-2 py-0.5 text-xs text-destructive hover:bg-destructive/25"
        onclick={() => library.toggleExcludePerformer(pid)}
      >
        −{performerName(pid)}
        <X class="size-3" />
      </button>
    {/each}
    {#each library.studioIds as sid (sid)}
      <button
        type="button"
        class="flex items-center gap-1 rounded-full bg-primary/15 px-2 py-0.5 text-xs text-primary hover:bg-primary/25"
        onclick={() => library.toggleStudio(sid)}
      >
        +@{studioName(sid)}
        <X class="size-3" />
      </button>
    {/each}
    {#each library.excludeStudioIds as sid (sid)}
      <button
        type="button"
        class="flex items-center gap-1 rounded-full bg-destructive/15 px-2 py-0.5 text-xs text-destructive hover:bg-destructive/25"
        onclick={() => library.toggleExcludeStudio(sid)}
      >
        −@{studioName(sid)}
        <X class="size-3" />
      </button>
    {/each}
    {#if library.searchInverse}
      <button
        type="button"
        class="flex items-center gap-1 rounded-full bg-destructive/15 px-2 py-0.5 text-xs text-destructive hover:bg-destructive/25"
        onclick={() => library.setSearchInverse(false)}
      >
        NOT search
        <X class="size-3" />
      </button>
    {/if}
    {#if library.unplayedOnly}
      <button
        type="button"
        class="flex items-center gap-1 rounded-full bg-primary/15 px-2 py-0.5 text-xs text-primary hover:bg-primary/25"
        onclick={() => library.setUnplayedOnly(false)}
      >
        Unplayed
        <X class="size-3" />
      </button>
    {/if}
    {#if library.minTagCount > 0}
      <button
        type="button"
        class="flex items-center gap-1 rounded-full bg-primary/15 px-2 py-0.5 text-xs text-primary hover:bg-primary/25"
        onclick={() => library.setMinTagCount(0)}
      >
        Tags ≥ {library.minTagCount}
        <X class="size-3" />
      </button>
    {/if}
    {#if library.identifiedOnly}
      <button
        type="button"
        class="flex items-center gap-1 rounded-full bg-primary/15 px-2 py-0.5 text-xs text-primary hover:bg-primary/25"
        onclick={() => library.setIdentifiedOnly(false)}
      >
        Identified
        <X class="size-3" />
      </button>
    {/if}
    {#if library.needsReviewOnly}
      <button
        type="button"
        class="flex items-center gap-1 rounded-full bg-amber-500/15 px-2 py-0.5 text-xs text-amber-800 hover:bg-amber-500/25 dark:text-amber-300"
        onclick={() => library.setNeedsReviewOnly(false)}
      >
        Needs review
        <X class="size-3" />
      </button>
    {/if}
    {#if library.ignoreState !== "any"}
      <button
        type="button"
        class="flex items-center gap-1 rounded-full bg-primary/15 px-2 py-0.5 text-xs text-primary hover:bg-primary/25"
        onclick={() => library.setIgnoreState("any")}
      >
        {library.ignoreState === "ignored" ? "Ignored" : "Not ignored"}
        <X class="size-3" />
      </button>
    {/if}
    {#each library.folderPaths as folder (folder)}
      <button
        type="button"
        title={folder}
        class="flex items-center gap-1 rounded-full bg-primary/15 px-2 py-0.5 text-xs text-primary hover:bg-primary/25"
        onclick={() => library.toggleFolder(folder)}
      >
        +/{folder.split(/[\\/]/).pop() || folder}
        <X class="size-3" />
      </button>
    {/each}
    {#if library.minPerformerCount > 0}
      <button
        type="button"
        class="flex items-center gap-1 rounded-full bg-primary/15 px-2 py-0.5 text-xs text-primary hover:bg-primary/25"
        onclick={() => library.setMinPerformerCount(0)}
      >
        Performers ≥ {library.minPerformerCount}
        <X class="size-3" />
      </button>
    {/if}
    {#if library.minHeight != null}
      <button
        type="button"
        class="flex items-center gap-1 rounded-full bg-primary/15 px-2 py-0.5 text-xs text-primary hover:bg-primary/25"
        onclick={() => library.setMinHeight(null)}
      >
        ≥{library.minHeight}p
        <X class="size-3" />
      </button>
    {/if}
    {#if library.minDurationMins != null || library.maxDurationMins != null}
      <button
        type="button"
        class="flex items-center gap-1 rounded-full bg-primary/15 px-2 py-0.5 text-xs text-primary hover:bg-primary/25"
        onclick={() => library.clearDurationFilters()}
      >
        {library.minDurationMins ?? "0"}–{library.maxDurationMins ?? "∞"} min
        <X class="size-3" />
      </button>
    {/if}
    {#if library.minFavorite > 0}
      <button
        type="button"
        class="flex items-center gap-1 rounded-full bg-primary/15 px-2 py-0.5 text-xs text-primary hover:bg-primary/25"
        onclick={() => { library.minFavorite = 0; void library.refresh(); }}
      >
        ♥ {library.minFavorite}+
        <X class="size-3" />
      </button>
    {/if}
    <button
      type="button"
      class="ml-1 text-xs text-muted-foreground hover:text-destructive"
      onclick={() => library.clearFilters()}
    >
      Clear all
    </button>
  </div>
{/if}
