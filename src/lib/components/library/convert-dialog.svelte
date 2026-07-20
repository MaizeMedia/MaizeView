<script lang="ts">
  import { onMount } from "svelte";
  import { Loader2, Zap, FileDown, Tag, ChevronDown, AlertTriangle } from "@lucide/svelte";
  import Button from "$components/ui/button/button.svelte";
  import {
    transcode as transcodeApi,
    type DownscalePreview,
    type OriginalMode,
    type FilenameMode,
    type TagMode,
  } from "$lib/api";
  import { stringifyError } from "$lib/utils";

  let {
    sceneIds = [],
    onclose,
  }: { sceneIds: string[]; onclose: () => void } = $props();

  // --- Preview state (fetched from backend) ---
  let preview = $state<DownscalePreview | null>(null);
  let previewError = $state<string | null>(null);
  let loading = $state(true);

  // --- User-selected options ---
  let targetHeight = $state<number>(1080);
  let originalMode = $state<OriginalMode>("replace");
  let filenameMode = $state<FilenameMode>("replace");
  let tagMode = $state<TagMode>("swap");
  let submitting = $state(false);
  let showNames = $state(false);

  const TARGETS = [
    { h: 1080, label: "1080p" },
    { h: 720, label: "720p" },
    { h: 1440, label: "1440p" },
  ];

  async function loadPreview() {
    loading = true;
    previewError = null;
    try {
      preview = await transcodeApi.preview(sceneIds, targetHeight);
    } catch (e) {
      previewError = stringifyError(e);
    } finally {
      loading = false;
    }
  }

  // Reload the preview when the target changes (debounced via the select).
  $effect(() => {
    void targetHeight;
    if (sceneIds.length > 0) {
      void loadPreview();
    }
  });

  function formatBytes(bytes: number): string {
    if (bytes <= 0) return "0";
    const gb = bytes / 1_073_741_824;
    const mb = bytes / 1_048_576;
    if (gb >= 1) return `${gb.toFixed(1)} GB`;
    return `${mb.toFixed(0)} MB`;
  }

  function basename(p: string | null): string {
    if (!p) return "";
    return p.split(/[\\/]/).pop() ?? p;
  }

  async function confirmConvert() {
    submitting = true;
    try {
      await transcodeApi.start({
        sceneIds,
        targetHeight,
        originalMode,
        filenameMode,
        tagMode,
      });
      onclose();
    } catch (e) {
      previewError = stringifyError(e);
      submitting = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape" && !submitting) onclose();
  }

  onMount(() => {
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  });

  let wouldTranscode = $derived(preview?.wouldTranscode ?? 0);
  let canConfirm = $derived(wouldTranscode > 0 && !submitting && !loading);
</script>

<!-- Modal overlay -->
<div
  class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
  role="presentation"
  data-testid="convert-dialog"
  onclick={() => !submitting && onclose()}
  onkeydown={(e) => {
    if ((e.key === "Enter" || e.key === " ") && !submitting) onclose();
  }}
>
  <div
    class="max-h-[90vh] w-full max-w-lg overflow-y-auto rounded-lg border border-border bg-card shadow-xl"
    role="dialog"
    aria-modal="true"
    aria-labelledby="convert-title"
    tabindex="-1"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => e.stopPropagation()}
  >
    <div class="border-b border-border px-5 py-4">
      <div class="flex items-center gap-2">
        <Zap class="size-5 text-primary" />
        <h2 id="convert-title" class="text-lg font-semibold">
          Convert {sceneIds.length} scene{sceneIds.length === 1 ? "" : "s"}
        </h2>
      </div>
      <p class="mt-1 text-sm text-muted-foreground">
        Downscale selected videos to a lower resolution to save space.
      </p>
    </div>

    <div class="space-y-4 px-5 py-4">
      {#if loading}
        <div class="flex items-center gap-2 text-sm text-muted-foreground">
          <Loader2 class="size-4 animate-spin" /> Analyzing selection…
        </div>
      {:else if previewError}
        <div class="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
          {previewError}
        </div>
      {:else if preview}
        <!-- Breakdown -->
        <div class="rounded-md border border-border bg-background/50 p-3">
          <div class="text-sm font-medium">Current resolutions</div>
          <div class="mt-2 flex flex-wrap gap-2" data-testid="convert-breakdown">
            {#each Object.entries(preview.byResolution) as [token, count]}
              <span class="rounded bg-muted px-2 py-0.5 text-xs">
                {count} at {token}
              </span>
            {/each}
          </div>
          <div class="mt-2 text-xs text-muted-foreground">
            {wouldTranscode} would be transcoded · {preview.skipped} already ≤ target
          </div>
          {#if preview.estimatedBytesSaved > 0}
            <div class="mt-2 text-xs text-primary">
              ≈ {formatBytes(preview.estimatedBytesSaved)} saved (estimated)
            </div>
          {/if}
        </div>

        <!-- Target resolution -->
        <label class="block">
          <span class="text-sm font-medium">Target resolution</span>
          <select
            data-testid="convert-target"
            class="mt-1 h-9 w-full rounded-md border border-input bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
            bind:value={targetHeight}
          >
            {#each TARGETS as t}
              <option value={t.h}>{t.label}</option>
            {/each}
          </select>
        </label>

        <!-- Original handling -->
        <div>
          <div class="flex items-center gap-1.5 text-sm font-medium">
            <FileDown class="size-3.5" /> Original file
          </div>
          <div class="mt-1 space-y-1">
            <label class="flex items-center gap-2 text-sm">
              <input type="radio" value="replace" bind:group={originalMode} data-testid="convert-original-replace" />
              Replace original (saves space)
            </label>
            <label class="flex items-center gap-2 text-sm">
              <input type="radio" value="keep" bind:group={originalMode} data-testid="convert-original-keep" />
              Keep both (original stays, add transcoded)
            </label>
          </div>
        </div>

        <!-- Filename handling -->
        <div>
          <div class="text-sm font-medium">Filename tokens (4K / 2160p / UHD)</div>
          <div class="mt-1 space-y-1">
            <label class="flex items-center gap-2 text-sm">
              <input type="radio" value="replace" bind:group={filenameMode} data-testid="convert-filename-replace" />
              Replace with target (e.g. 4K → 1080p)
            </label>
            <label class="flex items-center gap-2 text-sm">
              <input type="radio" value="remove" bind:group={filenameMode} />
              Remove token
            </label>
            <label class="flex items-center gap-2 text-sm">
              <input type="radio" value="leave" bind:group={filenameMode} />
              Leave filename
            </label>
          </div>
        </div>

        <!-- Tag handling -->
        <div>
          <div class="flex items-center gap-1.5 text-sm font-medium">
            <Tag class="size-3.5" /> Resolution tags
          </div>
          <div class="mt-1 space-y-1">
            <label class="flex items-center gap-2 text-sm">
              <input type="radio" value="swap" bind:group={tagMode} data-testid="convert-tag-swap" />
              Swap (remove old, add target tag)
            </label>
            <label class="flex items-center gap-2 text-sm">
              <input type="radio" value="remove" bind:group={tagMode} />
              Remove old tag only
            </label>
            <label class="flex items-center gap-2 text-sm">
              <input type="radio" value="leave" bind:group={tagMode} />
              Leave tags
            </label>
          </div>
        </div>

        <!-- Filename preview (collapsible) -->
        {#if preview.items.some((i) => i.previewFilename && basename(i.currentPath) !== i.previewFilename)}
          <div>
            <button
              type="button"
              class="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground"
              onclick={() => (showNames = !showNames)}
            >
              <ChevronDown class="size-3 transition-transform {showNames ? 'rotate-180' : ''}" />
              {showNames ? "Hide" : "Preview"} filename changes
            </button>
            {#if showNames}
              <div class="mt-2 max-h-40 space-y-1.5 overflow-y-auto rounded border border-border p-2 text-xs">
                {#each preview.items.filter((i) => i.previewFilename && basename(i.currentPath) !== i.previewFilename).slice(0, 20) as item}
                  <div>
                    <span class="text-muted-foreground line-through">{basename(item.currentPath)}</span>
                    <span class="mx-1 text-muted-foreground">→</span>
                    <span>{item.previewFilename}</span>
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        {/if}

        {#if originalMode === "replace" && wouldTranscode > 0}
          <div class="flex items-start gap-2 rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-xs text-amber-600 dark:text-amber-400">
            <AlertTriangle class="mt-0.5 size-3.5 shrink-0" />
            <span>
              Replace mode permanently deletes each original after its transcode is verified.
              This cannot be undone.
            </span>
          </div>
        {/if}
      {/if}
    </div>

    <div class="flex items-center justify-end gap-2 border-t border-border px-5 py-3">
      <Button variant="ghost" onclick={() => onclose()} disabled={submitting}>
        Cancel
      </Button>
      <Button onclick={confirmConvert} disabled={!canConfirm} data-testid="convert-confirm">
        {#if submitting}
          <Loader2 class="size-3.5 animate-spin" />
        {:else}
          <Zap class="size-3.5" />
        {/if}
        Convert {wouldTranscode > 0 ? `${wouldTranscode}` : ""}
      </Button>
    </div>
  </div>
</div>
