<script lang="ts">
  import { Loader2, X } from "@lucide/svelte";
  import Button from "$components/ui/button/button.svelte";
  import { library } from "$lib/stores/library.svelte";

  function shortPath(p: string): string {
    const parts = p.split(/[\\/]/);
    return parts.slice(-2).join("/");
  }

  let p = $derived(library.transcodeProgress);
</script>

{#if library.transcoding && p && !p.finished}
  <div
    class="border-b border-primary/25 bg-card/90 px-4 py-2.5"
    data-testid="transcode-progress"
  >
    <div class="flex items-center gap-3">
      <Loader2 class="size-4 shrink-0 animate-spin text-primary" />
      <div class="min-w-0 flex-1">
        <div class="text-sm font-medium">
          Converting {p.done}/{p.total}
          {#if p.skipped > 0}· {p.skipped} skipped{/if}
          {#if p.encoder}· {p.encoder}{/if}
          {#if p.filePercent !== null}
            · {p.filePercent}%
          {/if}
        </div>
        {#if p.currentPath}
          <div class="mt-0.5 truncate text-xs text-muted-foreground" title={p.currentPath}>
            {shortPath(p.currentPath)}
          </div>
        {/if}
        {#if p.filePercent !== null}
          <div class="mt-1 h-1.5 w-full overflow-hidden rounded-full bg-muted">
            <div
              class="h-full bg-primary transition-all"
              style="width: {p.filePercent}%"
            ></div>
          </div>
        {/if}
      </div>
      <Button
        variant="ghost"
        size="sm"
        onclick={() => void library.cancelTranscode()}
        disabled={library.transcodeCancelRequested}
      >
        <X class="size-3.5" />
        Cancel
      </Button>
    </div>
  </div>
{:else if p?.finished && p.total > 0}
  {@const failed = p.failed.length}
  {@const converted = p.done - failed - p.skipped}
  <div class="border-b border-border bg-card/60 px-4 py-2 text-xs text-muted-foreground">
    Convert finished: {converted} done
    {#if p.skipped > 0}· {p.skipped} skipped{/if}
    {#if failed > 0}
      · <span class="text-destructive">{failed} failed</span>
      {#if p.failed[0]}
        — {p.failed[0].reason}
      {/if}
    {/if}
  </div>
{/if}
