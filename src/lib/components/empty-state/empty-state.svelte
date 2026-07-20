<script lang="ts">
  import { Library, FolderPlus } from "@lucide/svelte";
  import Button from "$components/ui/button/button.svelte";

  let {
    title = "Your library is empty",
    hint = "Add a folder and run a scan to populate it.",
    actionLabel = "Add library folder",
    onAction,
  }: {
    title?: string;
    hint?: string;
    actionLabel?: string;
    onAction?: () => void;
  } = $props();
</script>

<div class="flex h-full flex-col items-center justify-center gap-4 text-center">
  <div class="flex size-16 items-center justify-center rounded-full bg-secondary text-muted-foreground">
    <Library class="size-7" />
  </div>
  <div class="space-y-1">
    <h2 class="text-lg font-semibold">{title}</h2>
    <p class="text-sm text-muted-foreground">{hint}</p>
  </div>
  {#if onAction}
    <Button onclick={onAction}>
      <FolderPlus class="size-4" />
      {actionLabel}
    </Button>
  {/if}
</div>
