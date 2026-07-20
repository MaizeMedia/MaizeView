<script lang="ts">
  import { onMount } from "svelte";
  import { FolderOpen, Trash2, ScanLine, RefreshCw, Loader2, Image, Sparkles, Search, X } from "@lucide/svelte";
  import { confirm } from "@tauri-apps/plugin-dialog";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { stringifyError } from "$lib/utils";
  import Button from "$components/ui/button/button.svelte";
  import Input from "$components/ui/input/input.svelte";
  import Separator from "$components/ui/separator/separator.svelte";
  import { pickFolder, pickSqliteFile, scanPaths, previews as previewsApi, fingerprints as fingerprintsApi, playerSettings, appearanceSettings, jobSettings, stashdbSettings, identify as identifyApi, stashImport, pathMeta as pathMetaApi, updates as updatesApi, DEFAULT_BATCH_IDENTIFY_LIBRARY_OPTIONS, type PreviewProgress, type FingerprintProgress, type PhashProgress, type StashDbIdentifyStats, type StashBoxPreset, type ImportStashResult, type BatchPathMetadataResult, type JobSettings, type UpdateCheck } from "$lib/api";
  import { getVersion } from "@tauri-apps/api/app";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { ACCENT_PRESETS, applyAccentPreset, isAccentPresetId, type AccentPresetId, DEFAULT_ACCENT } from "$lib/theme";
  import { library } from "$lib/stores/library.svelte";
  import type { ScanPath } from "$lib/api/types";

  let paths = $state<ScanPath[]>([]);
  let loading = $state(false);
  let pickedPath = $state(""); // chosen via Browse, committed via Add
  let newLabel = $state("");
  let error = $state<string | null>(null);
  let generatingPreviews = $state(false);
  let previewProgress = $state<PreviewProgress | null>(null);
  let generatingFingerprints = $state(false);
  let fingerprintProgress = $state<FingerprintProgress | null>(null);
  let generatingPhash = $state(false);
  let phashProgress = $state<PhashProgress | null>(null);
  let playbackVolume = $state(75);
  let savingVolume = $state(false);
  let playerDeleteEnabled = $state(false);
  let volumeSaveTimer: ReturnType<typeof setTimeout> | null = null;
  let stashBoxPresets = $state<StashBoxPreset[]>([]);
  let stashBoxActiveId = $state("stashdb");
  let stashBoxWaterfall = $state(false);
  let stashdbEndpoint = $state("https://stashdb.org/graphql");
  let stashdbAccountUrl = $state("https://stashdb.org/users/me");
  let stashdbApiKey = $state("");
  let stashdbKeyConfigured = $state(false);
  let savingStashdb = $state(false);
  let testingStashdb = $state(false);
  let stashdbTestMessage = $state<string | null>(null);
  let stashdbTestOk = $state(false);
  let identifySkipDays = $state(DEFAULT_BATCH_IDENTIFY_LIBRARY_OPTIONS.skip_within_days);
  let identifyForceRescan = $state(false);
  let identifyStats = $state<StashDbIdentifyStats | null>(null);
  let importingStash = $state(false);
  let stashImportResult = $state<ImportStashResult | null>(null);
  let batchPathRunning = $state(false);
  let batchPathResult = $state<BatchPathMetadataResult | null>(null);
  let accentPreset = $state<AccentPresetId>(DEFAULT_ACCENT);
  let savingAccent = $state(false);
  let jobWorkersMax = $state(0);
  let jobEffectiveWorkers = $state(2);
  let jobCpuCount = $state(0);
  let savingJobWorkers = $state(false);
  let jobWorkersSaveTimer: ReturnType<typeof setTimeout> | null = null;

  // About / updates — manual check only; the app makes no background calls.
  let appVersion = $state("");
  let checkingUpdates = $state(false);
  let updateResult = $state<UpdateCheck | null>(null);
  let updateError = $state<string | null>(null);

  async function checkUpdates() {
    if (checkingUpdates) return;
    checkingUpdates = true;
    updateError = null;
    updateResult = null;
    try {
      updateResult = await updatesApi.check();
    } catch (e) {
      updateError = stringifyError(e);
    } finally {
      checkingUpdates = false;
    }
  }

  onMount(async () => {
    appVersion = await getVersion().catch(() => "");
  });

  let jobWorkersLabel = $derived(
    jobWorkersMax === 0
      ? `Auto (${jobEffectiveWorkers})`
      : `${jobWorkersMax}`,
  );

  let activeStashBoxName = $derived(
    stashBoxPresets.find((p) => p.id === stashBoxActiveId)?.name ?? "StashDB",
  );

  async function refreshIdentifyStats() {
    try {
      identifyStats = await identifyApi.stats(identifySkipDays, identifyForceRescan);
    } catch {
      identifyStats = null;
    }
  }

  async function runLibraryIdentify() {
    await library.ensureBatchIdentifyListener();
    await library.batchIdentifyLibrary({
      auto_apply: true,
      skip_within_days: identifySkipDays,
      force_rescan: identifyForceRescan,
    });
    await refreshIdentifyStats();
  }

  async function runBatchPathMatch() {
    error = null;
    batchPathRunning = true;
    batchPathResult = null;
    try {
      batchPathResult = await pathMetaApi.batchApply();
      await library.refresh();
    } catch (e) {
      error = stringifyError(e);
    } finally {
      batchPathRunning = false;
    }
  }

  async function runStashImport() {
    error = null;
    stashImportResult = null;
    const chosen = await pickSqliteFile();
    if (!chosen) return;
    importingStash = true;
    try {
      stashImportResult = await stashImport.run(chosen);
      await library.refresh();
    } catch (e) {
      error = stringifyError(e);
    } finally {
      importingStash = false;
    }
  }

  async function loadPlaybackSettings() {
    try {
      const s = await playerSettings.get();
      playbackVolume = s.volume;
      playerDeleteEnabled = s.delete_in_player_enabled;
    } catch (e) {
      error = stringifyError(e);
    }
  }

  function applyJobSettings(s: JobSettings) {
    jobWorkersMax = s.workers_max;
    jobEffectiveWorkers = s.effective_workers;
    jobCpuCount = s.cpu_count;
  }

  async function loadJobSettings() {
    try {
      applyJobSettings(await jobSettings.get());
    } catch (e) {
      console.warn("load job settings failed", e);
    }
  }

  function onJobWorkersInput(e: Event) {
    jobWorkersMax = Number((e.currentTarget as HTMLInputElement).value);
    if (jobWorkersSaveTimer) clearTimeout(jobWorkersSaveTimer);
    jobWorkersSaveTimer = setTimeout(async () => {
      savingJobWorkers = true;
      try {
        applyJobSettings(await jobSettings.set(jobWorkersMax));
      } catch (err) {
        error = stringifyError(err);
      } finally {
        savingJobWorkers = false;
      }
    }, 300);
  }

  async function loadAppearanceSettings() {
    try {
      const s = await appearanceSettings.get();
      const id = isAccentPresetId(s.accent_preset) ? s.accent_preset : DEFAULT_ACCENT;
      accentPreset = id;
      applyAccentPreset(id);
    } catch (e) {
      console.warn("load appearance settings failed", e);
    }
  }

  async function selectAccent(id: AccentPresetId) {
    accentPreset = id;
    applyAccentPreset(id);
    savingAccent = true;
    try {
      await appearanceSettings.set(id);
    } catch (e) {
      error = stringifyError(e);
    } finally {
      savingAccent = false;
    }
  }

  async function togglePlayerDelete() {
    playerDeleteEnabled = !playerDeleteEnabled;
    try {
      const current = await playerSettings.get();
      await playerSettings.set(current.volume, current.muted, playerDeleteEnabled);
    } catch (e) {
      error = stringifyError(e);
      playerDeleteEnabled = !playerDeleteEnabled;
    }
  }

  function onPlaybackVolumeInput(e: Event) {
    playbackVolume = Number((e.currentTarget as HTMLInputElement).value);
    if (volumeSaveTimer) clearTimeout(volumeSaveTimer);
    volumeSaveTimer = setTimeout(async () => {
      savingVolume = true;
      try {
        const current = await playerSettings.get();
        await playerSettings.set(playbackVolume, current.muted);
      } catch (e) {
        error = stringifyError(e);
      } finally {
        savingVolume = false;
      }
    }, 300);
  }

  async function computeMissingPhash() {
    error = null;
    generatingPhash = true;
    phashProgress = null;
    try {
      // Fill gaps only — do not wipe existing rows.
      await fingerprintsApi.generatePhash(false);
    } catch (e) {
      error = stringifyError(e);
      generatingPhash = false;
    }
  }

  async function rebuildAllPhash() {
    const ok = await confirm(
      "Rebuild ALL pHash fingerprints? This deletes existing pHashes and recomputes from scratch (slow). Use only after an algorithm change.",
      { title: "Rebuild all pHashes", kind: "warning" },
    );
    if (!ok) return;
    error = null;
    generatingPhash = true;
    phashProgress = null;
    try {
      await fingerprintsApi.generatePhash(true);
    } catch (e) {
      error = stringifyError(e);
      generatingPhash = false;
    }
  }

  async function regenerateFingerprints() {
    error = null;
    generatingFingerprints = true;
    fingerprintProgress = null;
    try {
      await fingerprintsApi.generateMd5();
    } catch (e) {
      error = stringifyError(e);
      generatingFingerprints = false;
    }
  }

  async function regenerateThumbnails() {
    error = null;
    generatingPreviews = true;
    previewProgress = null;
    try {
      await previewsApi.generate();
    } catch (e) {
      error = stringifyError(e);
      generatingPreviews = false;
    }
  }

  async function stopPreviews() {
    try {
      await previewsApi.cancel();
    } catch (e) {
      error = stringifyError(e);
    }
  }

  async function stopMd5() {
    try {
      await fingerprintsApi.cancelMd5();
    } catch (e) {
      error = stringifyError(e);
    }
  }

  async function stopPhash() {
    try {
      await fingerprintsApi.cancelPhash();
    } catch (e) {
      error = stringifyError(e);
    }
  }

  async function stopAllMediaJobs() {
    try {
      await fingerprintsApi.cancelAllMediaJobs();
    } catch (e) {
      error = stringifyError(e);
    }
  }

  let anyMediaJob = $derived(
    generatingPreviews || generatingFingerprints || generatingPhash,
  );

  function applyStashBoxSettings(s: Awaited<ReturnType<typeof stashdbSettings.get>>) {
    stashBoxPresets = s.presets;
    stashBoxActiveId = s.active_id;
    stashBoxWaterfall = s.waterfall;
    stashdbEndpoint = s.endpoint;
    stashdbKeyConfigured = s.api_key_set;
    stashdbApiKey = "";
    const preset = s.presets.find((p) => p.id === s.active_id);
    stashdbAccountUrl = preset?.account_url ?? "https://stashdb.org/users/me";
  }

  function onStashBoxPresetChange(e: Event) {
    const id = (e.currentTarget as HTMLSelectElement).value;
    stashBoxActiveId = id;
    stashdbTestMessage = null;
    const preset = stashBoxPresets.find((p) => p.id === id);
    if (preset) {
      stashdbEndpoint = preset.endpoint;
      stashdbAccountUrl = preset.account_url;
      stashdbKeyConfigured = preset.api_key_set;
      stashdbApiKey = "";
    }
    // Persist active provider immediately so Test / Identify use this box.
    void (async () => {
      try {
        applyStashBoxSettings(await stashdbSettings.set(null, null, id));
      } catch (err) {
        error = stringifyError(err);
      }
    })();
  }

  async function loadStashdbSettings() {
    try {
      applyStashBoxSettings(await stashdbSettings.get());
    } catch (e) {
      error = stringifyError(e);
    }
  }

  async function saveStashdbSettings() {
    savingStashdb = true;
    error = null;
    stashdbTestMessage = null;
    try {
      const s = await stashdbSettings.set(
        stashdbApiKey.trim() || null,
        null,
        stashBoxActiveId,
        stashBoxWaterfall,
      );
      applyStashBoxSettings(s);
    } catch (e) {
      error = stringifyError(e);
    } finally {
      savingStashdb = false;
    }
  }

  async function testStashdbConnection() {
    testingStashdb = true;
    error = null;
    stashdbTestMessage = null;
    stashdbTestOk = false;
    try {
      const result = await stashdbSettings.test(
        stashdbApiKey.trim() || null,
        stashdbEndpoint.trim() || null,
      );
      stashdbTestOk = true;
      stashdbTestMessage = `Connected as ${result.username}`;
    } catch (e) {
      stashdbTestOk = false;
      stashdbTestMessage = stringifyError(e);
    } finally {
      testingStashdb = false;
    }
  }

  async function load() {
    loading = true;
    try {
      paths = await scanPaths.list();
    } catch (e) {
      error = stringifyError(e);
    } finally {
      loading = false;
    }
  }

  async function browse() {
    error = null;
    try {
      const chosen = await pickFolder();
      if (chosen) pickedPath = chosen;
    } catch (e) {
      error = stringifyError(e);
    }
  }

  async function add() {
    if (!pickedPath.trim()) return;
    error = null;
    try {
      const row = await scanPaths.add(pickedPath.trim(), newLabel.trim() || undefined);
      paths = [...paths, row];
      pickedPath = "";
      newLabel = "";
      library.bumpScanPaths();
    } catch (e) {
      error = stringifyError(e);
    }
  }

  async function remove(id: string) {
    try {
      await scanPaths.remove(id);
      paths = paths.filter((p) => p.id !== id);
      library.bumpScanPaths();
      await library.refresh();
      void refreshIdentifyStats();
    } catch (e) {
      error = stringifyError(e);
    }
  }

  onMount(() => {
    void load();
    void loadPlaybackSettings();
    void loadJobSettings();
    void loadAppearanceSettings();
    void loadStashdbSettings();
    void refreshIdentifyStats();
    void library.ensureProgressListener();
    const unlisteners: Promise<UnlistenFn>[] = [
      previewsApi.onProgress((p) => {
        previewProgress = p;
        generatingPreviews = !p.finished;
        if (p.finished) void library.refresh();
      }),
      fingerprintsApi.onMd5Progress((p) => {
        fingerprintProgress = p;
        generatingFingerprints = !p.finished;
      }),
      fingerprintsApi.onPhashProgress((p) => {
        phashProgress = p;
        generatingPhash = !p.finished;
      }),
    ];
    return () => {
      for (const u of unlisteners) void u.then((fn) => fn());
    };
  });
</script>

<section class="mx-auto w-full max-w-3xl space-y-6">
  <header class="space-y-1">
    <h1 class="text-2xl font-semibold tracking-tight">Settings</h1>
    <p class="text-sm text-muted-foreground">
      Library folders, playback defaults, and maintenance.
    </p>
  </header>

  {#if error}
    <div class="rounded-md border border-destructive/40 bg-destructive/10 px-4 py-3 text-sm text-destructive">
      {error}
    </div>
  {/if}

  <!-- Appearance -->
  <div class="space-y-3 rounded-lg border border-border bg-card p-4" data-testid="appearance-settings">
    <div>
      <h2 class="text-sm font-medium">Appearance</h2>
      <p class="text-xs text-muted-foreground">
        Accent color for buttons and highlights. Dark chrome stays; only the accent changes.
        {#if savingAccent}<span class="text-muted-foreground"> Saving…</span>{/if}
      </p>
    </div>
    <div class="flex flex-wrap gap-2" role="group" aria-label="Accent preset">
      {#each ACCENT_PRESETS as preset (preset.id)}
        <button
          type="button"
          class="flex items-center gap-2 rounded-md border px-3 py-2 text-sm transition
            {accentPreset === preset.id
              ? 'border-primary bg-primary/10 text-foreground'
              : 'border-border bg-background text-muted-foreground hover:border-primary/40 hover:text-foreground'}"
          aria-pressed={accentPreset === preset.id}
          onclick={() => void selectAccent(preset.id)}
        >
          <span
            class="size-3.5 rounded-full ring-1 ring-black/30"
            style:background={preset.swatch}
            aria-hidden="true"
          ></span>
          {preset.label}
        </button>
      {/each}
    </div>
  </div>

  <!-- Playback -->
  <div class="space-y-3 rounded-lg border border-border bg-card p-4">
    <div>
      <h2 class="text-sm font-medium">Playback volume</h2>
      <p class="text-xs text-muted-foreground">
        Default for new player windows. Adjusting volume in a player remembers your last level.
      </p>
    </div>
    <div class="flex items-center gap-4">
      <input
        type="range"
        min="0"
        max="100"
        step="1"
        value={playbackVolume}
        oninput={onPlaybackVolumeInput}
        class="h-1.5 flex-1 cursor-pointer accent-primary"
        aria-label="Default playback volume"
      />
      <span class="w-10 text-right text-sm tabular-nums text-muted-foreground">
        {Math.round(playbackVolume)}{savingVolume ? "…" : ""}
      </span>
    </div>
  </div>

  <!-- Background job intensity -->
  <div class="space-y-3 rounded-lg border border-border bg-card p-4" data-testid="job-intensity-settings">
    <div>
      <h2 class="text-sm font-medium">Background job intensity</h2>
      <p class="text-xs text-muted-foreground">
        Max parallel workers for library scan indexing and preview / pHash / MD5 jobs.
        Auto leaves headroom for the UI ({jobCpuCount || "…"} cores detected). Takes effect on the next job.
        {#if savingJobWorkers}<span class="text-muted-foreground"> Saving…</span>{/if}
      </p>
    </div>
    <div class="flex items-center gap-4">
      <span class="w-12 shrink-0 text-xs text-muted-foreground">Auto</span>
      <input
        type="range"
        min="0"
        max="16"
        step="1"
        value={jobWorkersMax}
        oninput={onJobWorkersInput}
        class="h-1.5 flex-1 cursor-pointer accent-primary"
        aria-label="Background job worker count"
      />
      <span class="w-20 shrink-0 text-right text-sm tabular-nums text-muted-foreground">
        {jobWorkersLabel}
      </span>
    </div>
    <p class="text-xs text-muted-foreground">
      Next job will use <span class="tabular-nums text-foreground">{jobEffectiveWorkers}</span> worker{jobEffectiveWorkers === 1 ? "" : "s"}.
      Drag right for a fixed count (1–16); leave at Auto when you want full speed with UI headroom.
    </p>
  </div>

  <div class="space-y-3 rounded-lg border border-border bg-card p-4">
    <div class="flex items-start justify-between gap-4">
      <div>
        <h2 class="text-sm font-medium">Delete from player</h2>
        <p class="text-xs text-muted-foreground">
          When enabled, the video player shows a delete button. Deletes the file from disk permanently (with confirmation).
        </p>
      </div>
      <label class="flex shrink-0 cursor-pointer items-center gap-2 text-sm">
        <input
          type="checkbox"
          checked={playerDeleteEnabled}
          onchange={togglePlayerDelete}
          class="size-4 accent-primary"
        />
        Show delete button
      </label>
    </div>
  </div>

  <Separator />

  <!-- Stash-box metadata -->
  <div class="space-y-3 rounded-lg border border-border bg-card p-4">
    <div>
      <h2 class="text-sm font-medium">Metadata providers (stash-box)</h2>
      <p class="text-xs text-muted-foreground">
        Primary provider for identify — StashDB, ThePornDB, FansDB, or JAVStash.
        Get a key from your
        <a href={stashdbAccountUrl} class="text-primary hover:underline" target="_blank" rel="noreferrer">{activeStashBoxName} account</a>.
        Optional waterfall tries other saved keys if fingerprints miss.
      </p>
    </div>
    <div class="space-y-1.5">
      <label for="stash-box-preset" class="text-xs font-medium text-muted-foreground">Provider</label>
      <select
        id="stash-box-preset"
        class="h-9 w-full rounded-md border border-input bg-background px-3 text-sm"
        value={stashBoxActiveId}
        onchange={onStashBoxPresetChange}
      >
        {#each stashBoxPresets as preset (preset.id)}
          <option value={preset.id}>
            {preset.name}{preset.api_key_set ? " · key saved" : ""}
          </option>
        {/each}
      </select>
    </div>
    <label class="flex cursor-pointer items-start gap-2 text-xs">
      <input
        type="checkbox"
        class="mt-0.5 accent-primary"
        checked={stashBoxWaterfall}
        onchange={(e) => {
          stashBoxWaterfall = (e.currentTarget as HTMLInputElement).checked;
          void stashdbSettings.set(null, null, null, stashBoxWaterfall).then(applyStashBoxSettings).catch((err) => {
            error = stringifyError(err);
          });
        }}
      />
      <span>
        <span class="font-medium">Waterfall identify</span>
        <span class="block text-muted-foreground">
          Try every provider with a saved API key (primary first) until fingerprints match. Title search stays on the primary / hit box.
        </span>
      </span>
    </label>
    <div class="space-y-1.5">
      <label for="stashdb-endpoint" class="text-xs font-medium text-muted-foreground">GraphQL endpoint</label>
      <Input id="stashdb-endpoint" value={stashdbEndpoint} readonly class="opacity-80" />
    </div>
    <div class="space-y-1.5">
      <label for="stashdb-key" class="text-xs font-medium text-muted-foreground">API key ({activeStashBoxName})</label>
      <Input
        id="stashdb-key"
        type="password"
        bind:value={stashdbApiKey}
        placeholder={stashdbKeyConfigured ? "•••••••• (configured — enter new key to replace)" : "Paste API key…"}
        autocomplete="off"
      />
    </div>
    <Button onclick={saveStashdbSettings} disabled={savingStashdb}>
      {#if savingStashdb}
        <Loader2 class="size-4 animate-spin" />
        Saving…
      {:else}
        Save provider settings
      {/if}
    </Button>
    <Button variant="outline" onclick={testStashdbConnection} disabled={testingStashdb}>
      {#if testingStashdb}
        <Loader2 class="size-4 animate-spin" />
        Testing…
      {:else}
        Test connection
      {/if}
    </Button>
    {#if stashdbTestMessage}
      <p
        class="text-xs"
        class:text-emerald-600={stashdbTestOk}
        class:text-destructive={!stashdbTestOk}
      >
        {stashdbTestMessage}
      </p>
    {/if}
    <div class="space-y-3 border-t border-border pt-3">
      <div class="grid gap-3 sm:grid-cols-2">
        <div class="space-y-1.5">
          <label for="identify-skip-days" class="text-xs font-medium text-muted-foreground">
            Skip if checked within
          </label>
          <select
            id="identify-skip-days"
            class="h-9 w-full rounded-md border border-input bg-background px-3 text-sm"
            value={String(identifySkipDays)}
            onchange={(e) => {
              identifySkipDays = Number(e.currentTarget.value);
              void refreshIdentifyStats();
            }}
            disabled={identifyForceRescan}
          >
            <option value={0}>Never skip</option>
            <option value={7}>7 days</option>
            <option value={30}>30 days</option>
            <option value={90}>90 days</option>
            <option value={365}>1 year</option>
          </select>
        </div>
        <label class="flex items-end gap-2 pb-2 text-sm">
          <input
            type="checkbox"
            bind:checked={identifyForceRescan}
            onchange={() => void refreshIdentifyStats()}
          />
          Re-identify all (ignore history)
        </label>
      </div>
      {#if identifyStats}
        <p class="text-xs text-muted-foreground">
          {identifyStats.pending} to run
          {#if identifyStats.checked_recently > 0 && !identifyForceRescan}
            · {identifyStats.checked_recently} skipped (checked recently)
          {/if}
          {#if identifyStats.never_checked > 0}
            · {identifyStats.never_checked} never checked
          {/if}
          {#if identifyStats.needs_review > 0}
            ·
            <button
              type="button"
              class="font-medium text-amber-800 underline underline-offset-2 hover:text-amber-900 dark:text-amber-300 dark:hover:text-amber-200"
              onclick={() => library.showNeedsReview()}
            >
              {identifyStats.needs_review} need review
            </button>
          {/if}
        </p>
      {/if}
      <div class="flex flex-wrap items-center gap-3">
        <Button
          variant="default"
          onclick={() => void runLibraryIdentify()}
          disabled={library.batchIdentifying || (identifyStats?.pending ?? 0) === 0}
        >
          {#if library.batchIdentifying}
            <Loader2 class="size-4 animate-spin" />
            Identifying…
          {:else}
            <Sparkles class="size-4" />
            Identify library
          {/if}
        </Button>
        {#if (identifyStats?.needs_review ?? 0) > 0}
          <Button variant="outline" onclick={() => library.showNeedsReview()}>
            Review {identifyStats?.needs_review} matches
          </Button>
        {/if}
        <span class="text-xs text-muted-foreground">
          Uses the active stash-box provider. Auto-applies only when exactly one match; otherwise open a scene and pick from Identify.
        </span>
      </div>
    </div>
  </div>

  <Separator />

  <!-- Batch path match (existing catalog only) -->
  <div class="space-y-3 rounded-lg border border-border bg-card p-4">
    <div>
      <h2 class="text-sm font-medium">Match paths across library</h2>
      <p class="text-xs text-muted-foreground">
        Link studios, performers, and tags that already exist in your catalog when their names appear in file paths or folders
        (skips media buckets like <code class="rounded bg-muted px-1">videos</code>). Does not create new names — use scene → Match path for that. Does not move files.
      </p>
    </div>
    <Button variant="outline" onclick={() => void runBatchPathMatch()} disabled={batchPathRunning}>
      {#if batchPathRunning}
        <Loader2 class="size-4 animate-spin" />
        Matching…
      {:else}
        <Search class="size-4" />
        Apply path matches
      {/if}
    </Button>
    {#if batchPathResult}
      <p class="text-xs text-muted-foreground">
        Scanned {batchPathResult.scenes_scanned} · hits on {batchPathResult.scenes_with_hits}
        · studios {batchPathResult.studios_linked}
        · performers {batchPathResult.performers_linked}
        · tags {batchPathResult.tags_linked}
      </p>
    {/if}
  </div>

  <Separator />

  <!-- Local Stash import -->
  <div class="space-y-3 rounded-lg border border-border bg-card p-4">
    <div>
      <h2 class="text-sm font-medium">Import from Stash</h2>
      <p class="text-xs text-muted-foreground">
        Match scenes by oshash / md5 / phash against a local <code class="rounded bg-muted px-1">stash-go.sqlite</code>
        and copy title, details, studio, performers, and tags. Does not move or rename files.
      </p>
    </div>
    <Button onclick={() => void runStashImport()} disabled={importingStash}>
      {#if importingStash}
        <Loader2 class="size-4 animate-spin" />
        Importing…
      {:else}
        Choose Stash database…
      {/if}
    </Button>
    {#if stashImportResult}
      <p class="text-xs text-muted-foreground">
        Matched {stashImportResult.matched} · updated {stashImportResult.updated}
        · unchanged {stashImportResult.skipped}
        {#if stashImportResult.errors > 0}
          · <span class="text-destructive">{stashImportResult.errors} errors</span>
        {/if}
      </p>
      {#if stashImportResult.last_error}
        <p class="text-xs text-destructive">{stashImportResult.last_error}</p>
      {/if}
    {/if}
  </div>

  <Separator />

  <header class="space-y-1">
    <h2 class="text-lg font-semibold tracking-tight">Library folders</h2>
    <p class="text-sm text-muted-foreground">
      Add folders containing your videos. MaizeView scans them recursively.
    </p>
  </header>

  <!-- Add new path -->
  <div class="space-y-3 rounded-lg border border-border bg-card p-4">
    <div class="flex items-end gap-3">
      <div class="min-w-0 flex-1 space-y-1.5">
        <label for="path" class="text-xs font-medium text-muted-foreground">Folder</label>
        <!-- Read-only display of the picked path; selection happens via Browse -->
        <Input
          id="path"
          value={pickedPath}
          placeholder="Click Browse… to choose a folder"
          readonly
          class="cursor-default"
          onclick={browse}
        />
      </div>
      <div class="w-44 space-y-1.5">
        <label for="label" class="text-xs font-medium text-muted-foreground">Label (optional)</label>
        <Input id="label" bind:value={newLabel} placeholder="Main" />
      </div>
      <Button variant="outline" onclick={browse}>
        <FolderOpen class="size-4" />
        Browse…
      </Button>
      <Button onclick={add} disabled={!pickedPath.trim()}>
        Add
      </Button>
    </div>
  </div>

  <Separator />

  <!-- Existing paths -->
  <div class="space-y-2">
    <div class="flex items-center justify-between">
      <h2 class="text-sm font-medium text-muted-foreground">
        Configured folders {#if paths.length}({paths.length}){/if}
      </h2>
      <Button variant="ghost" size="icon-sm" onclick={load} disabled={loading} aria-label="Refresh">
        {#if loading}
          <Loader2 class="size-4 animate-spin" />
        {:else}
          <RefreshCw class="size-4" />
        {/if}
      </Button>
    </div>

    {#if paths.length === 0}
      <p class="rounded-md border border-dashed border-border px-4 py-8 text-center text-sm text-muted-foreground">
        No folders yet. Click Browse… above to add one.
      </p>
    {:else}
      <ul class="space-y-2">
        {#each paths as p (p.id)}
          <li class="flex items-center gap-3 rounded-md border border-border bg-card/50 px-4 py-3">
            <div class="min-w-0 flex-1">
              <div class="flex items-center gap-2 truncate text-sm font-medium">
                <span class="truncate">{p.path}</span>
                {#if p.accessible === false}
                  <span class="shrink-0 rounded-full bg-amber-500/15 px-2 py-0.5 text-[10px] font-medium uppercase tracking-wide text-amber-400">
                    Offline
                  </span>
                {/if}
              </div>
              {#if p.label}
                <div class="text-xs text-muted-foreground">{p.label}</div>
              {/if}
            </div>
            <Button variant="ghost" size="icon-sm" onclick={() => remove(p.id)} aria-label="Remove">
              <Trash2 class="size-4 text-muted-foreground" />
            </Button>
          </li>
        {/each}
      </ul>
    {/if}
  </div>

  <Separator />

  <!-- Scan action -->
  <div class="flex items-center gap-3">
    <Button onclick={() => library.startScan()} disabled={library.scanning || paths.length === 0}>
      {#if library.scanning}
        <Loader2 class="size-4 animate-spin" />
        Scanning…
      {:else}
        <ScanLine class="size-4" />
        Scan library
      {/if}
    </Button>
    <span class="text-xs text-muted-foreground">
      {#if paths.length === 0}
        Add a folder first
      {:else if paths.some((p) => p.accessible === false)}
        {paths.filter((p) => p.accessible === false).length} folder(s) offline — scan will skip them
      {:else}
        Indexes videos, thumbnails, and fingerprints — does not add StashDB tags
      {/if}
    </span>
  </div>

  {#if library.lastProgress}
    <div class="rounded-md border border-border bg-card/50 px-4 py-3 text-xs text-muted-foreground">
      <div class="flex items-center gap-2 font-medium text-foreground">
        Scan {library.lastProgress.status}
      </div>
      <div class="mt-1 grid grid-cols-2 gap-x-6 gap-y-0.5 sm:grid-cols-4">
        <span>Found: {library.lastProgress.files_found}</span>
        <span>Added: {library.lastProgress.files_added}</span>
        <span>Updated: {library.lastProgress.files_updated}</span>
        <span>Removed: {library.lastProgress.files_removed}</span>
      </div>
    </div>
  {/if}

  <Separator />

  <div class="space-y-3">
    {#if anyMediaJob}
      <div class="flex items-center gap-3">
        <Button variant="destructive" size="sm" onclick={stopAllMediaJobs}>
          <X class="size-3.5" />
          Stop thumbnail / hash jobs
        </Button>
        <span class="text-xs text-muted-foreground">
          Cancels previews, MD5, and pHash (including post-scan auto jobs). In-flight ffmpeg may finish the current file.
        </span>
      </div>
    {/if}
    <div class="flex flex-wrap items-center gap-3">
      <Button
        variant="outline"
        onclick={regenerateThumbnails}
        disabled={generatingPreviews}
      >
        {#if generatingPreviews}
          <Loader2 class="size-4 animate-spin" />
          Generating…
        {:else}
          <Image class="size-4" />
          Regenerate thumbnails
        {/if}
      </Button>
      {#if generatingPreviews}
        <Button variant="ghost" size="sm" onclick={stopPreviews}>Stop</Button>
      {/if}
      <span class="text-xs text-muted-foreground">
        Rebuilds missing grid thumbnails and scrub sprites (requires FFmpeg).
      </span>
    </div>
    {#if previewProgress && previewProgress.total > 0}
      <div class="rounded-md border border-border bg-card/50 px-4 py-3 text-xs text-muted-foreground">
        <div class="font-medium text-foreground">
          Thumbnails {previewProgress.cancelled
            ? "stopped"
            : generatingPreviews
              ? "generating"
              : "done"}
          — {previewProgress.done}/{previewProgress.total}
        </div>
        {#if previewProgress.current_path}
          <div class="mt-1 truncate">{previewProgress.current_path}</div>
        {/if}
      </div>
    {/if}
    <div class="flex flex-wrap items-center gap-3">
      <Button
        variant="outline"
        onclick={regenerateFingerprints}
        disabled={generatingFingerprints}
      >
        {#if generatingFingerprints}
          <Loader2 class="size-4 animate-spin" />
          Hashing…
        {:else}
          <RefreshCw class="size-4" />
          Compute MD5 fingerprints
        {/if}
      </Button>
      {#if generatingFingerprints}
        <Button variant="ghost" size="sm" onclick={stopMd5}>Stop</Button>
      {/if}
      <span class="text-xs text-muted-foreground">
        Full-file MD5 for StashDB matching (runs automatically after scan).
      </span>
    </div>
    {#if fingerprintProgress && fingerprintProgress.total > 0}
      <div class="rounded-md border border-border bg-card/50 px-4 py-3 text-xs text-muted-foreground">
        <div class="font-medium text-foreground">
          MD5 {fingerprintProgress.cancelled
            ? "stopped"
            : generatingFingerprints
              ? "computing"
              : "done"}
          — {fingerprintProgress.done}/{fingerprintProgress.total}
        </div>
        {#if fingerprintProgress.current_path}
          <div class="mt-1 truncate">{fingerprintProgress.current_path}</div>
        {/if}
      </div>
    {/if}
    <div class="flex flex-wrap items-center gap-3">
      <Button variant="outline" onclick={computeMissingPhash} disabled={generatingPhash}>
        {#if generatingPhash}
          <Loader2 class="size-4 animate-spin" />
          Hashing…
        {:else}
          <RefreshCw class="size-4" />
          Compute missing pHash
        {/if}
      </Button>
      <Button variant="ghost" size="sm" onclick={rebuildAllPhash} disabled={generatingPhash}>
        Rebuild all…
      </Button>
      {#if generatingPhash}
        <Button variant="ghost" size="sm" onclick={stopPhash}>Stop</Button>
      {/if}
      <span class="text-xs text-muted-foreground">
        StashDB-compatible 5×5 sprite pHash. Prefer “missing” — rebuild wipes saved hashes first.
      </span>
    </div>
    {#if phashProgress && phashProgress.total > 0}
      <div class="rounded-md border border-border bg-card/50 px-4 py-3 text-xs text-muted-foreground">
        <div class="font-medium text-foreground">
          pHash {phashProgress.cancelled
            ? "stopped"
            : generatingPhash
              ? "computing"
              : "done"}
          — {phashProgress.done}/{phashProgress.total}
        </div>
        {#if phashProgress.current_path}
          <div class="mt-1 truncate">{phashProgress.current_path}</div>
        {/if}
      </div>
    {/if}
  </div>

  <Separator />

  <div class="space-y-2" data-testid="about-section">
    <h2 class="text-sm font-medium">About</h2>
    <div class="flex flex-wrap items-center gap-3 rounded-md border border-border bg-card/50 px-4 py-3 text-sm">
      <span class="text-muted-foreground">MaizeView v{appVersion}</span>
      <Button size="sm" variant="outline" onclick={() => void checkUpdates()} disabled={checkingUpdates}>
        {#if checkingUpdates}
          <Loader2 class="size-3.5 animate-spin" /> Checking…
        {:else}
          Check for updates
        {/if}
      </Button>
      {#if updateResult}
        {#if updateResult.update_available}
          <button
            class="text-primary hover:underline"
            onclick={() => void openUrl(updateResult!.url)}
          >
            v{updateResult.latest} available →
          </button>
        {:else}
          <span class="text-muted-foreground">You're up to date.</span>
        {/if}
      {/if}
      {#if updateError}
        <span class="text-xs text-destructive">{updateError}</span>
      {/if}
    </div>
    <p class="text-xs text-muted-foreground">
      Only checks when you click — MaizeView makes no background network calls.
    </p>
  </div>
</section>
