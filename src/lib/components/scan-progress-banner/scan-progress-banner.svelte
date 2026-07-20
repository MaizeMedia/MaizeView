<script lang="ts">
  import { X, Loader2, FolderSearch, FileSearch, Database, CheckCircle2, AlertCircle } from "@lucide/svelte";
  import Button from "$components/ui/button/button.svelte";
  import { library } from "$lib/stores/library.svelte";
  import type { ScanProgress } from "$lib/api/types";

  let { progress }: { progress: ScanProgress } = $props();

  const phaseMeta: Record<string, { label: string; icon: typeof FolderSearch }> = {
    walking: { label: "Discovering files", icon: FolderSearch },
    indexing: { label: "Hashing & probing", icon: FileSearch },
    writing: { label: "Writing to library", icon: Database },
    done: { label: "Done", icon: CheckCircle2 },
  };

  // Progress percent: based on processed vs found. During 'walking' we don't
  // know the total yet, so show an indeterminate shimmer.
  let pct = $derived.by(() => {
    if (progress.phase === "walking" || progress.files_found === 0) return null;
    return Math.min(100, Math.round((progress.files_processed / progress.files_found) * 100));
  });

  let isRunning = $derived(progress.status === "running");
  let isCancelled = $derived(progress.status === "cancelled");
  let phaseLabel = $derived(phaseMeta[progress.phase]?.label ?? progress.phase);
  let PhaseIcon = $derived(phaseMeta[progress.phase]?.icon ?? AlertCircle);

  function shortPath(p: string): string {
    const parts = p.split(/[\\/]/);
    return parts.slice(-2).join("/");
  }
</script>

{#snippet content()}
  <div class="flex items-center gap-3 px-4 py-2 text-sm">
    <!-- status icon -->
    {#if isRunning}
      <Loader2 class="size-4 shrink-0 animate-spin text-primary" />
    {:else if isCancelled}
      <AlertCircle class="size-4 shrink-0 text-muted-foreground" />
    {:else}
      <CheckCircle2 class="size-4 shrink-0 text-primary" />
    {/if}

    <PhaseIcon class="size-4 shrink-0 text-muted-foreground" />

    <div class="min-w-0 flex-1">
      {#if isRunning}
        <div class="flex items-baseline gap-2">
          <span class="font-medium">{phaseLabel}</span>
          {#if pct !== null}
            <span class="tabular-nums text-muted-foreground">{pct}%</span>
          {/if}
        </div>
        <div class="mt-1 flex items-center gap-3 text-xs text-muted-foreground">
          <span class="tabular-nums">{progress.files_processed}/{progress.files_found} files</span>
          {#if progress.skipped_paths?.length}
            <span class="text-amber-400">
              Skipped {progress.skipped_paths.length} offline folder{progress.skipped_paths.length === 1 ? "" : "s"}
            </span>
          {/if}
          {#if progress.files_added > 0}
            <span class="tabular-nums text-primary">+{progress.files_added} new</span>
          {/if}
          {#if progress.current_path}
            <span class="truncate">{shortPath(progress.current_path)}</span>
          {/if}
        </div>
        <!-- progress bar (or shimmer when indeterminate) -->
        <div class="mt-1.5 h-1 w-full overflow-hidden rounded-full bg-secondary">
          {#if pct !== null}
            <div class="h-full bg-primary transition-all duration-200" style="width: {pct}%"></div>
          {:else}
            <div class="h-full w-1/3 animate-[shimmer_1.2s_ease-in-out_infinite] rounded-full bg-primary"></div>
          {/if}
        </div>
      {:else}
        <div class="flex items-center gap-3">
          <span class="font-medium">
            {#if isCancelled}
              Scan cancelled — kept {progress.files_processed} of {progress.files_found} files
            {:else}
              Scan complete
            {/if}
          </span>
          <span class="text-xs text-muted-foreground">
            {progress.files_added} added · {progress.files_updated} updated{#if !isCancelled} · {progress.files_removed} removed{/if}
          </span>
          {#if isCancelled}
            <span class="text-xs text-muted-foreground">
              Re-scan anytime to pick up the rest
            </span>
          {/if}
        </div>
      {/if}
    </div>

    {#if isRunning}
      <Button variant="outline" size="sm" onclick={() => library.cancelScan()}>
        <X class="size-3.5" />
        Cancel
      </Button>
    {/if}
  </div>
{/snippet}

{#if isRunning}
  <!-- Always-visible banner while scanning -->
  <div class="border-b border-primary/20 bg-primary/5">
    {@render content()}
  </div>
{:else if library.lastProgress && (library.lastProgress.status === "completed" || library.lastProgress.status === "cancelled")}
  <!-- Brief completion toast that auto-dismisses via store on next scan -->
  <div class="border-b border-border bg-card/60">
    {@render content()}
  </div>
{/if}

<style>
  @keyframes shimmer {
    0% { transform: translateX(-100%); }
    100% { transform: translateX(400%); }
  }
</style>
