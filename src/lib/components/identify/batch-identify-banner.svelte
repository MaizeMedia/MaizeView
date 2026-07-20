<script lang="ts">
  import { Sparkles, Loader2 } from "@lucide/svelte";
  import { library } from "$lib/stores/library.svelte";

  let p = $derived(library.batchIdentifyProgress);
</script>

{#if library.batchIdentifying && p}
  <div class="border-b border-border bg-card/80 px-4 py-2 text-xs">
    <div class="flex items-center gap-2 font-medium text-foreground">
      {#if p.finished}
        StashDB batch identify complete
      {:else}
        <Loader2 class="size-3.5 animate-spin" />
        StashDB batch identify…
      {/if}
    </div>
    <div class="mt-1 flex flex-wrap gap-x-4 gap-y-0.5 text-muted-foreground">
      <span>{p.done}/{p.total} scenes</span>
      {#if p.skipped > 0}
        <span>{p.skipped} skipped</span>
      {/if}
      <span>{p.matched} matched</span>
      <span>{p.applied} auto-applied</span>
      {#if p.needs_review > 0}
        <span class="text-amber-700 dark:text-amber-400">{p.needs_review} need review</span>
      {/if}
      {#if p.errors > 0}
        <span class="text-destructive">{p.errors} errors</span>
      {/if}
    </div>
    {#if p.last_error}
      <div class="mt-1 truncate text-destructive">{p.last_error}</div>
    {/if}
  </div>
{:else if p?.finished && (p.total > 0 || p.skipped > 0)}
  <div class="border-b border-border bg-card/60 px-4 py-2 text-xs text-muted-foreground">
    <Sparkles class="mr-1 inline size-3.5" />
    {#if p.total === 0}
      Nothing to identify — {p.skipped} skipped (checked recently).
    {:else}
      Batch identify: {p.applied}/{p.total} applied, {p.matched} matched
      {#if p.needs_review > 0}
        ·
        <button
          type="button"
          class="font-medium text-amber-800 underline underline-offset-2 hover:text-amber-900 dark:text-amber-300 dark:hover:text-amber-200"
          onclick={() => library.showNeedsReview()}
        >
          {p.needs_review} need review — show them
        </button>
      {/if}
      {#if p.skipped > 0}
        · {p.skipped} skipped
      {/if}
    {/if}
  </div>
{/if}
