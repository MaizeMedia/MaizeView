<script lang="ts">
  import { onMount } from "svelte";
  import { Loader2, Copy, RefreshCw, Trash2 } from "@lucide/svelte";
  import { confirm } from "@tauri-apps/plugin-dialog";
  import Button from "$components/ui/button/button.svelte";
  import {
    duplicates as duplicatesApi,
    assetUrl,
    type DuplicateGroup,
    type DuplicateSceneEntry,
  } from "$lib/api";
  import { library } from "$lib/stores/library.svelte";
  import { stringifyError } from "$lib/utils";

  let loading = $state(true);
  let error = $state<string | null>(null);
  let groups = $state<DuplicateGroup[]>([]);
  let threshold = $state(8);
  let keepers = $state<Record<string, string>>({});
  let resolvingGroupKey = $state<string | null>(null);

  function groupKey(group: DuplicateGroup): string {
    return group.scenes
      .map((s) => s.scene_id)
      .sort()
      .join("|");
  }

  function defaultKeeper(scenes: DuplicateSceneEntry[]): string {
    return scenes.reduce((best, scene) =>
      scene.favorite > best.favorite ? scene : best,
    ).scene_id;
  }

  function keeperForGroup(group: DuplicateGroup): string {
    const key = groupKey(group);
    return keepers[key] ?? defaultKeeper(group.scenes);
  }

  function setKeeper(group: DuplicateGroup, sceneId: string) {
    keepers = { ...keepers, [groupKey(group)]: sceneId };
  }

  function fmtDuration(s: number | null): string {
    if (s == null) return "";
    const m = Math.floor(s / 60);
    const sec = Math.floor(s % 60);
    if (m < 60) return `${m}:${sec.toString().padStart(2, "0")}`;
    const h = Math.floor(m / 60);
    return `${h}:${(m % 60).toString().padStart(2, "0")}:${sec.toString().padStart(2, "0")}`;
  }

  function fmtSize(b: number): string {
    const units = ["B", "KB", "MB", "GB", "TB"];
    let v = b;
    let u = 0;
    while (v >= 1024 && u < units.length - 1) {
      v /= 1024;
      u += 1;
    }
    return `${v.toFixed(u === 0 ? 0 : 1)} ${units[u]}`;
  }

  function fmtBitrate(bps: number | null): string {
    if (bps == null) return "";
    return `${(bps / 1_000_000).toFixed(1)} Mb/s`;
  }

  /** One-line tech summary for keeper decisions: h264 1920x1080 · 1:23:45 · 59.9 fps · 8.2 Mb/s · 1.4 GB */
  function specLine(s: DuplicateSceneEntry): string {
    const parts: string[] = [];
    if (s.codec) parts.push(s.codec);
    if (s.width != null && s.height != null) parts.push(`${s.width}x${s.height}`);
    const dur = fmtDuration(s.duration);
    if (dur) parts.push(dur);
    if (s.fps != null) parts.push(`${s.fps.toFixed(2).replace(/\.?0+$/, "")} fps`);
    const br = fmtBitrate(s.bitrate);
    if (br) parts.push(br);
    if (s.size_bytes > 0) parts.push(fmtSize(s.size_bytes));
    return parts.join(" · ");
  }

  async function load() {
    loading = true;
    error = null;
    try {
      groups = await duplicatesApi.findGroups(threshold);
      const next: Record<string, string> = { ...keepers };
      for (const group of groups) {
        const key = groupKey(group);
        const ids = new Set(group.scenes.map((s) => s.scene_id));
        if (!next[key] || !ids.has(next[key])) {
          next[key] = defaultKeeper(group.scenes);
        }
      }
      keepers = next;
    } catch (e) {
      error = stringifyError(e);
    } finally {
      loading = false;
    }
  }

  async function deleteDuplicates(group: DuplicateGroup) {
    const key = groupKey(group);
    const keeperId = keeperForGroup(group);
    const deleteIds = group.scenes
      .map((s) => s.scene_id)
      .filter((id) => id !== keeperId);

    if (deleteIds.length === 0) return;

    const keeper = group.scenes.find((s) => s.scene_id === keeperId);
    const keeperLabel =
      keeper?.title ?? keeper?.file_path?.split(/[\\/]/).pop() ?? "selected scene";

    const ok = await confirm(
      `Delete ${deleteIds.length} duplicate file${deleteIds.length === 1 ? "" : "s"} and keep “${keeperLabel}”? Highest favorite level is merged onto the keeper.`,
      { title: "Delete duplicates", kind: "warning" },
    );
    if (!ok) return;

    resolvingGroupKey = key;
    error = null;
    try {
      await duplicatesApi.resolveGroup(keeperId, deleteIds);
      await library.refresh();
      await load();
    } catch (e) {
      error = stringifyError(e);
    } finally {
      resolvingGroupKey = null;
    }
  }

  onMount(() => {
    void load();
  });
</script>

<section class="mx-auto w-full max-w-5xl space-y-6">
  <header class="flex flex-wrap items-end justify-between gap-4">
    <div class="space-y-1">
      <h1 class="text-2xl font-semibold tracking-tight">Duplicates</h1>
      <p class="text-sm text-muted-foreground">
        Groups scenes with similar pHash fingerprints (Hamming distance ≤ threshold). Pick a keeper, then delete the rest.
      </p>
    </div>
    <div class="flex items-center gap-3">
      <label class="flex items-center gap-2 text-xs text-muted-foreground">
        Threshold
        <input
          type="number"
          min="0"
          max="32"
          bind:value={threshold}
          class="h-8 w-16 rounded-md border border-input bg-background px-2 text-sm"
        />
      </label>
      <Button variant="outline" onclick={load} disabled={loading}>
        {#if loading}
          <Loader2 class="size-4 animate-spin" />
        {:else}
          <RefreshCw class="size-4" />
        {/if}
        Refresh
      </Button>
    </div>
  </header>

  {#if error}
    <div class="rounded-md border border-destructive/40 bg-destructive/10 px-4 py-3 text-sm text-destructive">
      {error}
    </div>
  {/if}

  {#if loading && groups.length === 0}
    <div class="flex items-center justify-center py-16 text-muted-foreground">
      <Loader2 class="size-6 animate-spin" />
    </div>
  {:else if groups.length === 0}
    <div class="rounded-md border border-dashed border-border px-6 py-12 text-center text-sm text-muted-foreground">
      <Copy class="mx-auto mb-2 size-8 opacity-40" />
      No duplicate groups found. Run a scan and wait for pHash fingerprints to finish computing.
    </div>
  {:else}
    <p class="text-xs text-muted-foreground">{groups.length} duplicate group{groups.length === 1 ? "" : "s"}</p>
    <ul class="space-y-4">
      {#each groups as group (groupKey(group))}
        {@const key = groupKey(group)}
        {@const keeperId = keeperForGroup(group)}
        <li class="rounded-lg border border-border bg-card/50 p-4">
          <div class="mb-3 flex flex-wrap items-center justify-between gap-2">
            <div class="text-xs font-medium text-muted-foreground">
              {group.scenes.length} scenes · max distance {group.max_distance}
            </div>
            <Button
              variant="destructive"
              size="sm"
              disabled={resolvingGroupKey === key || group.scenes.length < 2}
              onclick={() => deleteDuplicates(group)}
            >
              {#if resolvingGroupKey === key}
                <Loader2 class="size-4 animate-spin" />
              {:else}
                <Trash2 class="size-4" />
              {/if}
              Delete duplicates
            </Button>
          </div>
          <div class="grid grid-cols-[repeat(auto-fill,minmax(200px,1fr))] gap-3">
            {#each group.scenes as scene (scene.scene_id)}
              {@const isKeeper = scene.scene_id === keeperId}
              {@const spec = specLine(scene)}
              <div
                class="overflow-hidden rounded-md border bg-background text-left transition-colors {isKeeper
                  ? 'border-primary ring-1 ring-primary/40'
                  : 'border-border'}"
              >
                <label class="block cursor-pointer">
                  <div class="relative aspect-video shrink-0 overflow-hidden bg-secondary/40">
                    {#if scene.thumb_path}
                      <img
                        src={assetUrl(scene.thumb_path) ?? ""}
                        alt=""
                        class="absolute inset-0 size-full object-contain"
                      />
                    {/if}
                    <div class="absolute left-2 top-2 flex items-center gap-1.5">
                      <input
                        type="radio"
                        name="keeper-{key}"
                        checked={isKeeper}
                        onchange={() => setKeeper(group, scene.scene_id)}
                        class="size-4 accent-primary"
                      />
                      {#if isKeeper}
                        <span class="rounded bg-primary px-1.5 py-0.5 text-[10px] font-medium text-primary-foreground">
                          Keeper
                        </span>
                      {/if}
                    </div>
                    {#if scene.favorite > 0}
                      <span class="absolute right-2 top-2 rounded bg-black/60 px-1.5 py-0.5 text-[10px] text-white">
                        ♥ {scene.favorite}
                      </span>
                    {/if}
                    {#if scene.identified}
                      <span
                        class="absolute right-2 rounded bg-emerald-600/80 px-1.5 py-0.5 text-[10px] font-medium text-white {scene.favorite > 0
                          ? 'top-9'
                          : 'top-2'}"
                        title="Identified via stash-box"
                      >
                        Identified
                      </span>
                    {/if}
                  </div>
                  <div class="space-y-0.5 p-2">
                    <div class="truncate text-sm font-medium">
                      {scene.title ?? scene.file_path?.split(/[\\/]/).pop() ?? "Untitled"}
                    </div>
                    {#if spec}
                      <div class="truncate text-[11px] text-foreground/80" title={spec}>{spec}</div>
                    {/if}
                    {#if scene.file_path}
                      <div class="truncate font-mono text-[10px] text-muted-foreground" title={scene.file_path}>
                        {scene.file_path}
                      </div>
                    {/if}
                    <div class="truncate font-mono text-[10px] text-muted-foreground/60">{scene.phash}</div>
                  </div>
                </label>
                <div class="border-t border-border px-2 pb-2">
                  <button
                    type="button"
                    class="text-[11px] text-muted-foreground underline-offset-2 hover:text-foreground hover:underline"
                    onclick={() => library.openDetail(scene.scene_id)}
                  >
                    Open details
                  </button>
                </div>
              </div>
            {/each}
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</section>
