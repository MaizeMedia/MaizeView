<script lang="ts">
  import { onMount } from "svelte";
  import { Plus, Trash2, Loader2, Filter } from "@lucide/svelte";
  import Button from "$components/ui/button/button.svelte";
  import Input from "$components/ui/input/input.svelte";
  import EmptyState from "$components/empty-state/empty-state.svelte";
  import { tags as tagsApi, type TagWithCount } from "$lib/api";
  import { catalogs } from "$lib/stores/catalogs.svelte";
  import { library } from "$lib/stores/library.svelte";
  import { stringifyError } from "$lib/utils";

  let all = $state<TagWithCount[]>([]);
  let loading = $state(true);
  let creating = $state(false);
  let newName = $state("");
  let error = $state<string | null>(null);
  let confirmDeleteId = $state<string | null>(null);

  async function load() {
    loading = true;
    try {
      all = await tagsApi.listWithCounts();
    } catch (e) {
      error = stringifyError(e);
    } finally {
      loading = false;
    }
  }

  async function create() {
    if (!newName.trim()) return;
    try {
      await catalogs.createTag(newName.trim());
      newName = "";
      creating = false;
      await load();
    } catch (e) {
      error = stringifyError(e);
    }
  }

  async function remove(id: string) {
    try {
      await catalogs.deleteTag(id);
      confirmDeleteId = null;
      await load();
    } catch (e) {
      error = stringifyError(e);
    }
  }

  /** Click a tag → jump to library filtered to it. */
  function filterBy(id: string) {
    library.view = "library";
    library.tagIds = [id];
    void library.refresh();
  }

  onMount(load);
</script>

<section class="mx-auto w-full max-w-4xl space-y-6">
  <header class="flex items-center justify-between">
    <div>
      <h1 class="text-2xl font-semibold tracking-tight">Tags</h1>
      <p class="text-xs text-muted-foreground">
        {all.length} tag{all.length === 1 ? "" : "s"} · click a tag to filter the library
      </p>
    </div>
    <Button variant="outline" size="sm" onclick={() => (creating = !creating)}>
      <Plus class="size-4" />
      New tag
    </Button>
  </header>

  {#if error}
    <div class="rounded-md border border-destructive/40 bg-destructive/10 px-4 py-3 text-sm text-destructive">
      {error}
    </div>
  {/if}

  {#if creating}
    <div class="flex gap-2">
      <Input
        bind:value={newName}
        placeholder="Tag name"
        onkeydown={(e) => {
          if (e.key === "Enter") create();
          if (e.key === "Escape") { creating = false; newName = ""; }
        }}
      />
      <Button onclick={create} disabled={!newName.trim()}>Create</Button>
      <Button variant="ghost" onclick={() => { creating = false; newName = ""; }}>Cancel</Button>
    </div>
  {/if}

  {#if loading}
    <div class="flex justify-center py-12 text-muted-foreground">
      <Loader2 class="size-6 animate-spin" />
    </div>
  {:else if all.length === 0}
    <EmptyState
      title="No tags yet"
      hint="Add tags to scenes from the detail drawer, or create one above."
    />
  {:else}
    <div class="flex flex-wrap gap-2">
      {#each all as t (t.id)}
        <div class="group flex items-center gap-1 rounded-full border border-border bg-card pl-3 pr-1 py-1 text-sm transition-colors hover:border-primary/50">
          <button
            type="button"
            class="flex items-center gap-1.5"
            onclick={() => filterBy(t.id)}
            title="Filter library by this tag"
          >
            <span>#{t.name}</span>
            <span class="rounded-full bg-secondary px-1.5 text-[10px] tabular-nums text-muted-foreground">
              {t.scene_count}
            </span>
          </button>
          {#if confirmDeleteId === t.id}
            <span class="flex items-center gap-1 pl-1 text-xs">
              <button type="button" class="text-destructive hover:underline" onclick={() => remove(t.id)}>Delete</button>
              <button type="button" class="text-muted-foreground hover:underline" onclick={() => (confirmDeleteId = null)}>No</button>
            </span>
          {:else}
            <button
              type="button"
              class="ml-1 flex size-5 items-center justify-center rounded-full text-muted-foreground opacity-0 transition-opacity hover:bg-destructive/15 hover:text-destructive group-hover:opacity-100"
              aria-label="Delete tag"
              title="Delete tag"
              onclick={() => (confirmDeleteId = t.id)}
            >
              <Trash2 class="size-3.5" />
            </button>
          {/if}
        </div>
      {/each}
    </div>

    <div class="flex items-center gap-1.5 pt-2 text-xs text-muted-foreground">
      <Filter class="size-3" />
      Tip: combine tags + performers in the topbar Filter for finer searches.
    </div>
  {/if}
</section>
