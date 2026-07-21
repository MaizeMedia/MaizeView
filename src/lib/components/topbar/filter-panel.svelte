<script lang="ts">
  import { onMount } from "svelte";
  import { X, Check, Loader2, Ban } from "@lucide/svelte";
  import { tags as tagsApi, performers as performersApi, studios as studiosApi, type IgnoreState } from "$lib/api";
  import type { PerformerRow, TagRow, StudioRow } from "$lib/api/types";
  import { library } from "$lib/stores/library.svelte";

  let allTags = $state<TagRow[]>([]);
  let allPerformers = $state<PerformerRow[]>([]);
  let allStudios = $state<StudioRow[]>([]);
  let loading = $state(true);
  let tagQuery = $state("");
  let excludeTagQuery = $state("");
  let perfQuery = $state("");
  let excludePerfQuery = $state("");
  let studioQuery = $state("");
  let excludeStudioQuery = $state("");

  async function load() {
    loading = true;
    try {
      const [t, p, s] = await Promise.all([
        tagsApi.list(),
        performersApi.list(),
        studiosApi.list(),
      ]);
      allTags = t;
      allPerformers = p;
      allStudios = s;
    } finally {
      loading = false;
    }
  }

  function matchesQuery(name: string, query: string): boolean {
    return name.toLowerCase().includes(query.trim().toLowerCase());
  }

  let filteredTags = $derived(allTags.filter((t) => matchesQuery(t.name, tagQuery)));
  let filteredExcludeTags = $derived(allTags.filter((t) => matchesQuery(t.name, excludeTagQuery)));
  let filteredPerformers = $derived(allPerformers.filter((p) => matchesQuery(p.name, perfQuery)));
  let filteredExcludePerformers = $derived(
    allPerformers.filter((p) => matchesQuery(p.name, excludePerfQuery)),
  );
  let filteredStudios = $derived(allStudios.filter((s) => matchesQuery(s.name, studioQuery)));
  let filteredExcludeStudios = $derived(
    allStudios.filter((s) => matchesQuery(s.name, excludeStudioQuery)),
  );

  const TAG_COUNT_PRESETS = [0, 1, 3, 5, 10] as const;
  const HEIGHT_PRESETS: { value: number | null; label: string }[] = [
    { value: null, label: "Any" },
    { value: 720, label: "720p+" },
    { value: 1080, label: "1080p+" },
    { value: 1440, label: "1440p+" },
    { value: 2160, label: "4K+" },
  ];

  onMount(load);
</script>

<div class="max-h-[70vh] space-y-4 overflow-y-auto pr-1">
  {#if loading}
    <div class="flex justify-center py-6 text-muted-foreground">
      <Loader2 class="size-5 animate-spin" />
    </div>
  {:else}
    <!-- Text search options -->
    <div class="space-y-2 rounded-md border border-border bg-card/40 p-2.5">
      <div class="text-xs font-medium text-muted-foreground">Text search</div>
      <label class="flex cursor-pointer items-start gap-2 text-xs">
        <input
          type="checkbox"
          checked={library.searchInverse}
          onchange={(e) => library.setSearchInverse((e.currentTarget as HTMLInputElement).checked)}
          class="mt-0.5 accent-primary"
        />
        <span>
          <span class="font-medium">Invert search</span>
          <span class="block text-muted-foreground">Show scenes that do <em>not</em> match any search word (title, details, path, tags, performers).</span>
        </span>
      </label>
      <p class="text-[11px] leading-snug text-muted-foreground">
        Tip: prefix with <code class="rounded bg-muted px-1">-</code> to exclude inline —
        e.g. <code class="rounded bg-muted px-1">1080p -wmv</code>
      </p>
    </div>

    <!-- Curation gates -->
    <div class="space-y-2 rounded-md border border-border bg-card/40 p-2.5" data-testid="curation-gates">
      <div class="text-xs font-medium text-muted-foreground">Curation gates</div>
      <div>
        <div class="mb-1 text-[11px] text-muted-foreground">Min tags</div>
        <select
          data-testid="min-tag-count"
          value={library.minTagCount}
          onchange={(e) => library.setMinTagCount(Number((e.currentTarget as HTMLSelectElement).value))}
          class="h-7 w-full rounded-md border border-input bg-background px-2 text-xs"
        >
          {#each TAG_COUNT_PRESETS as n (n)}
            <option value={n}>{n === 0 ? "Off" : `≥ ${n}`}</option>
          {/each}
        </select>
        <p class="mt-1 text-[11px] leading-snug text-muted-foreground">
          Use with exclude tags so sparsely tagged files don’t fill the list.
        </p>
      </div>
      <label class="flex cursor-pointer items-start gap-2 text-xs" data-testid="identified-only-filter">
        <input
          type="checkbox"
          checked={library.identifiedOnly}
          onchange={(e) => library.setIdentifiedOnly((e.currentTarget as HTMLInputElement).checked)}
          class="mt-0.5 accent-primary"
        />
        <span>
          <span class="font-medium">Identified only</span>
          <span class="block text-muted-foreground">Scenes with a successful stash-box identify apply.</span>
        </span>
      </label>
      <label class="flex cursor-pointer items-start gap-2 text-xs" data-testid="needs-review-filter">
        <input
          type="checkbox"
          checked={library.needsReviewOnly}
          onchange={(e) => library.setNeedsReviewOnly((e.currentTarget as HTMLInputElement).checked)}
          class="mt-0.5 accent-primary"
        />
        <span>
          <span class="font-medium">Needs review</span>
          <span class="block text-muted-foreground">Multiple stash-box matches — open a scene and pick the right one.</span>
        </span>
      </label>
      <div>
        <div class="mb-1 text-[11px] text-muted-foreground">Ignore state</div>
        <select
          data-testid="ignore-state"
          value={library.ignoreState}
          onchange={(e) => library.setIgnoreState((e.currentTarget as HTMLSelectElement).value as IgnoreState)}
          class="h-7 w-full rounded-md border border-input bg-background px-2 text-xs"
        >
          <option value="any">Any</option>
          <option value="ignored">Ignored only</option>
          <option value="not_ignored">Not ignored</option>
        </select>
        <p class="mt-1 text-[11px] leading-snug text-muted-foreground">
          Ignored scenes are skipped by identify (drawer Unlink / selection Ignore identify).
        </p>
      </div>
      <div>
        <div class="mb-1 text-[11px] text-muted-foreground">Min performers</div>
        <select
          data-testid="min-performer-count"
          value={library.minPerformerCount}
          onchange={(e) => library.setMinPerformerCount(Number((e.currentTarget as HTMLSelectElement).value))}
          class="h-7 w-full rounded-md border border-input bg-background px-2 text-xs"
        >
          {#each [0, 1, 2, 3] as n (n)}
            <option value={n}>{n === 0 ? "Off" : `≥ ${n}`}</option>
          {/each}
        </select>
      </div>
      <div>
        <div class="mb-1 text-[11px] text-muted-foreground">Min resolution</div>
        <select
          data-testid="min-height"
          value={library.minHeight ?? ""}
          onchange={(e) => {
            const v = (e.currentTarget as HTMLSelectElement).value;
            library.setMinHeight(v === "" ? null : Number(v));
          }}
          class="h-7 w-full rounded-md border border-input bg-background px-2 text-xs"
        >
          {#each HEIGHT_PRESETS as opt (opt.label)}
            <option value={opt.value ?? ""}>{opt.label}</option>
          {/each}
        </select>
      </div>
    </div>

    <!-- Include tags -->
    <div>
      <div class="mb-1.5 flex items-center justify-between gap-2">
        <div class="text-xs font-medium text-muted-foreground">
          Include tags <span class="text-muted-foreground/60">({allTags.length})</span>
        </div>
        {#if library.tagIds.length > 1}
          <label class="flex items-center gap-1.5 text-[11px] text-muted-foreground">
            <select
              value={library.tagMatchAny ? "any" : "all"}
              onchange={(e) => library.setTagMatchAny((e.currentTarget as HTMLSelectElement).value === "any")}
              class="h-6 rounded border border-input bg-background px-1.5 text-[11px]"
            >
              <option value="all">Match all</option>
              <option value="any">Match any</option>
            </select>
          </label>
        {/if}
      </div>
      <input
        bind:value={tagQuery}
        placeholder="Filter tags…"
        class="mb-2 h-7 w-full rounded-md border border-input bg-background px-2 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring"
      />
      {#if filteredTags.length === 0}
        <p class="text-xs text-muted-foreground">No tags yet. Add tags from a scene's detail.</p>
      {:else}
        <div class="flex max-h-32 flex-wrap gap-1.5 overflow-y-auto">
          {#each filteredTags as t (t.id)}
            <button
              type="button"
              onclick={() => library.toggleTag(t.id)}
              class="flex items-center gap-1 rounded-full border px-2 py-0.5 text-xs transition-colors
                {library.tagIds.includes(t.id)
                ? 'border-primary bg-primary/15 text-primary'
                : 'border-border bg-card hover:bg-accent'}"
            >
              {#if library.tagIds.includes(t.id)}
                <Check class="size-3" />
              {/if}
              {t.name}
            </button>
          {/each}
        </div>
      {/if}
    </div>

    <!-- Exclude tags -->
    <div>
      <div class="mb-1.5 text-xs font-medium text-destructive/90">
        Exclude tags
      </div>
      <input
        bind:value={excludeTagQuery}
        placeholder="Filter tags to exclude…"
        class="mb-2 h-7 w-full rounded-md border border-input bg-background px-2 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring"
      />
      {#if filteredExcludeTags.length === 0}
        <p class="text-xs text-muted-foreground">No tags available.</p>
      {:else}
        <div class="flex max-h-32 flex-wrap gap-1.5 overflow-y-auto">
          {#each filteredExcludeTags as t (t.id)}
            <button
              type="button"
              onclick={() => library.toggleExcludeTag(t.id)}
              class="flex items-center gap-1 rounded-full border px-2 py-0.5 text-xs transition-colors
                {library.excludeTagIds.includes(t.id)
                ? 'border-destructive/60 bg-destructive/15 text-destructive'
                : 'border-border bg-card hover:bg-accent'}"
            >
              {#if library.excludeTagIds.includes(t.id)}
                <Ban class="size-3" />
              {/if}
              {t.name}
            </button>
          {/each}
        </div>
      {/if}
    </div>

    <!-- Include performers -->
    <div>
      <div class="mb-1.5 text-xs font-medium text-muted-foreground">
        Include performers <span class="text-muted-foreground/60">({allPerformers.length})</span>
      </div>
      <input
        bind:value={perfQuery}
        placeholder="Filter performers…"
        class="mb-2 h-7 w-full rounded-md border border-input bg-background px-2 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring"
      />
      {#if filteredPerformers.length === 0}
        <p class="text-xs text-muted-foreground">No performers yet.</p>
      {:else}
        <div class="flex max-h-32 flex-wrap gap-1.5 overflow-y-auto">
          {#each filteredPerformers as p (p.id)}
            <button
              type="button"
              onclick={() => library.togglePerformer(p.id)}
              class="flex items-center gap-1 rounded-full border px-2 py-0.5 text-xs transition-colors
                {library.performerIds.includes(p.id)
                ? 'border-primary bg-primary/15 text-primary'
                : 'border-border bg-card hover:bg-accent'}"
            >
              {#if library.performerIds.includes(p.id)}
                <Check class="size-3" />
              {/if}
              {p.name}
            </button>
          {/each}
        </div>
      {/if}
    </div>

    <!-- Exclude performers -->
    <div>
      <div class="mb-1.5 text-xs font-medium text-destructive/90">
        Exclude performers
      </div>
      <input
        bind:value={excludePerfQuery}
        placeholder="Filter performers to exclude…"
        class="mb-2 h-7 w-full rounded-md border border-input bg-background px-2 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring"
      />
      {#if filteredExcludePerformers.length === 0}
        <p class="text-xs text-muted-foreground">No performers available.</p>
      {:else}
        <div class="flex max-h-32 flex-wrap gap-1.5 overflow-y-auto">
          {#each filteredExcludePerformers as p (p.id)}
            <button
              type="button"
              onclick={() => library.toggleExcludePerformer(p.id)}
              class="flex items-center gap-1 rounded-full border px-2 py-0.5 text-xs transition-colors
                {library.excludePerformerIds.includes(p.id)
                ? 'border-destructive/60 bg-destructive/15 text-destructive'
                : 'border-border bg-card hover:bg-accent'}"
            >
              {#if library.excludePerformerIds.includes(p.id)}
                <Ban class="size-3" />
              {/if}
              {p.name}
            </button>
          {/each}
        </div>
      {/if}
    </div>

    <!-- Include studios -->
    <div>
      <div class="mb-1.5 text-xs font-medium text-muted-foreground">
        Include studios <span class="text-muted-foreground/60">({allStudios.length})</span>
      </div>
      <input
        bind:value={studioQuery}
        placeholder="Filter studios…"
        class="mb-2 h-7 w-full rounded-md border border-input bg-background px-2 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring"
      />
      {#if filteredStudios.length === 0}
        <p class="text-xs text-muted-foreground">No studios yet.</p>
      {:else}
        <div class="flex max-h-28 flex-wrap gap-1.5 overflow-y-auto">
          {#each filteredStudios as s (s.id)}
            <button
              type="button"
              onclick={() => library.toggleStudio(s.id)}
              class="flex items-center gap-1 rounded-full border px-2 py-0.5 text-xs transition-colors
                {library.studioIds.includes(s.id)
                ? 'border-primary bg-primary/15 text-primary'
                : 'border-border bg-card hover:bg-accent'}"
            >
              {#if library.studioIds.includes(s.id)}
                <Check class="size-3" />
              {/if}
              {s.name}
            </button>
          {/each}
        </div>
      {/if}
    </div>

    <!-- Exclude studios -->
    <div>
      <div class="mb-1.5 text-xs font-medium text-destructive/90">
        Exclude studios
      </div>
      <input
        bind:value={excludeStudioQuery}
        placeholder="Filter studios to exclude…"
        class="mb-2 h-7 w-full rounded-md border border-input bg-background px-2 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring"
      />
      {#if filteredExcludeStudios.length === 0}
        <p class="text-xs text-muted-foreground">No studios available.</p>
      {:else}
        <div class="flex max-h-28 flex-wrap gap-1.5 overflow-y-auto">
          {#each filteredExcludeStudios as s (s.id)}
            <button
              type="button"
              onclick={() => library.toggleExcludeStudio(s.id)}
              class="flex items-center gap-1 rounded-full border px-2 py-0.5 text-xs transition-colors
                {library.excludeStudioIds.includes(s.id)
                ? 'border-destructive/60 bg-destructive/15 text-destructive'
                : 'border-border bg-card hover:bg-accent'}"
            >
              {#if library.excludeStudioIds.includes(s.id)}
                <Ban class="size-3" />
              {/if}
              {s.name}
            </button>
          {/each}
        </div>
      {/if}
    </div>

    <!-- Duration + playback -->
    <div class="space-y-2 rounded-md border border-border bg-card/40 p-2.5">
      <div class="text-xs font-medium text-muted-foreground">Duration (minutes)</div>
      <div class="flex items-center gap-2">
        <input
          type="number"
          min="0"
          placeholder="Min"
          value={library.minDurationMins ?? ""}
          onchange={(e) => {
            const v = (e.currentTarget as HTMLInputElement).value;
            library.setMinDurationMins(v === "" ? null : Math.max(0, Number(v)));
          }}
          class="h-7 w-full rounded-md border border-input bg-background px-2 text-xs"
        />
        <span class="text-xs text-muted-foreground">–</span>
        <input
          type="number"
          min="0"
          placeholder="Max"
          value={library.maxDurationMins ?? ""}
          onchange={(e) => {
            const v = (e.currentTarget as HTMLInputElement).value;
            library.setMaxDurationMins(v === "" ? null : Math.max(0, Number(v)));
          }}
          class="h-7 w-full rounded-md border border-input bg-background px-2 text-xs"
        />
      </div>
      <label class="flex cursor-pointer items-center gap-2 text-xs">
        <input
          type="checkbox"
          checked={library.unplayedOnly}
          onchange={(e) => library.setUnplayedOnly((e.currentTarget as HTMLInputElement).checked)}
          class="accent-primary"
        />
        Unplayed only
      </label>
    </div>

    {#if library.hasFilters}
      <button
        type="button"
        class="flex items-center gap-1 text-xs text-muted-foreground hover:text-destructive"
        onclick={() => library.clearFilters()}
      >
        <X class="size-3" />
        Clear all filters
      </button>
    {/if}
  {/if}
</div>
