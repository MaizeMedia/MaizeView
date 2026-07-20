<script lang="ts">
  // 4Play window: four videos, one per quadrant.
  //
  // FOUR mpv instances embed into the TOP-LEVEL window (wid = parent HWND —
  // embedding into a quadrant pane makes mpv a grandchild of the Tauri
  // window, and grandchildren never paint; progress.md gotcha 4). Each
  // instance's render window is claimed by diff (quad_claim_mpv) and fitted
  // over its quadrant; a Rust-side subclass keeps all four fitted through
  // move/resize.
  //
  // Instances use SYNTHETIC labels (`<windowLabel>-q<i>`): the plugin skips
  // its get_webview_window lookup when `wid` is already in initialOptions.
  // Synthetic labels receive no events, so observedProperties stays empty,
  // playback position/EOF are polled, and instances are destroyed manually
  // on close.
  //
  // Controls: click a pane to solo its audio (click again to mute all),
  // per-pane Prev/Next (play history), seek bar + time, volume slider on the
  // soloed pane, Space or the bar button toggles pause-all, bar ✕ closes
  // the window.
  //
  // Background transparency matters: like the single-scene player, this
  // window is transparent and the videos sit BELOW the webview in paint
  // order, so the grid cells must stay transparent for video to show through.

  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import { PhysicalSize } from "@tauri-apps/api/dpi";
  import {
    init as mpvInit,
    command as mpvCommand,
    setProperty as mpvSetProperty,
    getProperty as mpvGetProperty,
    destroy as mpvDestroy,
  } from "tauri-plugin-libmpv-api";
  import { Volume2, Play, Pause, X, SkipBack, SkipForward, Plus, Maximize2, Minimize2, Trash2, Loader2 } from "@lucide/svelte";
  import { confirm } from "@tauri-apps/plugin-dialog";
  import FavoriteButton from "$components/favorite-button.svelte";
  import {
    claimPlayerQueue,
    sceneFilePath,
    sceneScrubPreview,
    scenes,
    assetUrl,
    playerSettings,
    DEFAULT_MPV_CONFIG,
  } from "$lib/api";
  import {
    parseScrubberVtt,
    cueAtTime,
    spriteDimensions,
    scrubPreviewStyle,
    scrubPreviewDisplaySize,
    type ScrubberCue,
  } from "$lib/scrubber-preview";
  import { shuffleWeight, weightedPickId, type ShuffleMeta } from "$lib/shuffle";
  import { stringifyError, fmtTime } from "$lib/utils";

  const windowLabel = getCurrentWebviewWindow().label;
  const instanceLabel = (i: number) => `${windowLabel}-q${i}`;

  interface PaneState {
    status: "loading" | "playing" | "error";
    detail: string;
    timePos: number;
    duration: number;
    volume: number;
    sceneId: string;
    paused: boolean;
  }

  let panes = $state<(PaneState | null)[]>([null, null, null, null]);
  let soloIndex = $state<number | null>(null);
  let allPaused = $state(false);
  let isFullscreen = $state(false);
  let bootError = $state<string | null>(null);

  // The whole staged queue (the quad window plays 4 at a time and rotates
  // through the rest on EOF). Fresh scenes are drawn from drawBag: in staged
  // order, or weighted (ADR-010) when the source playlist has shuffle on.
  let stagedIds: string[] = [];
  let shuffleDefault = false;
  let shuffleMetaMap = new Map<string, ShuffleMeta>();
  let drawBag: string[] = [];
  let pollTimer: ReturnType<typeof setInterval> | null = null;
  const advancing = [false, false, false, false];
  // While a pane's seek slider is dragged, the poll must not move its thumb.
  const seeking = [false, false, false, false];
  // Per-pane play history for Prev/Next (histPos = currently playing entry).
  const playHistory: string[][] = [[], [], [], []];
  let histPos = $state([0, 0, 0, 0]);
  // Top-level HWND (mpv wid target), captured at boot for pane re-inits.
  let parentHwnd = 0;

  // Scrub previews (sprite sheet + VTT per scene, same as the player).
  let previewUrls = $state<(string | null)[]>([null, null, null, null]);
  let previewCues = $state<ScrubberCue[][]>([[], [], [], []]);
  let previewHover = $state<({ t: number; x: number } | null)[]>([null, null, null, null]);

  // Chrome auto-hide: visible on mouse activity, fades after 3 s idle or
  // when the cursor leaves the window (unless a seek drag is in flight).
  // The mouse cursor hides with it (standard player behavior).
  let controlsVisible = $state(true);
  // Reactive viewport width for the hover-preview sizing (same pattern as
  // the single player's windowWidth).
  let innerWidth = $state(1280);
  let idleTimer: ReturnType<typeof setTimeout> | null = null;
  function setChromeVisible(visible: boolean) {
    controlsVisible = visible;
    document.body.classList.toggle("cursor-hidden", !visible);
  }
  function pokeControls() {
    setChromeVisible(true);
    if (idleTimer) clearTimeout(idleTimer);
    idleTimer = setTimeout(() => {
      // Same policy as the single player: chrome stays up while paused.
      if (!seeking.some(Boolean) && !allPaused) setChromeVisible(false);
    }, 3000);
  }
  function hideControlsIfIdle() {
    if (!seeking.some(Boolean) && !allPaused) setChromeVisible(false);
  }

  let unlistenClose: (() => void) | null = null;
  let unlistenMoved: (() => void) | null = null;
  let unlistenResized: (() => void) | null = null;

  // Debounced native re-tile. The Rust subclass is the primary defense
  // against mpv's fit-to-full hook; this is belt-and-braces for geometry
  // changes it might miss. Also persists the window size (restored on open).
  let relayoutTimer: ReturnType<typeof setTimeout> | null = null;
  function onResize() {
    if (relayoutTimer) clearTimeout(relayoutTimer);
    relayoutTimer = setTimeout(() => {
      invoke("quad_relayout", { label: windowLabel }).catch((e) =>
        console.warn("quad_relayout failed", e),
      );
      saveWindowSize();
    }, 100);
  }

  const SIZE_KEY = "quadWindowSize";
  async function restoreWindowSize() {
    try {
      const raw = localStorage.getItem(SIZE_KEY);
      if (!raw) return;
      const { w, h } = JSON.parse(raw);
      if (typeof w === "number" && typeof h === "number" && w >= 640 && h >= 360) {
        await getCurrentWebviewWindow().setSize(new PhysicalSize(w, h));
      }
    } catch {
      // best effort only
    }
  }
  function saveWindowSize() {
    getCurrentWebviewWindow()
      .innerSize()
      .then((s) => localStorage.setItem(SIZE_KEY, JSON.stringify({ w: s.width, h: s.height })))
      .catch(() => {});
  }

  function weightFor(id: string): number {
    return shuffleWeight(shuffleMetaMap.get(id));
  }

  function refillBag() {
    drawBag = [...stagedIds];
  }

  // Draw the next scene for a pane: staged order, or weighted without
  // replacement (ADR-010) when the source playlist has shuffle-by-default.
  function drawNextSceneId(): string | null {
    if (drawBag.length === 0) refillBag();
    if (drawBag.length === 0) return null;
    if (!shuffleDefault) return drawBag.shift()!;
    const id = weightedPickId(drawBag, weightFor);
    drawBag = drawBag.filter((x) => x !== id);
    return id;
  }

  async function claimWithRetry(i: number): Promise<number> {
    // mpv creates its render window asynchronously on init — retry a beat.
    let lastErr: unknown = null;
    for (let attempt = 0; attempt < 10; attempt++) {
      try {
        return await invoke<number>("quad_claim_mpv", { label: windowLabel, index: i });
      } catch (e) {
        lastErr = e;
        await new Promise((r) => setTimeout(r, 200));
      }
    }
    throw lastErr;
  }

  // Load a scene into pane `i`'s instance and apply the pane's current audio
  // state. Must be explicit about pause: keep-open leaves pause=yes at EOF
  // and the property persists across loadfile (rotation loaded frozen
  // frames). Records the scene in the pane's history unless it's a history
  // replay (Prev/Next navigation).
  async function loadFileIntoPane(i: number, sceneId: string, recordHistory: boolean) {
    const label = instanceLabel(i);
    const volume = panes[i]?.volume ?? 50;
    const path = await sceneFilePath(sceneId);
    if (!path) throw new Error(`no playable file for scene ${sceneId}`);
    await mpvCommand("loadfile", [path], label);
    await mpvSetProperty("pause", allPaused, label);
    await mpvSetProperty("mute", soloIndex !== i, label);
    await mpvSetProperty("volume", volume, label);
    panes[i] = {
      status: "playing",
      detail: path.split(/[\\/]/).pop() ?? path,
      timePos: 0,
      duration: 0,
      volume,
      sceneId,
      paused: allPaused,
    };
    if (recordHistory) {
      // A fresh scene truncates any "future" entries left after a Prev.
      playHistory[i].length = histPos[i] + 1;
      playHistory[i].push(sceneId);
      histPos[i] = playHistory[i].length - 1;
    }
    void loadPreview(i, sceneId);
  }

  // Scrub preview (sprite + VTT) for the scene now playing in pane `i`.
  async function loadPreview(i: number, sceneId: string) {
    previewUrls[i] = null;
    previewCues[i] = [];
    try {
      const preview = await sceneScrubPreview(sceneId);
      if (!preview) return;
      const sprite = assetUrl(preview.sprite_path);
      if (!sprite) return;
      const cues = parseScrubberVtt(preview.vtt_text);
      if (cues.length === 0) return;
      previewUrls[i] = sprite;
      previewCues[i] = cues;
    } catch {
      // no preview generated for this scene — hover just shows the time
    }
  }

  async function startPane(i: number, sceneId: string, parentHwnd: number) {
    const label = instanceLabel(i);
    await mpvInit(
      {
        initialOptions: { ...DEFAULT_MPV_CONFIG.initialOptions, wid: parentHwnd },
        observedProperties: [],
      },
      label,
    );
    await loadFileIntoPane(i, sceneId, true);
    await claimWithRetry(i);
  }

  // Live seek: the thumb updates locally every input event, and the video
  // follows throttled (150 ms); final seek lands on release. The poll leaves
  // the thumb alone via the `seeking` flag.
  const liveSeekAt = [0, 0, 0, 0];
  function onSeekInput(i: number, v: number) {
    if (panes[i]) panes[i] = { ...panes[i]!, timePos: v };
    const now = Date.now();
    if (now - liveSeekAt[i] > 150) {
      liveSeekAt[i] = now;
      void mpvSetProperty("time-pos", v, instanceLabel(i)).catch(() => {});
    }
  }
  async function onSeekCommit(i: number, v: number) {
    seeking[i] = false;
    await mpvSetProperty("time-pos", v, instanceLabel(i)).catch(() => {});
  }

  // Hover position over a seek bar → preview bubble time + x (strip coords).
  function onSeekHover(i: number, e: PointerEvent) {
    const p = panes[i];
    if (!p || p.duration <= 0) return;
    const input = e.currentTarget as HTMLInputElement;
    const rect = input.getBoundingClientRect();
    const frac = Math.min(1, Math.max(0, (e.clientX - rect.left) / rect.width));
    previewHover[i] = { t: frac * p.duration, x: input.offsetLeft + frac * rect.width };
  }
  function onSeekLeave(i: number) {
    if (!seeking[i]) previewHover[i] = null;
  }

  async function onVolumeInput(i: number, v: number) {
    if (panes[i]) panes[i] = { ...panes[i]!, volume: v };
    await mpvSetProperty("volume", v, instanceLabel(i)).catch(() => {});
  }

  async function forEachPlaying(fn: (label: string, j: number) => Promise<void>) {
    await Promise.all(
      panes.map((p, j) =>
        p?.status === "playing" ? fn(instanceLabel(j), j).catch(() => {}) : Promise.resolve(),
      ),
    );
  }

  // Solo one pane's audio; clicking the soloed pane again mutes everything.
  async function toggleSolo(i: number) {
    if (panes[i]?.status !== "playing") return;
    soloIndex = soloIndex === i ? null : i;
    await forEachPlaying((label, j) => mpvSetProperty("mute", soloIndex !== j, label));
    // Bar controls target the soloed pane — refresh its favorite level.
    if (soloIndex != null) await refreshSoloFavorite();
  }

  // ─── Soloed-pane bar controls (favorite, volume, delete) ────────────────
  let soloFav = $state(0);
  let deleteEnabled = $state(false);
  let deleting = $state(false);

  async function refreshSoloFavorite() {
    const p = soloIndex != null ? panes[soloIndex] : null;
    if (!p) return;
    try {
      const [meta] = await scenes.shuffleMeta([p.sceneId]);
      soloFav = meta?.favorite ?? 0;
    } catch {
      soloFav = 0;
    }
  }

  async function setSoloFavorite(next: number) {
    const p = soloIndex != null ? panes[soloIndex] : null;
    if (!p) return;
    soloFav = next;
    await scenes.setFavorite(p.sceneId, next).catch(() => {});
  }

  /** Delete the soloed pane's scene (file + library entry), then refill the pane. */
  async function deleteSoloedScene() {
    const i = soloIndex;
    const p = i != null ? panes[i] : null;
    if (i == null || !p || p.status !== "playing" || deleting) return;
    const targetId = p.sceneId;
    const ok = await confirm(
      `Delete "${p.detail}" permanently?\n\nThis removes the video file from disk and from your library.`,
      { title: "Delete video", kind: "warning", okLabel: "Delete", cancelLabel: "Cancel" },
    );
    if (!ok) return;

    deleting = true;
    try {
      // Advance the pane first so its mpv releases the file handle (Windows
      // lock), then delete. No next scene → stop the instance instead.
      const hadNext = stagedIds.filter((id) => id !== targetId).length > 0;
      if (hadNext) await nextPane(i);
      else {
        await mpvCommand("stop", [], instanceLabel(i)).catch(() => {});
        panes[i] = null;
        // Unsoloing must unmute the remaining panes (toggleSolo muted them).
        await forEachPlaying((label) => mpvSetProperty("mute", false, label));
        soloIndex = null;
      }
      // The pane must actually be off the target file before deleting —
      // a failed advance (dead next file) leaves the lock in place.
      if (panes[i]?.sceneId === targetId) {
        throw new Error("player could not release the file — close the pane's video and try again");
      }
      // Windows may hold the lock briefly after mpv releases the file.
      await new Promise((r) => setTimeout(r, 300));
      await scenes.delete(targetId);
      // Drop the deleted scene from the rotation bag + every pane history.
      stagedIds = stagedIds.filter((id) => id !== targetId);
      drawBag = drawBag.filter((id) => id !== targetId);
      for (let j = 0; j < 4; j++) {
        const idx = playHistory[j].indexOf(targetId);
        if (idx >= 0) {
          playHistory[j].splice(idx, 1);
          if (histPos[j] >= idx) histPos[j] = Math.max(0, histPos[j] - 1);
        }
      }
      // The heart now points at the newly loaded scene — refresh its level.
      await refreshSoloFavorite();
    } catch (e) {
      bootError = `Delete failed: ${stringifyError(e)}`;
      setTimeout(() => (bootError = null), 4000);
    } finally {
      deleting = false;
    }
  }

  async function togglePauseAll() {
    allPaused = !allPaused;
    await forEachPlaying((label) => mpvSetProperty("pause", allPaused, label));
    // Paused keeps chrome up (single-player policy); resuming re-arms idle.
    pokeControls();
  }

  // Load the next drawn scene into pane `i` (EOF rotation, manual Next,
  // reload of an empty pane). Skips dead files; gives up only when every
  // remaining queued scene is unplayable.
  async function advancePane(i: number) {
    if (stagedIds.length === 0) return;
    for (let tries = 0; tries < stagedIds.length; tries++) {
      const sceneId = drawNextSceneId();
      if (!sceneId) break;
      try {
        if (panes[i] == null) await startPane(i, sceneId, parentHwnd);
        else await loadFileIntoPane(i, sceneId, true);
        return;
      } catch {
        // dead scene — try the next drawn one
      }
    }
    if (panes[i] != null) {
      panes[i] = { ...panes[i]!, status: "error", detail: "no more playable scenes in the queue" };
    }
  }

  // Manual Next: replay forward history if there is any (after a Prev),
  // otherwise rotate to a fresh staged scene.
  async function nextPane(i: number) {
    if (histPos[i] < playHistory[i].length - 1) {
      histPos[i] += 1;
      try {
        await loadFileIntoPane(i, playHistory[i][histPos[i]], false);
      } catch {
        histPos[i] -= 1; // file died — stay put
      }
      return;
    }
    await advancePane(i);
  }

  async function prevPane(i: number) {
    if (histPos[i] <= 0 || panes[i]?.status !== "playing") return;
    histPos[i] -= 1;
    try {
      await loadFileIntoPane(i, playHistory[i][histPos[i]], false);
    } catch {
      histPos[i] += 1; // couldn't play it — stay put
    }
  }

  // Synthetic labels receive no mpv events (the plugin emits per window
  // label), so playback position + EOF are polled per instance.
  async function pollPanes() {
    await Promise.all(
      panes.map(async (p, i) => {
        if (p?.status !== "playing" || advancing[i]) return;
        advancing[i] = true;
        try {
          const [timePos, duration, eof, paused] = await Promise.all([
            mpvGetProperty<number>("time-pos", "double", instanceLabel(i)),
            mpvGetProperty<number>("duration", "double", instanceLabel(i)),
            mpvGetProperty<boolean>("eof-reached", "flag", instanceLabel(i)),
            mpvGetProperty<boolean>("pause", "flag", instanceLabel(i)),
          ]);
          // Don't move the thumb while the user drags this pane's slider.
          const next = { ...panes[i]! };
          if (!seeking[i] && timePos != null && duration != null) {
            next.timePos = timePos;
            next.duration = duration;
          }
          if (paused != null) next.paused = paused;
          panes[i] = next;
          // EOF rotation skips paused panes (per-pane or pause-all).
          if (eof === true && !allPaused && !next.paused) await advancePane(i);
        } catch {
          // instance gone — ignore
        } finally {
          advancing[i] = false;
        }
      }),
    );
  }

  // Per-pane play/pause (the bar's Pause all still applies to everything).
  async function togglePanePause(i: number) {
    const p = panes[i];
    if (!p || p.status !== "playing") return;
    const next = !p.paused;
    panes[i] = { ...p, paused: next };
    await mpvSetProperty("pause", next, instanceLabel(i)).catch(() => {});
    pokeControls();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "F11") {
      e.preventDefault();
      void toggleFullscreen();
      return;
    }
    if (e.key === "Escape" && isFullscreen) {
      e.preventDefault();
      void toggleFullscreen();
      return;
    }
    if (e.code === "Space") {
      e.preventDefault();
      void togglePauseAll();
    }
  }

  // Same pattern as the single player: read the real state, flip it.
  async function toggleFullscreen() {
    try {
      const win = getCurrentWebviewWindow();
      const next = !(await win.isFullscreen());
      await win.setFullscreen(next);
      isFullscreen = next;
    } catch (e) {
      console.warn("fullscreen failed", e);
    }
  }

  // Destroy all four mpv instances and wait for their VO threads to exit —
  // the top-level window must outlive them: letting the window destroy race
  // mpv teardown rendezvous-deadlocks the UI thread (cross-thread
  // DestroyWindow against dying VO threads). A per-instance timeout keeps a
  // wedged instance from blocking the close forever.
  async function destroyInstances() {
    await Promise.all(
      [0, 1, 2, 3].map((i) =>
        Promise.race([
          mpvDestroy(instanceLabel(i)).catch(() => {}),
          new Promise((r) => setTimeout(r, 3000)),
        ]),
      ),
    );
  }

  async function closeWindow() {
    await destroyInstances();
    await getCurrentWebviewWindow().destroy();
  }

  onMount(async () => {
    window.addEventListener("resize", onResize);
    window.addEventListener("keydown", onKeydown);
    window.addEventListener("mousemove", pokeControls);
    document.documentElement.addEventListener("mouseleave", hideControlsIfIdle);
    pokeControls();
    const thisWin = getCurrentWebviewWindow();
    isFullscreen = await thisWin.isFullscreen().catch(() => false);
    unlistenMoved = await thisWin.onMoved(() => onResize());
    unlistenResized = await thisWin.onResized(() => onResize());
    // The plugin only auto-destroys mpv instances keyed by a REAL window
    // label; ours are synthetic, so clean them up ourselves on close —
    // awaited, so the window outlives the VO threads (see destroyInstances).
    unlistenClose = await thisWin.onCloseRequested(async () => {
      await destroyInstances();
    });

    try {
      // 0. Reapply the last quad window size (before panes measure it).
      await restoreWindowSize();
      // Delete-in-player is opt-in (same setting as the single player).
      deleteEnabled =
        (await playerSettings.get().catch(() => null))?.delete_in_player_enabled ?? false;

      // 1. Claim the staged queue → the whole rotation list (plays 4 at a
      //    time; EOF advances each pane through the rest). Honor the source
      //    playlist's shuffle-by-default: rotation draws weighted (ADR-010).
      const staged = await claimPlayerQueue(windowLabel);
      stagedIds = staged?.scene_ids ?? [];
      if (stagedIds.length === 0) {
        throw new Error("no staged queue for this window — open 4Play from a playlist");
      }
      shuffleDefault = staged?.shuffle_by_default ?? false;
      if (shuffleDefault) {
        try {
          const meta = await scenes.shuffleMeta(stagedIds);
          shuffleMetaMap = new Map(
            meta.map((m) => [m.id, { favorite: m.favorite, lastPlayedAt: m.last_played_at }]),
          );
        } catch (e) {
          console.warn("shuffle meta unavailable, using flat weights", e);
        }
      }
      refillBag();
      const sceneIds = [0, 1, 2, 3]
        .map(() => drawNextSceneId())
        .filter((x): x is string => x != null);

      // 2. Create the 4 quadrant child HWNDs. The top-level window's own
      //    HWND comes back as `parent` — THAT is what every mpv instance
      //    embeds into (pane embedding = grandchild mpv = nothing paints).
      const { parent } = await invoke<{ panes: number[]; parent: number }>(
        "quad_create_panes",
        { label: windowLabel },
      );
      parentHwnd = parent;

      // 3. Sequential init → claim per quadrant: each claim's diff
      //    identifies the render window the just-initialized instance made.
      //    Reuse the single-player mpv options; NO osd_level (deadlocks
      //    init), no observed properties (synthetic labels get no events).
      for (let i = 0; i < sceneIds.length; i++) {
        panes[i] = {
          status: "loading",
          detail: "",
          timePos: 0,
          duration: 0,
          volume: 50,
          sceneId: "",
          paused: false,
        };
        try {
          await startPane(i, sceneIds[i], parent);
        } catch (e) {
          panes[i] = {
            status: "error",
            detail: stringifyError(e),
            timePos: 0,
            duration: 0,
            volume: 50,
            sceneId: "",
            paused: false,
          };
        }
      }

      // 4. Poll playback position + EOF per instance (no mpv events on
      //    synthetic labels); EOF advances the pane to the next staged scene.
      pollTimer = setInterval(() => void pollPanes(), 1000);
    } catch (e) {
      bootError = stringifyError(e);
    }
  });

  onDestroy(() => {
    window.removeEventListener("resize", onResize);
    window.removeEventListener("keydown", onKeydown);
    window.removeEventListener("mousemove", pokeControls);
    document.documentElement.removeEventListener("mouseleave", hideControlsIfIdle);
    if (relayoutTimer) clearTimeout(relayoutTimer);
    if (pollTimer) clearInterval(pollTimer);
    if (idleTimer) clearTimeout(idleTimer);
    unlistenClose?.();
    unlistenMoved?.();
    unlistenResized?.();
  });
</script>

<!-- 2x2 grid of cells (click = solo). Cells with a live pane must stay
     background-TRANSPARENT (the video paints below the webview); empty
     panes (fewer than 4 staged) get an opaque fill so they don't show the
     desktop through the transparent window. Nested inputs stop propagation
     so slider drags don't toggle solo. -->
<svelte:window bind:innerWidth />

<div class="fixed inset-0 grid grid-cols-2 grid-rows-2">
  {#each [0, 1, 2, 3] as i}
    <div
      class="relative border {panes[i] == null
        ? 'border-solid border-zinc-700 bg-zinc-900/90'
        : `border-solid ${soloIndex === i ? 'border-lime-400' : 'border-zinc-500/60'}`}"
      role="button"
      tabindex="0"
      onclick={() => void toggleSolo(i)}
      onkeydown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          e.stopPropagation();
          void toggleSolo(i);
        }
      }}
    >
      {#if panes[i] == null}
        <button
          class="absolute left-1/2 top-1/2 flex -translate-x-1/2 -translate-y-1/2 items-center gap-1 rounded bg-black/60 px-2 py-1 text-xs text-zinc-300 hover:text-white"
          aria-label="Load next scene into Q{i + 1}"
          onclick={(e) => {
            e.stopPropagation();
            void advancePane(i);
          }}
        >
          <Plus class="size-3.5" /> Load next
        </button>
      {/if}

      {#if panes[i]?.status === "loading"}
        <span class="absolute left-2 top-10 rounded bg-black/60 px-2 py-1 text-xs text-zinc-300">
          loading…
        </span>
      {:else if panes[i]?.status === "error"}
        <span class="absolute inset-x-2 top-10 rounded bg-red-900/80 px-2 py-1 text-xs text-red-100">
          {panes[i]?.detail}
        </span>
      {:else if panes[i]?.status === "playing"}
        <!-- Per-pane transport. Bottom row lifts the strip above the
             window-level control bar. Fades with idle like the bar. -->
        <div
          class="absolute inset-x-2 {i >= 2 ? 'bottom-12' : 'bottom-2'} flex items-center gap-1.5 transition-opacity duration-300 {controlsVisible
            ? 'opacity-100'
            : 'pointer-events-none opacity-0'}"
          role="presentation"
          onclick={(e) => e.stopPropagation()}
        >
          {#if previewHover[i] && previewUrls[i] && previewCues[i].length}
            {@const cue = cueAtTime(previewCues[i], previewHover[i]!.t)}
            {#if cue}
              {@const dims = spriteDimensions(previewCues[i])}
              {@const size = scrubPreviewDisplaySize(Math.round(innerWidth / 2))}
              <div
                class="pointer-events-none absolute bottom-7 z-20 rounded border border-zinc-700 shadow-lg"
                style="left: max(0px, min(calc(100% - {size.width + 8}px), {previewHover[i]!.x -
                  size.width / 2}px)); {scrubPreviewStyle(
                  previewUrls[i]!,
                  cue,
                  dims.width,
                  dims.height,
                  size.width,
                  size.height,
                )}"
              >
                <span
                  class="absolute inset-x-0 bottom-0 bg-black/70 text-center font-mono text-[10px] text-zinc-200"
                >
                  {fmtTime(previewHover[i]!.t)}
                </span>
              </div>
            {/if}
          {/if}
          <button
            class="rounded-full bg-black/60 p-1.5 text-white transition hover:bg-white/15"
            title={panes[i]!.paused ? "Play" : "Pause"}
            aria-label="{panes[i]!.paused ? 'Play' : 'Pause'} Q{i + 1}"
            onclick={() => void togglePanePause(i)}
          >
            {#if panes[i]!.paused}
              <Play class="size-3.5" />
            {:else}
              <Pause class="size-3.5" />
            {/if}
          </button>
          <button
            class="rounded-full bg-black/60 p-1.5 text-white transition hover:bg-white/15 disabled:opacity-30 disabled:hover:bg-transparent"
            title="Previous scene"
            aria-label="Previous scene Q{i + 1}"
            disabled={histPos[i] === 0}
            onclick={() => void prevPane(i)}
          >
            <SkipBack class="size-3.5" />
          </button>
          <input
            type="range"
            min="0"
            max={Math.max(panes[i]!.duration, 1)}
            step="0.1"
            class="maize-scrubber h-1.5 min-w-0 flex-1 cursor-pointer appearance-none rounded-full bg-white/25"
            style="background: linear-gradient(to right, hsl(var(--primary)) {Math.min(
                1,
                panes[i]!.timePos / Math.max(panes[i]!.duration, 1),
              ) * 100}%, rgba(255,255,255,0.25) {Math.min(
                1,
                panes[i]!.timePos / Math.max(panes[i]!.duration, 1),
              ) * 100}%);"
            value={panes[i]!.timePos}
            aria-label="Seek Q{i + 1}"
            onpointerdown={() => (seeking[i] = true)}
            onpointermove={(e) => onSeekHover(i, e)}
            onpointerleave={() => onSeekLeave(i)}
            oninput={(e) => onSeekInput(i, Number(e.currentTarget.value))}
            onchange={(e) => void onSeekCommit(i, Number(e.currentTarget.value))}
          />
          <button
            class="rounded-full bg-black/60 p-1.5 text-white transition hover:bg-white/15"
            title="Next scene"
            aria-label="Next scene Q{i + 1}"
            onclick={() => void nextPane(i)}
          >
            <SkipForward class="size-3.5" />
          </button>
          <span class="shrink-0 rounded bg-black/60 px-1.5 py-0.5 font-mono text-[10px] text-zinc-200">
            {fmtTime(panes[i]!.timePos)} / {fmtTime(panes[i]!.duration)}
          </span>
        </div>
      {/if}
    </div>
  {/each}
</div>

<!-- Control bar: fades with idle (same auto-hide as pane chrome). -->
<div
  class="fixed inset-x-0 bottom-0 z-50 flex items-center gap-3 border-t border-zinc-800 bg-zinc-950/80 px-4 py-2 font-mono text-sm font-bold text-zinc-200 backdrop-blur-sm transition-opacity duration-300 {controlsVisible
    ? 'opacity-100'
    : 'pointer-events-none opacity-0'}"
>
  <span>4PLAY</span>
  <button
    class="flex items-center gap-1 rounded bg-zinc-800/80 px-2 py-1 hover:bg-zinc-700"
    onclick={() => void togglePauseAll()}
  >
    {#if allPaused}
      <Play class="size-4" /> Resume all
    {:else}
      <Pause class="size-4" /> Pause all
    {/if}
  </button>
  <button
    class="flex items-center gap-1 rounded bg-zinc-800/80 px-2 py-1 hover:bg-zinc-700"
    title="Advance all four panes to fresh scenes"
    aria-label="Next scene on all panes"
    onclick={() => {
      for (let i = 0; i < 4; i++) void nextPane(i);
    }}
  >
    <SkipForward class="size-4" /> Next all
  </button>
  {#if soloIndex != null && panes[soloIndex]?.status === "playing"}
    {@const solo = panes[soloIndex]!}
    {@const si = soloIndex}
    <span class="flex items-center gap-1.5 rounded bg-zinc-800/80 px-2 py-1" title="Soloed pane controls (Q{soloIndex + 1})">
      <FavoriteButton level={soloFav} onChange={setSoloFavorite} size="sm" variant="overlay" />
    </span>
    <span class="flex items-center gap-1.5 rounded bg-zinc-800/80 px-2 py-1" title="Solo volume">
      <Volume2 class="size-4" />
      <input
        type="range"
        min="0"
        max="100"
        step="1"
        class="maize-scrubber h-1.5 w-24 cursor-pointer appearance-none rounded-full bg-white/25"
        value={solo.volume}
        aria-label="Solo volume"
        oninput={(e) => void onVolumeInput(si, Number(e.currentTarget.value))}
      />
    </span>
    {#if deleteEnabled}
      <button
        class="flex items-center gap-1 rounded bg-zinc-800/80 px-2 py-1 text-red-400 hover:bg-zinc-700 disabled:opacity-40"
        title="Delete the soloed pane's video permanently"
        aria-label="Delete video Q{soloIndex + 1}"
        disabled={deleting}
        onclick={() => void deleteSoloedScene()}
      >
        {#if deleting}
          <Loader2 class="size-4 animate-spin" />
        {:else}
          <Trash2 class="size-4" />
        {/if}
        Delete
      </button>
    {/if}
  {/if}
  <button
    class="flex items-center gap-1 rounded bg-zinc-800/80 px-2 py-1 hover:bg-zinc-700"
    title={isFullscreen ? "Exit fullscreen (F11 / Esc)" : "Fullscreen (F11)"}
    aria-label={isFullscreen ? "Exit fullscreen" : "Fullscreen"}
    onclick={() => void toggleFullscreen()}
  >
    {#if isFullscreen}
      <Minimize2 class="size-4" /> Exit
    {:else}
      <Maximize2 class="size-4" /> Fullscreen
    {/if}
  </button>
  <button
    class="flex items-center gap-1 rounded bg-zinc-800/80 px-2 py-1 hover:bg-zinc-700"
    onclick={() => void closeWindow()}
  >
    <X class="size-4" /> Close
  </button>
  <span class="truncate font-normal opacity-90">
    {#if bootError}
      error — {bootError}
    {:else}
      {panes.map((p, i) => (p ? `Q${i + 1}:${p.status}` : null)).filter(Boolean).join("  ")}
    {/if}
  </span>
</div>

<style>
  /* Same as the player window: the webview body must be transparent so mpv's
     video (painting underneath the webview) shows through. app.css sets an
     opaque near-black body bg; override for the quad window. */
  :global(body) {
    background: transparent !important;
  }
  /* Hide the cursor when the chrome auto-hides (standard player behavior). */
  :global(body.cursor-hidden),
  :global(body.cursor-hidden *) {
    cursor: none !important;
  }
</style>
