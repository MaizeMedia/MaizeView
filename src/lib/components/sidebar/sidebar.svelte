<script lang="ts">
  import { Library, Heart, ListMusic, Tags, Settings, ScanLine, Loader2, Copy } from "@lucide/svelte";
  import Separator from "$components/ui/separator/separator.svelte";
  import PrivacyBox from "./privacy-box.svelte";
  import DonateBox from "./donate-box.svelte";
  import { library, type View } from "$lib/stores/library.svelte";

  const primary: { id: View; label: string; icon: typeof Library }[] = [
    { id: "library", label: "Library", icon: Library },
    { id: "favorites", label: "Favorites", icon: Heart },
    { id: "playlists", label: "Playlists", icon: ListMusic },
    { id: "tags", label: "Tags", icon: Tags },
    { id: "duplicates", label: "Duplicates", icon: Copy },
  ];

  const secondary: { id: View; label: string; icon: typeof Library }[] = [
    { id: "settings", label: "Settings", icon: Settings },
  ];

  function nav(id: View) {
    library.view = id;
    if (id === "library" || id === "favorites") {
      void library.refresh();
    }
    if (id === "duplicates") {
      // view loads its own data on mount
    }
  }
</script>

<aside class="flex h-full w-56 flex-col border-r border-border bg-card/40">
  <div class="flex items-center gap-2 px-5 py-4">
    <span class="text-xl">🌽</span>
    <span class="text-sm font-semibold tracking-tight">MaizeView</span>
  </div>

  <Separator />

  <nav class="flex flex-1 flex-col gap-1 p-3">
    <div class="flex flex-col gap-1">
      {#each primary as item (item.id)}
        {@const Icon = item.icon}
        <button
          type="button"
          onclick={() => nav(item.id)}
          data-testid="nav-{item.id}"
          class="flex items-center gap-3 rounded-md px-3 py-2 text-sm font-medium transition-colors
            {library.view === item.id
            ? 'bg-secondary text-secondary-foreground'
            : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground'}"
        >
          <Icon class="size-4" />
          {item.label}
        </button>
      {/each}
    </div>

    <div class="mt-auto flex flex-col gap-1">
      <PrivacyBox />
      <button
        type="button"
        onclick={() => library.startScan()}
        disabled={library.scanning}
        data-testid="scan-library"
        class="flex items-center gap-2 rounded-md bg-primary px-3 py-2 text-sm font-semibold text-primary-foreground transition-colors hover:bg-primary/90 disabled:opacity-60"
      >
        {#if library.scanning}
          <Loader2 class="size-4 animate-spin" />
          Scanning…
        {:else}
          <ScanLine class="size-4" />
          Scan library
        {/if}
      </button>

      <Separator class="my-2" />

      {#each secondary as item (item.id)}
        {@const Icon = item.icon}
        <button
          type="button"
          onclick={() => nav(item.id)}
          data-testid="nav-{item.id}"
          class="flex items-center gap-3 rounded-md px-3 py-2 text-sm font-medium transition-colors
            {library.view === item.id
            ? 'bg-secondary text-secondary-foreground'
            : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground'}"
        >
          <Icon class="size-4" />
          {item.label}
        </button>
      {/each}

      <DonateBox />
    </div>
  </nav>
</aside>
