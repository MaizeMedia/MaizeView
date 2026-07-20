<script lang="ts">
  import { AlertTriangle } from "@lucide/svelte";
  import Button from "$components/ui/button/button.svelte";
  import { scanPaths } from "$lib/api";
  import { library } from "$lib/stores/library.svelte";

  let offlinePaths = $state<string[]>([]);

  async function refresh() {
    try {
      const rows = await scanPaths.list();
      offlinePaths = rows.filter((p) => p.accessible === false).map((p) => p.path);
    } catch {
      offlinePaths = [];
    }
  }

  // Re-check when scan paths change (add/remove) or the user navigates views.
  $effect(() => {
    void library.scanPathsEpoch;
    void library.view;
    void refresh();
  });

  function goSettings() {
    library.view = "settings";
  }
</script>

{#if offlinePaths.length > 0}
  <div
    class="flex items-start gap-3 border-b border-amber-500/30 bg-amber-500/10 px-4 py-2 text-sm text-amber-100"
    role="status"
  >
    <AlertTriangle class="mt-0.5 size-4 shrink-0 text-amber-400" />
    <div class="min-w-0 flex-1">
      <div class="font-medium text-amber-50">
        {offlinePaths.length} library folder{offlinePaths.length === 1 ? "" : "s"} offline
      </div>
      <div class="mt-0.5 truncate text-xs text-amber-100/80">
        {offlinePaths.join(" · ")}
      </div>
      <p class="mt-1 text-xs text-amber-100/70">
        Scans skip unreachable folders. Reconnect the drive or update paths in Settings.
      </p>
    </div>
    <Button variant="outline" size="sm" class="shrink-0 border-amber-400/40" onclick={goSettings}>
      Settings
    </Button>
  </div>
{/if}
