<script lang="ts">
  import { Film, Check, Play, Heart } from "@lucide/svelte";
  import type { SceneGridRow } from "$lib/api/types";
  import { assetUrl, openPlayerWindow } from "$lib/api";
  import { library } from "$lib/stores/library.svelte";
  import FavoriteButton from "$components/favorite-button.svelte";

  let {
    scene,
    selectable = false,
    selected = false,
    onToggleSelect,
  }: {
    scene: SceneGridRow;
    selectable?: boolean;
    selected?: boolean;
    onToggleSelect?: () => void;
  } = $props();

  function fmtDuration(s: number | null): string {
    if (s == null) return "";
    const m = Math.floor(s / 60);
    const sec = Math.floor(s % 60);
    if (m < 60) return `${m}:${sec.toString().padStart(2, "0")}`;
    const h = Math.floor(m / 60);
    return `${h}:${(m % 60).toString().padStart(2, "0")}:${sec.toString().padStart(2, "0")}`;
  }

  let isSelected = $derived(
    selectable ? selected : library.selectionMode && library.selectedIds.has(scene.id),
  );

  function activate() {
    if (selectable && onToggleSelect) {
      onToggleSelect();
      return;
    }
    if (library.selectionMode) {
      library.toggleSelected(scene.id);
    } else {
      library.openDetail(scene.id);
    }
  }

  async function playScene(e: MouseEvent) {
    e.stopPropagation();
    if (!scene.file_path) return;
    try {
      await openPlayerWindow({
        sceneId: scene.id,
        filePath: scene.file_path,
        title: `MaizeView — ${scene.title ?? scene.file_path.split(/[\\/]/).pop() ?? "Player"}`,
      });
    } catch (err) {
      console.error("play failed", err);
    }
  }
</script>

<div
  role="button"
  tabindex="0"
  aria-pressed={isSelected}
  class="group relative flex cursor-pointer flex-col overflow-hidden rounded-lg border bg-card text-left transition-colors hover:border-primary/50 hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring
    {isSelected ? 'border-primary ring-2 ring-primary/40' : 'border-border'}"
  onclick={activate}
  onkeydown={(e) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      activate();
    }
  }}
>
  <!-- Selection check (library grid or playlist multi-open) -->
  {#if selectable || library.selectionMode}
    <div
      class="absolute left-2 top-2 z-20 flex size-6 items-center justify-center rounded-full border-2 transition-colors
        {isSelected ? 'border-primary bg-primary text-primary-foreground' : 'border-white/70 bg-black/40 text-transparent'}"
    >
      <Check class="size-3.5" />
    </div>
  {/if}

  <!-- Thumb area: fixed 16:9 box; portrait sources scale to fit (no flex blowout). -->
  <div class="relative aspect-video w-full shrink-0 overflow-hidden bg-secondary/60">
    {#if scene.thumb_path}
      <img src={assetUrl(scene.thumb_path) ?? ""} alt="" class="absolute inset-0 size-full object-contain" loading="lazy" decoding="async" />
    {:else}
      <div class="absolute inset-0 flex items-center justify-center text-muted-foreground/40">
        <Film class="size-10" />
      </div>
    {/if}

    {#if scene.duration}
      <span class="absolute bottom-1.5 right-1.5 rounded bg-black/70 px-1.5 py-0.5 text-[11px] font-medium tabular-nums text-white">
        {fmtDuration(scene.duration)}
      </span>
    {/if}

    {#if scene.file_path && !library.selectionMode && !selectable}
      <!-- pointer-events-none until hover so card click opens the detail drawer -->
      <button
        type="button"
        onclick={playScene}
        aria-label="Play"
        class="absolute bottom-2 left-2 z-10 flex size-9 items-center justify-center rounded-full bg-primary/90 text-primary-foreground opacity-0 shadow-lg transition-opacity pointer-events-none group-hover:opacity-100 group-hover:pointer-events-auto"
      >
        <Play class="size-4 translate-x-0.5" fill="currentColor" />
      </button>
    {/if}

    {#if scene.width && scene.height}
      <span class="absolute bottom-1.5 left-1.5 rounded bg-black/70 px-1.5 py-0.5 text-[10px] font-medium text-white/80">
        {scene.height}p
      </span>
    {/if}

    <!-- Favorite: single heart + level counter -->
    <div
      class="absolute right-1.5 top-1.5 rounded-full bg-black/50 px-1.5 py-1 opacity-0 transition-opacity focus-within:opacity-100 group-hover:opacity-100"
    >
      <FavoriteButton
        level={scene.favorite}
        size="sm"
        class="text-white"
        onChange={async (next) => {
          await library.setFavoriteLevel(scene, next);
        }}
      />
    </div>
  </div>

  <!-- Meta -->
  <div class="flex flex-col gap-1 p-3">
    <div class="line-clamp-2 text-sm font-medium leading-snug">
      {scene.title ?? scene.file_path?.split(/[\\/]/).pop() ?? "Untitled"}
    </div>
    <div class="flex items-center gap-2 text-xs text-muted-foreground">
      {#if scene.favorite > 0}
        <span class="inline-flex items-center gap-0.5 text-primary" title={`Favorite level ${scene.favorite}`}>
          <Heart class="size-3" fill="currentColor" stroke="currentColor" stroke-width="2" />
          <span class="tabular-nums">{scene.favorite}</span>
        </span>
        <span aria-hidden="true">·</span>
      {/if}
      <span>{scene.play_count} plays</span>
    </div>
  </div>
</div>
