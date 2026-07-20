<script lang="ts">
  import Sidebar from "$components/sidebar/sidebar.svelte";
  import Topbar from "$components/topbar/topbar.svelte";
  import LibraryGrid from "$components/library/library-grid.svelte";
  import ScanPathsPanel from "$components/settings/scan-paths-panel.svelte";
  import ScanProgressBanner from "$components/scan-progress-banner/scan-progress-banner.svelte";
  import OfflinePathsBanner from "$components/offline-paths-banner/offline-paths-banner.svelte";
  import SceneDrawer from "$components/detail/scene-drawer.svelte";
  import PlaylistsView from "$components/playlists/playlists-view.svelte";
  import TagsView from "$components/tags/tags-view.svelte";
  import DuplicatesView from "$components/duplicates/duplicates-view.svelte";
  import BatchIdentifyBanner from "$components/identify/batch-identify-banner.svelte";
  import ConvertProgressBanner from "$components/library/convert-progress-banner.svelte";
  import { library } from "$lib/stores/library.svelte";
  import { onMount } from "svelte";

  onMount(() => {
    void library.ensureProgressListener();
    void library.ensureBatchIdentifyListener();
    void library.ensureSceneDeletedListener();
    void library.ensureTranscodeListener();
  });
</script>

<div class="flex h-screen w-screen overflow-hidden">
  <Sidebar />

  <div class="flex min-w-0 flex-1 flex-col">
    <Topbar />
    <OfflinePathsBanner />
    {#if library.lastProgress}
      <ScanProgressBanner progress={library.lastProgress} />
    {/if}
    <BatchIdentifyBanner />
    <ConvertProgressBanner />
    <div class="flex min-h-0 flex-1">
      <main class="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden p-6">
        {#if library.view === "library" || library.view === "favorites"}
          <LibraryGrid />
        {:else}
      <div class="min-h-0 flex-1">
        {#if library.view === "settings"}
          <div class="h-full overflow-y-auto">
            <ScanPathsPanel />
          </div>
        {:else if library.view === "playlists"}
          <div class="h-full min-h-0 overflow-hidden">
            <PlaylistsView />
          </div>
        {:else if library.view === "tags"}
          <div class="h-full overflow-y-auto">
            <TagsView />
          </div>
        {:else if library.view === "duplicates"}
          <div class="h-full overflow-y-auto">
            <DuplicatesView />
          </div>
        {/if}
      </div>
        {/if}
      </main>

      {#if library.selectedSceneId}
        <SceneDrawer sceneId={library.selectedSceneId} />
      {/if}
    </div>
  </div>
</div>
