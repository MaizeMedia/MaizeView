<script lang="ts">
  import { onMount, type Snippet } from "svelte";
  import SceneCard from "./scene-card.svelte";
  import type { SceneGridRow } from "$lib/api/types";

  let {
    scenes,
    minColWidth = 220,
    gap = 16,
    metaHeight = 76,
    overscanRows = 2,
    /** Optional custom cell; defaults to SceneCard. */
    item,
  }: {
    scenes: SceneGridRow[];
    minColWidth?: number;
    gap?: number;
    metaHeight?: number;
    overscanRows?: number;
    item?: Snippet<[SceneGridRow]>;
  } = $props();

  let viewport: HTMLDivElement | undefined = $state();
  let width = $state(0);
  let scrollTop = $state(0);
  let viewportHeight = $state(0);
  let measuredRowStride = $state<number | null>(null);

  const columns = $derived.by(() => {
    if (width <= 0) return 1;
    return Math.max(1, Math.floor((width + gap) / (minColWidth + gap)));
  });

  const columnWidth = $derived.by(() => {
    if (width <= 0 || columns <= 0) return minColWidth;
    return (width - (columns - 1) * gap) / columns;
  });

  const estimatedRowStride = $derived.by(() => {
    const thumbH = columnWidth * (9 / 16);
    return thumbH + metaHeight + gap;
  });

  const rowStride = $derived(measuredRowStride ?? estimatedRowStride);

  const rows = $derived.by(() => {
    const out: SceneGridRow[][] = [];
    for (let i = 0; i < scenes.length; i += columns) {
      out.push(scenes.slice(i, i + columns));
    }
    return out;
  });

  const totalHeight = $derived(Math.max(0, rows.length * rowStride - gap));

  const visibleRange = $derived.by(() => {
    if (rows.length === 0 || rowStride <= 0) return { start: 0, end: 0 };
    const start = Math.max(0, Math.floor(scrollTop / rowStride) - overscanRows);
    const end = Math.min(
      rows.length,
      Math.ceil((scrollTop + viewportHeight) / rowStride) + overscanRows,
    );
    return { start, end };
  });

  const visibleRows = $derived(rows.slice(visibleRange.start, visibleRange.end));

  $effect(() => {
    void columns;
    void columnWidth;
    measuredRowStride = null;
  });

  function onScroll() {
    if (!viewport) return;
    scrollTop = viewport.scrollTop;
  }

  function measureRow(node: HTMLDivElement, enabled: boolean) {
    if (!enabled) return {};
    const ro = new ResizeObserver((entries) => {
      const h = entries[0]?.contentRect.height;
      if (h && h > 0) measuredRowStride = h + gap;
    });
    ro.observe(node);
    return { destroy: () => ro.disconnect() };
  }

  onMount(() => {
    if (!viewport) return;
    const ro = new ResizeObserver((entries) => {
      const rect = entries[0]?.contentRect;
      if (!rect) return;
      width = rect.width;
      viewportHeight = rect.height;
    });
    ro.observe(viewport);
    width = viewport.clientWidth;
    viewportHeight = viewport.clientHeight;
    return () => ro.disconnect();
  });
</script>

<div bind:this={viewport} class="h-full overflow-y-auto overflow-x-hidden" onscroll={onScroll}>
  <div class="relative w-full" style:height="{totalHeight}px">
    {#each visibleRows as row, i (visibleRange.start + i)}
      {@const rowIndex = visibleRange.start + i}
      <div
        class="absolute left-0 right-0 grid gap-4"
        style:top="{rowIndex * rowStride}px"
        style:grid-template-columns="repeat({columns}, minmax(0, 1fr))"
        use:measureRow={i === 0}
      >
        {#each row as scene (scene.id)}
          {#if item}
            {@render item(scene)}
          {:else}
            <SceneCard {scene} />
          {/if}
        {/each}
      </div>
    {/each}
  </div>
</div>
