<script lang="ts">
  // Player window (Phase 3, ADR-005 / ADR-012).
  //
  // Boots a libmpv instance in this native OS window, loads the file passed in
  // the URL, and draws a custom overlay (play/pause, scrubber, time, volume,
  // favorite button) on top of the video. The overlay talks to libmpv through
  // our plugin-agnostic $lib/api/player wrapper — never to the plugin directly.
  //
  // The window was created transparent (see openPlayerWindow); this component
  // keeps its own root background transparent so mpv paints underneath and the
  // overlay floats on top.

  import { onMount, onDestroy } from "svelte";
  import { confirm } from "@tauri-apps/plugin-dialog";
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import { Play, Pause, Volume2, VolumeX, Loader2, AlertTriangle, SkipForward, SkipBack, Shuffle, Trash2, ListPlus, Plus, Bookmark, BookmarkPlus, X, Maximize2, Minimize2 } from "@lucide/svelte";
  import FavoriteButton from "$components/favorite-button.svelte";
  import { cycleFavoriteLevel } from "$lib/favorite";
  import { stringifyError, fmtTime } from "$lib/utils";
  import {
    sessionCooldownSize,
    shuffleWeight,
    weightedPickId,
    type ShuffleMeta,
  } from "$lib/shuffle";
  import {
    createPlayer,
    scenes,
    segments as segmentsApi,
    claimPlayerQueue,
    sceneFilePath,
    sceneScrubPreview,
    playerSettings,
    assetUrl,
    playlists as playlistsApi,
    type PlaylistRow,
    type PlayerHandle,
    type PlayerObservedEvent,
    type PlayerEvent,
    type StagedQueue,
    type SceneSegment,
  } from "$lib/api";
  import {
    cueAtTime,
    parseScrubberVtt,
    scrubPreviewDisplaySize,
    scrubPreviewStyle,
    spriteDimensions,
    type ScrubberCue,
  } from "$lib/scrubber-preview";

  // ─── params from the catalog window ────────────────────────────────────
  function param(name: string): string | null {
    return new URLSearchParams(window.location.search).get(name);
  }
  // Must match the real Tauri window label (includes timestamp when forceNewWindow).
  // Reconstructing `player-${sceneId}` misses the staged queue for playlist Play.
  const windowLabel = getCurrentWebviewWindow().label;
  const initialSceneId = param("sceneId") ?? "";
  const initialFilePath = param("file") ?? "";
  const hasQueue = param("hasQueue") === "1";

  // Small helpers for resilient error display + hung-promise timeouts.
  function withTimeout<T>(p: Promise<T>, ms: number, msg: string): Promise<T> {
    return new Promise((resolve, reject) => {
      const t = setTimeout(() => reject(new Error(msg)), ms);
      p.then(
        (v) => { clearTimeout(t); resolve(v); },
        (e) => { clearTimeout(t); reject(e); },
      );
    });
  }
  function delay(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms));
  }

  // ─── lifecycle / libmpv handle ──────────────────────────────────────────
  let player: PlayerHandle | null = $state(null);
  let phase: "loading" | "ready" | "error" = $state("loading");
  let errorMsg = $state<string | null>(null);

  // ─── observed mpv state (mirrors PLAYER_OBSERVED_PROPERTIES) ────────────
  let paused = $state(true);
  let timePos = $state<number | null>(null); // seconds
  let duration = $state<number | null>(null); // seconds
  let volume = $state(75); // 0..100 — restored from settings on init
  let muted = $state(false);
  /** Skip persisting while applying saved volume on startup. */
  let volumePersistReady = $state(false);
  let saveVolumeTimer: ReturnType<typeof setTimeout> | null = null;
  let filename = $state<string | null>(null);
  let hwdec = $state<string | null>(null); // active backend from hwdec-current
  let eofReached = $state(false);

  // ─── overlay UI state ───────────────────────────────────────────────────
  let controlsVisible = $state(true);
  let hideTimer: ReturnType<typeof setTimeout> | null = null;
  let seeking = $state(false); // suppress time-pos updates while user drags scrubber
  let scrubValue = $state(0); // scrubber position while dragging
  let lastLiveSeekAt = 0; // throttle for live seek-while-dragging
  let scrubHoverTime = $state<number | null>(null);
  let scrubHoverX = $state(0); // px from left of scrub track
  let scrubPreviewSpriteUrl = $state<string | null>(null);
  let scrubPreviewCues = $state<ScrubberCue[]>([]);
  let windowWidth = $state(typeof window !== "undefined" ? window.innerWidth : 960);

  const scrubPreviewTime = $derived(seeking ? scrubValue : scrubHoverTime);
  const scrubPreviewCue = $derived(
    scrubPreviewTime != null ? cueAtTime(scrubPreviewCues, scrubPreviewTime) : null,
  );
  const scrubPreviewDims = $derived(spriteDimensions(scrubPreviewCues));
  const scrubPreviewSize = $derived(scrubPreviewDisplaySize(windowWidth));
  const scrubPreviewStyles = $derived(
    scrubPreviewSpriteUrl && scrubPreviewCue
      ? scrubPreviewStyle(
          scrubPreviewSpriteUrl,
          scrubPreviewCue,
          scrubPreviewDims.width,
          scrubPreviewDims.height,
          scrubPreviewSize.width,
          scrubPreviewSize.height,
        )
      : null,
  );
  const scrubPreviewVisible = $derived(
    scrubPreviewStyles != null && duration != null && duration > 0,
  );

  // Favorite (read+write via the scenes API; same 0..5 scale as the catalog).
  let favorite = $state(0);

  // ─── queue state (ADR-011: each window owns its queue + shuffle) ────────
  // The ordered list of scene IDs in this window's play queue. Null while
  // we're still claiming it from the Rust stash (or if single-scene mode).
  let queue = $state<string[]>([initialSceneId]);
  let queueIndex = $state(0);
  let currentSceneId = $state(initialSceneId);
  // Per-scene favorite + last_played cache for shuffle weights (ADR-010).
  let shuffleMetaByScene = $state<Record<string, ShuffleMeta>>({});
  // Recent plays this window — stay cool after a full-queue pass resets.
  let sessionRecent = $state<string[]>([]);
  // Scenes we've already played under shuffle (so we don't repeat until
  // the queue is exhausted, mirroring "without replacement" from ADR-010).
  let shufflePlayed = $state<Set<string>>(new Set());
  // Back-stack for Prev while shuffle is on (linear mode uses queueIndex).
  let shuffleHistory = $state<string[]>([]);
  let shuffleOn = $state(false);
  // Suppress the EOF→next auto-advance when the user explicitly paused or
  // is mid-navigation. (keep-open means EOF sets eof-reached, not close.)
  let advancing = $state(false);
  let deleteEnabled = $state(false);
  let deleting = $state(false);
  let isFullscreen = $state(false);

  let playlistPanelOpen = $state(false);
  let allPlaylists = $state<PlaylistRow[]>([]);
  let playlistToast = $state<string | null>(null);
  let newPlaylistName = $state("");
  let showNewPlaylistField = $state(false);

  // Timed segments / bookmarks
  let sceneSegments = $state<SceneSegment[]>([]);
  let segmentMarkIn = $state<number | null>(null);
  let segmentsPanelOpen = $state(false);

  async function loadSegments(sceneId: string) {
    try {
      sceneSegments = await segmentsApi.list(sceneId);
    } catch (e) {
      console.warn("list segments failed", e);
      sceneSegments = [];
    }
  }

  function segmentPct(sec: number): number {
    if (!duration || duration <= 0) return 0;
    return Math.min(100, Math.max(0, (sec / duration) * 100));
  }

  async function seekToSegment(seg: SceneSegment) {
    await player?.seek(seg.start_sec, "absolute");
    pokeControls();
  }

  function markSegmentIn() {
    segmentMarkIn = effectiveTime;
    pokeControls();
  }

  async function saveSegmentAtOut() {
    const start = segmentMarkIn ?? effectiveTime;
    const end = segmentMarkIn != null ? effectiveTime : null;
    const startSec = Math.min(start, end ?? start);
    const endSec =
      end != null && Math.abs(end - start) >= 0.25 ? Math.max(start, end) : null;
    const label = window.prompt(
      endSec != null ? "Segment label (optional)" : "Bookmark label (optional)",
      "",
    );
    if (label === null) {
      // cancelled
      return;
    }
    try {
      const row = await segmentsApi.create({
        scene_id: currentSceneId,
        start_sec: startSec,
        end_sec: endSec,
        label: label.trim() || (endSec != null ? "Segment" : "Bookmark"),
      });
      sceneSegments = [...sceneSegments, row].sort((a, b) => a.start_sec - b.start_sec);
      segmentMarkIn = null;
      flashPlaylistToast(endSec != null ? "Segment saved" : "Bookmark saved");
    } catch (e) {
      console.warn("create segment failed", e);
    }
    pokeControls();
  }

  async function deleteSegment(id: string) {
    try {
      await segmentsApi.delete(id);
      sceneSegments = sceneSegments.filter((s) => s.id !== id);
    } catch (e) {
      console.warn("delete segment failed", e);
    }
    pokeControls();
  }

  function toggleSegmentsPanel() {
    segmentsPanelOpen = !segmentsPanelOpen;
    if (segmentsPanelOpen) playlistPanelOpen = false;
    pokeControls();
  }

  async function loadPlaylists() {
    try {
      allPlaylists = await playlistsApi.list();
    } catch (e) {
      console.warn("list playlists failed", e);
    }
  }

  let playlistToastTimer: ReturnType<typeof setTimeout> | null = null;
  function flashPlaylistToast(msg: string) {
    playlistToast = msg;
    // Re-arm on repeat flashes so rapid toasts (e.g. a dead-file skip chain)
    // don't get cleared early by a previous flash's timer.
    if (playlistToastTimer) clearTimeout(playlistToastTimer);
    playlistToastTimer = setTimeout(() => {
      playlistToast = null;
      playlistToastTimer = null;
    }, 2500);
  }

  async function togglePlaylistPanel() {
    playlistPanelOpen = !playlistPanelOpen;
    if (playlistPanelOpen) {
      segmentsPanelOpen = false;
      await loadPlaylists();
    }
    pokeControls();
  }

  async function addCurrentToPlaylist(playlistId: string, playlistName: string) {
    try {
      const inserted = await playlistsApi.add(playlistId, currentSceneId);
      flashPlaylistToast(inserted ? `Added to “${playlistName}”` : `Already in “${playlistName}”`);
      await loadPlaylists();
    } catch (e) {
      console.warn("add to playlist failed", e);
    }
  }

  async function createPlaylistAndAddCurrent() {
    if (!newPlaylistName.trim()) return;
    try {
      const row = await playlistsApi.create(newPlaylistName.trim());
      await playlistsApi.add(row.id, currentSceneId);
      flashPlaylistToast(`Created “${row.name}” and added scene`);
      newPlaylistName = "";
      showNewPlaylistField = false;
      await loadPlaylists();
    } catch (e) {
      console.warn("create playlist failed", e);
    }
  }

  const hasQueueNav = $derived(queue.length > 1);
  const hasPrev = $derived(
    hasQueueNav && (shuffleOn ? shuffleHistory.length > 0 : queueIndex > 0),
  );
  // Shuffle wraps forever (new weighted pass after exhaustion).
  const hasNext = $derived(
    hasQueueNav && (shuffleOn ? true : queueIndex < queue.length - 1),
  );

  // ─── derived ────────────────────────────────────────────────────────────
  const effectiveTime = $derived(seeking ? scrubValue : (timePos ?? 0));
  const progress = $derived(
    duration && duration > 0 ? Math.min(1, effectiveTime / duration) : 0,
  );

  // ─── overlay auto-hide ──────────────────────────────────────────────────
  // Transparent WebView2 passes events through fully-clear pixels to mpv, so
  // we keep a near-invisible hit layer when chrome is hidden. Hide is driven
  // by an idle timer; don't trust continuous window mousemove (synthetic
  // events after layout keep chrome stuck on).
  const CONTROLS_IDLE_MS = 2000;
  let hideIgnoreUntil = 0;
  let lastPointerX = Number.NaN;
  let lastPointerY = Number.NaN;

  function showControls() {
    controlsVisible = true;
    scheduleHideControls();
  }

  function scheduleHideControls() {
    if (hideTimer) clearTimeout(hideTimer);
    hideTimer = setTimeout(() => {
      if (paused || phase !== "ready") return;
      controlsVisible = false;
      segmentsPanelOpen = false;
      playlistPanelOpen = false;
      hideIgnoreUntil = Date.now() + 400;
      lastPointerX = Number.NaN;
      lastPointerY = Number.NaN;
    }, CONTROLS_IDLE_MS);
  }

  function onPointerActivity(e: PointerEvent) {
    if (Date.now() < hideIgnoreUntil) return;
    const x = e.clientX;
    const y = e.clientY;
    if (Number.isFinite(lastPointerX)) {
      const dx = Math.abs(x - lastPointerX);
      const dy = Math.abs(y - lastPointerY);
      // Require a real move — ignores jitter / synthetic 0-delta events.
      if (dx < 3 && dy < 3) return;
    }
    lastPointerX = x;
    lastPointerY = y;
    showControls();
  }

  /** Legacy name used by control handlers. */
  function pokeControls() {
    showControls();
  }

  function closeFloatingPanels() {
    segmentsPanelOpen = false;
    playlistPanelOpen = false;
  }

  // ─── playback control handlers ──────────────────────────────────────────
  async function togglePlay() {
    await player?.togglePause();
    pokeControls();
  }

  function onScrubStart(e: Event) {
    seeking = true;
    scrubValue = effectiveTime;
    const input = e.currentTarget as HTMLInputElement;
    if (e instanceof PointerEvent || e instanceof MouseEvent) {
      updateScrubHoverFromClientX(input, (e as MouseEvent).clientX);
    }
    pokeControls();
  }
  function onScrubInput(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    scrubValue = Number(input.value);
    // Live seek: video follows the drag (throttled 150 ms; the exact seek
    // lands on commit, same as the 4Play per-pane scrubbers).
    const now = Date.now();
    if (now - lastLiveSeekAt >= 150) {
      lastLiveSeekAt = now;
      void player?.seek(scrubValue, "absolute").catch(() => {});
    }
    // Keep preview centered on the thumb while dragging.
    if (duration && duration > 0) {
      const rect = input.getBoundingClientRect();
      scrubHoverX = (scrubValue / duration) * rect.width;
      scrubHoverTime = scrubValue;
    }
  }
  function scrubTimeFromPointer(input: HTMLInputElement, clientX: number): number {
    const max = Number(input.max) || 0;
    if (max <= 0) return 0;
    const rect = input.getBoundingClientRect();
    const ratio = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width));
    return ratio * max;
  }
  function updateScrubHoverFromClientX(input: HTMLInputElement, clientX: number) {
    if (duration == null || duration <= 0 || !scrubPreviewCues.length) return;
    scrubHoverTime = scrubTimeFromPointer(input, clientX);
    const rect = input.getBoundingClientRect();
    scrubHoverX = Math.min(rect.width, Math.max(0, clientX - rect.left));
  }
  function onScrubPointerMove(e: PointerEvent) {
    updateScrubHoverFromClientX(e.currentTarget as HTMLInputElement, e.clientX);
  }
  function onScrubTrackMove(e: MouseEvent) {
    const track = e.currentTarget as HTMLElement;
    const input = track.querySelector("input[type=range]") as HTMLInputElement | null;
    if (!input) return;
    updateScrubHoverFromClientX(input, e.clientX);
  }
  function onScrubPointerLeave() {
    if (!seeking) scrubHoverTime = null;
  }
  async function loadScrubPreviewForScene(sceneId: string) {
    scrubPreviewSpriteUrl = null;
    scrubPreviewCues = [];
    try {
      const preview = await sceneScrubPreview(sceneId);
      if (!preview) return;
      const sprite = assetUrl(preview.sprite_path);
      if (!sprite) return;
      const cues = parseScrubberVtt(preview.vtt_text);
      if (cues.length === 0) return;
      scrubPreviewSpriteUrl = sprite;
      scrubPreviewCues = cues;
    } catch (e) {
      console.warn("scrub preview unavailable", e);
    }
  }
  async function onScrubCommit() {
    if (!player || duration == null) return;
    await player.seek(scrubValue, "absolute");
    seeking = false;
    scrubHoverTime = null;
    pokeControls();
  }

  function onVolumeInput(e: Event) {
    const v = Number((e.currentTarget as HTMLInputElement).value);
    void player?.setVolume(v);
    persistVolume(v, muted);
  }
  async function toggleMute() {
    await player?.toggleMute();
    persistVolume(volume, !muted);
    pokeControls();
  }

  function persistVolume(v: number, m: boolean) {
    if (!volumePersistReady) return;
    if (saveVolumeTimer) clearTimeout(saveVolumeTimer);
    saveVolumeTimer = setTimeout(() => {
      void playerSettings.set(v, m).catch((e) => console.warn("save volume failed", e));
    }, 250);
  }

  async function bumpFavorite() {
    const next = cycleFavoriteLevel(favorite);
    favorite = next;
    const prev = shuffleMetaByScene[currentSceneId];
    shuffleMetaByScene = {
      ...shuffleMetaByScene,
      [currentSceneId]: { favorite: next, lastPlayedAt: prev?.lastPlayedAt ?? null },
    };
    try {
      await scenes.setFavorite(currentSceneId, next);
    } catch (e) {
      console.warn("setFavorite failed", e);
    }
    pokeControls();
  }

  async function setFavoriteLevel(next: number) {
    favorite = next;
    const prev = shuffleMetaByScene[currentSceneId];
    shuffleMetaByScene = {
      ...shuffleMetaByScene,
      [currentSceneId]: { favorite: next, lastPlayedAt: prev?.lastPlayedAt ?? null },
    };
    try {
      await scenes.setFavorite(currentSceneId, next);
    } catch (e) {
      console.warn("setFavorite failed", e);
    }
    pokeControls();
  }

  // ─── watch-threshold play recording + dead-file skipping ────────────────
  // A scene counts as "played" after min(30s, 10% of duration) of ACCUMULATED
  // forward watch time (paused time and seek jumps don't count) — or at EOF,
  // which always counts so very short files still register. Recording on load
  // poisoned the shuffle's last_played_at anti-recency with Prev navigation
  // and accidental Next taps.
  let watchAccum = 0; // seconds of accumulated forward playback this load
  let lastWatchPos: number | null = null; // last time-pos sample (delta base)
  let playRecorded = false; // one recordPlay per load
  // Consecutive load failures — auto-skip stops and shows the persistent
  // error state once we've churned through everything playable.
  let consecutiveLoadFailures = 0;

  function watchThreshold(): number {
    if (duration != null && Number.isFinite(duration) && duration > 0) {
      return Math.min(30, 0.1 * duration);
    }
    return 30; // unknown/zero duration fallback
  }

  /** Threshold crossed (or EOF): record the play, then re-fetch detail
      AWAITED so the shuffle meta cache reads the post-play last_played_at
      (the old fire-on-load code re-fetched right after an UN-awaited
      recordPlay and could cache the pre-play timestamp). */
  async function recordPlayAndRefresh(sceneId: string) {
    try {
      await scenes.recordPlay(sceneId);
    } catch (e) {
      console.warn("recordPlay failed", e);
    }
    noteSessionPlay(sceneId); // session cooldown stays paired with recordPlay
    await refreshSceneDetail(sceneId);
  }

  /** Fire the once-per-load play record when the watch threshold is crossed.
      `force` (EOF) bypasses the threshold so very short files still count. */
  function maybeRecordPlay(force = false) {
    if (playRecorded || !currentSceneId) return;
    if (!force && watchAccum < watchThreshold()) return;
    playRecorded = true;
    void recordPlayAndRefresh(currentSceneId);
  }

  /** Accumulate forward watch time from time-pos samples. Paused time and
      seek discontinuities (>2s jump either way between samples) don't count. */
  function trackWatchTime(pos: number | null) {
    if (pos == null) {
      lastWatchPos = null;
      return;
    }
    if (!paused && lastWatchPos != null) {
      const delta = pos - lastWatchPos;
      if (delta > 0 && delta <= 2) {
        watchAccum += delta;
        maybeRecordPlay();
      }
    }
    lastWatchPos = pos;
  }

  /** Refresh favorite + last_played cache for a scene (hearts + shuffle weight). */
  async function refreshSceneDetail(sceneId: string) {
    try {
      const d = await scenes.detail(sceneId);
      const fav = d.scene.favorite ?? 0;
      // Don't clobber the heart UI if the user already moved to another scene.
      if (sceneId === currentSceneId) favorite = fav;
      shuffleMetaByScene = {
        ...shuffleMetaByScene,
        [sceneId]: {
          favorite: fav,
          lastPlayedAt: d.scene.last_played_at ?? new Date().toISOString(),
        },
      };
    } catch {
      // non-fatal
    }
  }

  /** Transient toast for a dead file; bring the chrome up so it's visible. */
  function flashSkipToast(sceneId: string, path: string | null) {
    const name = path?.split(/[\\/]/).pop() || sceneId;
    flashPlaylistToast(`Can't play ${name} — file missing, skipping`);
    showControls();
  }

  /** True once consecutive load failures have churned through everything
      playable (max(3, queue length)) — trips the persistent error state. */
  function loadFailureGuardTripped(): boolean {
    if (consecutiveLoadFailures < Math.max(3, queue.length)) return false;
    phase = "error";
    errorMsg = "No playable files in queue (drive offline?)";
    return true;
  }

  // ─── queue navigation ───────────────────────────────────────────────────

  async function refreshShuffleMeta(sceneIds: string[]) {
    const missing = sceneIds.filter((id) => !shuffleMetaByScene[id]);
    if (missing.length === 0) return;
    try {
      const rows = await scenes.shuffleMeta(missing);
      const next = { ...shuffleMetaByScene };
      for (const row of rows) {
        next[row.id] = {
          favorite: row.favorite ?? 0,
          lastPlayedAt: row.last_played_at,
        };
      }
      shuffleMetaByScene = next;
    } catch (e) {
      console.warn("shuffleMeta failed", e);
    }
  }

  function noteSessionPlay(sceneId: string) {
    const coolN = sessionCooldownSize(queue.length);
    const next = [...sessionRecent.filter((id) => id !== sceneId), sceneId];
    sessionRecent = next.slice(Math.max(0, next.length - Math.max(coolN, 1)));
  }

  function weightForShuffle(id: string): number {
    const coolN = sessionCooldownSize(queue.length);
    const coolSet = new Set(sessionRecent.slice(Math.max(0, sessionRecent.length - coolN)));
    return shuffleWeight(shuffleMetaByScene[id], { sessionCooldown: coolSet.has(id) });
  }

  /** Load a scene by ID into mpv. Resolves the file path, updates state.
      Returns true on success; false (with a toast + failure count) when the
      file is missing or unloadable, so callers can auto-skip like EOF does.
      `loadTimeoutMs` guards the initial mount load against a hung loadfile. */
  async function loadScene(sceneId: string, loadTimeoutMs?: number): Promise<boolean> {
    if (!player) return false;
    let path = await sceneFilePath(sceneId);
    if (!path) {
      // Fallback to the URL-passed path if this is the initial scene.
      if (sceneId === initialSceneId && initialFilePath) path = initialFilePath;
      else {
        console.warn(`No file path for scene ${sceneId}, skipping`);
        flashSkipToast(sceneId, null);
        consecutiveLoadFailures += 1;
        return false;
      }
    }
    currentSceneId = sceneId;
    seeking = false;
    scrubHoverTime = null;
    timePos = 0;
    // Reset watch-threshold tracking for the new load.
    watchAccum = 0;
    lastWatchPos = null;
    playRecorded = false;
    try {
      const load = player.loadFile(path);
      await (loadTimeoutMs != null
        ? withTimeout(load, loadTimeoutMs, "mpv loadfile timed out — likely a vo/hwdec/wid conflict")
        : load);
      await player.setPaused(false);
      paused = false;
      phase = "ready";
      showControls();
    } catch (e) {
      console.error("loadFile failed", e);
      flashSkipToast(sceneId, path);
      consecutiveLoadFailures += 1;
      return false;
    }
    consecutiveLoadFailures = 0;
    // Refresh favorite + last_played + scrub preview for this scene. The play
    // itself is recorded later — at the watch threshold, or at EOF.
    await refreshSceneDetail(sceneId);
    await loadScrubPreviewForScene(sceneId);
    await loadSegments(sceneId);
    segmentMarkIn = null;
    return true;
  }

  /** Weighted-random draw of the next scene (ADR-010/011), without replacement + anti-recency.
      `excludeId` keeps a just-failed (dead) scene out of the immediate redraw
      when a pass reset would otherwise put it back in the candidate pool. */
  function drawNextShuffled(excludeId: string | null = null): string | null {
    let remaining = queue.filter((id) => !shufflePlayed.has(id) && id !== excludeId);
    if (remaining.length === 0) {
      // Queue exhausted — new pass, but keep sessionRecent so recent titles stay cool.
      shufflePlayed = new Set();
      remaining = queue.filter((id) => id !== excludeId);
      if (remaining.length === 0) return null;
    }
    return weightedPickId(remaining, weightForShuffle);
  }

  async function goNext() {
    if (advancing || !player || !hasQueueNav) return;
    advancing = true;
    // Dead files chain inside one advance: keep drawing until something
    // plays, the queue/pass runs dry, or the failure guard trips.
    let lastFailedId: string | null = null;
    let pushedHistory = false;
    try {
      while (true) {
        let nextId: string | null;
        if (shuffleOn) {
          if (!pushedHistory && currentSceneId) {
            shuffleHistory = [...shuffleHistory, currentSceneId];
            pushedHistory = true;
          }
          nextId = drawNextShuffled(lastFailedId);
          if (nextId) {
            shufflePlayed = new Set([...shufflePlayed, nextId]);
            const idx = queue.indexOf(nextId);
            if (idx >= 0) queueIndex = idx;
          }
        } else {
          if (queueIndex >= queue.length - 1) break; // end of queue
          nextId = queue[queueIndex + 1];
          queueIndex += 1;
        }
        if (!nextId) break;
        if (await loadScene(nextId)) break;
        lastFailedId = nextId;
        if (loadFailureGuardTripped()) break;
      }
    } finally {
      advancing = false;
    }
    pokeControls();
  }

  async function goPrev() {
    if (advancing || !player || !hasPrev) return;
    advancing = true;
    try {
      if (shuffleOn) {
        const prevId = shuffleHistory[shuffleHistory.length - 1];
        shuffleHistory = shuffleHistory.slice(0, -1);
        if (!prevId) return;
        const idx = queue.indexOf(prevId);
        if (idx >= 0) queueIndex = idx;
        await loadScene(prevId);
      } else {
        queueIndex -= 1;
        await loadScene(queue[queueIndex]);
      }
    } finally {
      advancing = false;
    }
    pokeControls();
  }

  function toggleShuffle() {
    if (!hasQueueNav) return;
    shuffleOn = !shuffleOn;
    if (shuffleOn) {
      // Seed the shuffle with the current scene as already-played so we don't
      // immediately re-pick it.
      shufflePlayed = new Set([currentSceneId]);
      shuffleHistory = [];
    } else {
      // Resync the linear index to wherever we are.
      const idx = queue.indexOf(currentSceneId);
      if (idx >= 0) queueIndex = idx;
      shuffleHistory = [];
    }
    pokeControls();
  }

  async function toggleFullscreen() {
    try {
      const win = getCurrentWebviewWindow();
      const next = !(await win.isFullscreen());
      await win.setFullscreen(next);
      isFullscreen = next;
    } catch (e) {
      console.warn("fullscreen failed", e);
    }
    pokeControls();
  }

  async function closePlayerWindow() {
    try {
      await getCurrentWebviewWindow().close();
    } catch (e) {
      console.warn("close window failed", e);
    }
  }

  /** Pick the next scene id after deleting `excludeId` (linear or shuffle). */
  function nextSceneAfterDelete(excludeId: string): string | null {
    const idx = queue.indexOf(excludeId);
    if (idx < 0) return null;

    if (shuffleOn) {
      const remaining = queue.filter((id) => id !== excludeId && !shufflePlayed.has(id));
      if (remaining.length === 0) {
        const anyOther = queue.filter((id) => id !== excludeId);
        return weightedPickId(anyOther, weightForShuffle) ?? anyOther[0] ?? null;
      }
      return weightedPickId(remaining, weightForShuffle);
    }

    if (idx < queue.length - 1) return queue[idx + 1];
    return null;
  }

  /** Tear down mpv so the current file handle is released before delete. */
  async function releasePlayerHandle() {
    if (!player) return;
    unlisten?.();
    unlistenEvents?.();
    unlisten = null;
    unlistenEvents = null;
    const handle = player;
    player = null;
    try {
      await handle.stop();
    } catch {
      // non-fatal
    }
    try {
      await handle.destroy();
    } catch {
      // non-fatal
    }
    // Windows may hold the lock briefly after mpv releases the file.
    await delay(250);
  }

  async function deleteCurrentScene() {
    if (!player || deleting) return;
    const targetId = currentSceneId;
    const label = filename ?? targetId;
    const ok = await confirm(
      `Delete "${label}" permanently?\n\nThis removes the video file from disk and from your library.`,
      { title: "Delete video", kind: "warning", okLabel: "Delete", cancelLabel: "Cancel" },
    );
    if (!ok) return;

    deleting = true;
    advancing = true;
    let deletedOk = false;
    try {
      const nextId = nextSceneAfterDelete(targetId);

      if (nextId) {
        await loadScene(nextId);
        await delay(150);
      } else {
        phase = "loading";
        await releasePlayerHandle();
      }

      await scenes.delete(targetId);
      deletedOk = true;

      const newQueue = queue.filter((id) => id !== targetId);
      queue = newQueue;
      if (shuffleOn) {
        const next = new Set(shufflePlayed);
        next.delete(targetId);
        shufflePlayed = next;
      } else if (nextId) {
        queueIndex = newQueue.indexOf(nextId);
      }

      if (!nextId) {
        await getCurrentWebviewWindow().close();
        return;
      }
    } catch (e) {
      if (!deletedOk) {
        errorMsg = `Delete failed: ${stringifyError(e)}`;
        phase = "error";
      }
    } finally {
      deleting = false;
      advancing = false;
    }
    pokeControls();
  }

  // ─── mount: boot libmpv, load file, observe ─────────────────────────────
  let unlisten: (() => void) | null = null;
  let unlistenEvents: (() => void) | null = null;
  let removeResizeListener: (() => void) | null = null;

  onMount(async () => {
    pokeControls();
    const onResize = () => {
      windowWidth = window.innerWidth;
    };
    window.addEventListener("resize", onResize);
    onResize();
    removeResizeListener = () => window.removeEventListener("resize", onResize);

    // ─── claim the staged queue (if any) ──────────────────────────────────
    if (hasQueue) {
      try {
        const staged: StagedQueue | null = await claimPlayerQueue(windowLabel);
        if (staged && staged.scene_ids.length > 0) {
          queue = staged.scene_ids;
          queueIndex = Math.min(staged.start_index, queue.length - 1);
          shuffleOn = staged.shuffle_by_default;
          if (shuffleOn) {
            shufflePlayed = new Set([queue[queueIndex]]);
            shuffleHistory = [];
          }
          void refreshShuffleMeta(queue);
        } else {
          console.warn(
            `claim_player_queue returned empty for label=${windowLabel} (hasQueue=1)`,
          );
        }
      } catch (e) {
        console.warn("claim queue failed", e);
      }
    }

    if (!initialFilePath && !hasQueue) {
      phase = "error";
      errorMsg = "No file path provided to player window.";
      return;
    }

    try {
      // Timeout on init too — a hung mpv create must surface, not spin forever.
      player = await withTimeout(createPlayer(), 8000, "mpv init (createPlayer) timed out");

      // Restore last-used volume (or settings default) before the first file loads.
      const saved = await playerSettings.get();
      volume = saved.volume;
      muted = saved.muted;
      deleteEnabled = saved.delete_in_player_enabled;
      await player.setVolume(saved.volume);
      await player.setMuted(saved.muted);
      volumePersistReady = true;
    } catch (e) {
      phase = "error";
      errorMsg = `libmpv init failed: ${stringifyError(e)}. ` +
        `Ensure libmpv-2.dll + libmpv-wrapper.dll are bundled next to the app.`;
      return;
    }

    // Observe property changes → drive our reactive state.
    unlisten = await player.onPropertyChange((e: PlayerObservedEvent) => {
      switch (e.name) {
        case "pause":
          paused = Boolean(e.data);
          if (paused) {
            controlsVisible = true;
            if (hideTimer) clearTimeout(hideTimer);
          } else {
            scheduleHideControls();
          }
          break;
        case "time-pos": {
          const pos = e.data as number | null;
          if (!seeking) timePos = pos;
          trackWatchTime(pos);
          break;
        }
        case "duration": duration = e.data as number | null; break;
        case "volume": volume = e.data as number; break;
        case "mute": muted = e.data as boolean; break;
        case "filename": filename = e.data as string | null; break;
        case "hwdec-current": hwdec = e.data as string | null; break;
        case "eof-reached":
          eofReached = e.data as boolean;
          if (eofReached) {
            paused = true;
            // A fully watched scene always counts as played — force the
            // threshold so very short files don't slip under it.
            maybeRecordPlay(true);
            // Auto-advance to the next scene in the queue (ADR-011).
            if (hasNext) goNext();
          }
          break;
      }
    });

    // Diagnostic: log ALL mpv events. Remove once playback is confirmed stable.
    unlistenEvents = await player.onEvent((e) => {
      console.log("[mpv event]", e);
    });

    // Load the initial scene (8s timeout guards the first loadfile against a
    // vo/hwdec/wid hang). A dead first scene auto-skips forward through the
    // queue exactly like EOF would; only when nothing plays do we land on the
    // persistent error state.
    const startId = queue[queueIndex] ?? initialSceneId;
    if (!(await loadScene(startId, 8000))) {
      if (hasQueueNav) await goNext();
      if (phase === "loading") {
        // First scene dead and nothing (more) to advance to.
        phase = "error";
        errorMsg = hasQueueNav
          ? "No playable files in queue (drive offline?)"
          : `No playable file found for scene ${startId}.`;
      }
    }
  });

  onDestroy(() => {
    if (hideTimer) clearTimeout(hideTimer);
    if (saveVolumeTimer) clearTimeout(saveVolumeTimer);
    removeResizeListener?.();
    // Flush volume on close so the next window picks it up immediately.
    if (volumePersistReady) {
      void playerSettings.set(volume, muted);
    }
    unlisten?.();
    unlistenEvents?.();
    // releasePlayerHandle() may have already destroyed mpv during delete.
    player?.destroy().catch(() => {});
  });

  // Keyboard shortcuts (space=play/pause, ←/→=seek, ↑/↓=volume, m=mute, f=fav, F11=fullscreen)
  function onKeydown(e: KeyboardEvent) {
    // Don't hijack typing in form fields (e.g. the "New playlist" input).
    const target = e.target;
    if (
      target instanceof HTMLElement &&
      (target.tagName === "INPUT" ||
        target.tagName === "TEXTAREA" ||
        target.tagName === "SELECT" ||
        target.isContentEditable)
    ) {
      return;
    }
    if (e.key === "Escape") {
      if (segmentsPanelOpen || playlistPanelOpen) {
        e.preventDefault();
        closeFloatingPanels();
        pokeControls();
        return;
      }
      if (isFullscreen) {
        e.preventDefault();
        void toggleFullscreen();
        return;
      }
    }
    if (e.key === "F11") {
      e.preventDefault();
      void toggleFullscreen();
      return;
    }
    switch (e.key) {
      case " ":
        e.preventDefault();
        togglePlay();
        break;
      case "ArrowRight":
        player?.seek((timePos ?? 0) + 5, "relative");
        pokeControls();
        break;
      case "ArrowLeft":
        player?.seek((timePos ?? 0) - 5, "relative");
        pokeControls();
        break;
      case "ArrowUp":
        e.preventDefault();
        player?.setVolume(Math.min(100, volume + 5));
        persistVolume(Math.min(100, volume + 5), muted);
        pokeControls();
        break;
      case "ArrowDown":
        e.preventDefault();
        player?.setVolume(Math.max(0, volume - 5));
        persistVolume(Math.max(0, volume - 5), muted);
        pokeControls();
        break;
      case "m":
      case "M":
        toggleMute();
        break;
      case "f":
      case "F":
        bumpFavorite();
        break;
      case "n":
      case "N":
        goNext();
        break;
      case "p":
      case "P":
        goPrev();
        break;
      case "s":
      case "S":
        toggleShuffle();
        break;
      case "i":
      case "I":
        markSegmentIn();
        break;
      case "o":
      case "O":
        void saveSegmentAtOut();
        break;
      case "b":
      case "B":
        toggleSegmentsPanel();
        break;
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<!-- Transparent overlay root. The WINDOW is opaque (transparent:false in
     openPlayerWindow — that's required for wid embedding to work). But this
     webview's content is transparent, so mpv's child window (painting the
     video underneath the webview) shows through everywhere our CSS is
     transparent. Only the actual controls have visible backgrounds. -->
<main
  class="player-shell relative h-screen w-screen overflow-hidden bg-transparent text-foreground"
  class:cursor-none={!controlsVisible && phase === "ready"}
  onpointermove={onPointerActivity}
  onpointerdown={showControls}
>
  {#if phase === "loading"}
    <div class="absolute inset-0 flex items-center justify-center bg-black/40">
      <Loader2 class="size-8 animate-spin text-primary" />
    </div>
  {:else if phase === "error"}
    <div class="absolute inset-0 flex flex-col items-center justify-center gap-3 bg-black/70 p-8 text-center">
      <AlertTriangle class="size-10 text-destructive" />
      <p class="max-w-md text-sm text-muted-foreground">{errorMsg}</p>
      <button
        type="button"
        onclick={() => void closePlayerWindow()}
        class="mt-1 rounded-md bg-primary px-3 py-1.5 text-xs text-primary-foreground transition hover:opacity-90"
      >
        Close window
      </button>
    </div>
  {/if}

  <!-- ─── overlay (only when video is up) ──────────────────────────────── -->
  {#if phase === "ready"}
    <!-- Near-invisible hit layer so pointer events reach the webview when
         chrome is hidden (fully transparent pixels pass through to mpv). -->
    {#if !controlsVisible}
      <div
        class="absolute inset-0 z-10"
        style="background: rgba(0,0,0,0.01)"
        aria-hidden="true"
      ></div>
    {/if}

    <!-- Top gradient + title -->
    <div
      class="pointer-events-none absolute inset-x-0 top-0 z-20 flex items-start justify-between gap-4 bg-gradient-to-b from-black/70 to-transparent p-4 pb-12 transition-opacity duration-200"
      class:opacity-0={!controlsVisible}
      class:pointer-events-none={!controlsVisible}
      style={!controlsVisible ? "visibility:hidden" : undefined}
    >
      <div class="min-w-0">
        <div class="truncate text-sm font-medium text-white drop-shadow">
          {filename ?? "Playing"}
        </div>
        <div class="mt-0.5 flex items-center gap-2 text-xs text-white/60">
          {#if queue.length > 1}
            <span>{shuffleOn ? "shuffle" : `${queueIndex + 1}/${queue.length}`}</span>
            <span aria-hidden="true">·</span>
          {/if}
          {#if hwdec && hwdec !== "no" && hwdec !== ""}
            <span class="text-emerald-400">HW: {hwdec}</span>
          {:else}
            <span class="text-amber-400">HW: off</span>
          {/if}
        </div>
      </div>
    </div>

    <!-- Bottom controls -->
    <div
      class="absolute inset-x-0 bottom-0 z-20 bg-gradient-to-t from-black/80 to-transparent px-4 pb-4 pt-12 transition-opacity duration-200"
      class:opacity-0={!controlsVisible}
      class:pointer-events-none={!controlsVisible}
      style={!controlsVisible ? "visibility:hidden" : undefined}
    >
      <!-- scrubber -->
      <div class="mb-2 flex items-center gap-3">
        <span class="w-14 text-right font-mono text-xs tabular-nums text-white/80">
          {fmtTime(effectiveTime)}
        </span>
        <div
          class="relative flex min-w-0 flex-1 items-center py-3"
          onmousemove={onScrubTrackMove}
          onmouseleave={onScrubPointerLeave}
          role="presentation"
        >
          {#if scrubPreviewVisible && scrubPreviewTime != null && scrubPreviewStyles}
            <div
              class="pointer-events-none absolute bottom-full z-30 mb-1 -translate-x-1/2"
              style:left="{scrubHoverX}px"
              aria-hidden="true"
            >
              <div
                class="overflow-hidden rounded-md border border-white/20 bg-black shadow-lg"
                style={scrubPreviewStyles}
              ></div>
              <div class="mt-0.5 text-center font-mono text-[10px] tabular-nums text-white/80">
                {fmtTime(scrubPreviewTime)}
              </div>
            </div>
          {/if}
          <!-- segment markers on the track -->
          {#if duration && duration > 0}
            <div class="pointer-events-none absolute inset-x-0 top-1/2 z-10 h-0 -translate-y-1/2" aria-hidden="true">
              {#each sceneSegments as seg (seg.id)}
                {#if seg.end_sec != null}
                  <div
                    class="absolute top-1/2 h-1.5 -translate-y-1/2 rounded-sm bg-amber-400/50"
                    style:left="{segmentPct(seg.start_sec)}%"
                    style:width="{Math.max(0.4, segmentPct(seg.end_sec) - segmentPct(seg.start_sec))}%"
                  ></div>
                {/if}
                <button
                  type="button"
                  class="pointer-events-auto absolute top-1/2 z-20 h-3 w-1.5 -translate-x-1/2 -translate-y-1/2 rounded-sm bg-amber-300 shadow hover:bg-amber-200"
                  style:left="{segmentPct(seg.start_sec)}%"
                  title="{seg.label || 'Segment'} @ {fmtTime(seg.start_sec)}"
                  aria-label="Jump to {seg.label || 'segment'}"
                  onclick={(e) => {
                    e.stopPropagation();
                    void seekToSegment(seg);
                  }}
                ></button>
              {/each}
              {#if segmentMarkIn != null}
                <div
                  class="absolute top-1/2 z-20 h-3 w-1 -translate-x-1/2 -translate-y-1/2 rounded-sm bg-sky-400"
                  style:left="{segmentPct(segmentMarkIn)}%"
                  title="In: {fmtTime(segmentMarkIn)}"
                ></div>
              {/if}
            </div>
          {/if}
          <input
            type="range"
            min="0"
            max={duration ?? 0}
            step="0.1"
            value={effectiveTime}
            onmousedown={onScrubStart}
            onpointerdown={onScrubStart}
            oninput={onScrubInput}
            onmouseup={onScrubCommit}
            onpointerup={onScrubCommit}
            ontouchstart={onScrubStart}
            ontouchend={onScrubCommit}
            onpointermove={onScrubPointerMove}
            class="maize-scrubber relative z-0 h-1.5 w-full cursor-pointer appearance-none rounded-full bg-white/25"
            style="background: linear-gradient(to right, hsl(var(--primary)) {progress * 100}%, rgba(255,255,255,0.25) {progress * 100}%);"
            aria-label="Seek"
          />
        </div>
        <span class="w-14 font-mono text-xs tabular-nums text-white/80">
          {duration != null ? fmtTime(duration) : "--:--"}
        </span>
      </div>

      <!-- button row -->
      <div class="flex items-center gap-3">
        <button
          type="button"
          onclick={goPrev}
          disabled={!hasPrev}
          aria-label="Previous"
          title={hasQueueNav ? "Previous (P)" : "Open a playlist or multi-select Play for queue navigation"}
          class="rounded-full p-1.5 text-white transition hover:bg-white/15 disabled:opacity-30 disabled:hover:bg-transparent"
        >
          <SkipBack class="size-5" fill="currentColor" />
        </button>

        <button
          type="button"
          onclick={togglePlay}
          aria-label={paused ? "Play" : "Pause"}
          class="rounded-full p-1.5 text-white transition hover:bg-white/15"
        >
          {#if paused}
            <Play class="size-5" fill="currentColor" />
          {:else}
            <Pause class="size-5" fill="currentColor" />
          {/if}
        </button>

        <button
          type="button"
          onclick={goNext}
          disabled={!hasNext}
          aria-label="Next"
          title={hasQueueNav ? "Next (N)" : "Open a playlist or multi-select Play for queue navigation"}
          class="rounded-full p-1.5 text-white transition hover:bg-white/15 disabled:opacity-30 disabled:hover:bg-transparent"
        >
          <SkipForward class="size-5" fill="currentColor" />
        </button>

        <button
          type="button"
          onclick={toggleShuffle}
          disabled={!hasQueueNav}
          aria-label="Shuffle"
          title={hasQueueNav ? "Shuffle (S) — weighted by favorite" : "Needs a queue (playlist / multi-select Play)"}
          class="rounded-full p-1.5 transition hover:bg-white/15 disabled:opacity-30 disabled:hover:bg-transparent"
          class:text-primary={shuffleOn && hasQueueNav}
          class:text-white={!shuffleOn || !hasQueueNav}
        >
          <Shuffle class="size-4" />
        </button>

        <!-- volume -->
        <div class="flex items-center gap-1.5">
          <button
            type="button"
            onclick={toggleMute}
            aria-label={muted ? "Unmute" : "Mute"}
            class="rounded-full p-1.5 text-white transition hover:bg-white/15"
          >
            {#if muted || volume === 0}
              <VolumeX class="size-5" />
            {:else}
              <Volume2 class="size-5" />
            {/if}
          </button>
          <input
            type="range"
            min="0"
            max="100"
            value={muted ? 0 : volume}
            oninput={onVolumeInput}
            class="maize-scrubber h-1 w-20 cursor-pointer appearance-none rounded-full bg-white/25"
            aria-label="Volume"
          />
        </div>

        <span class="ml-1 text-xs text-white/50">
          {#if hasQueueNav}
            {shuffleOn ? "shuffle" : `${queueIndex + 1}/${queue.length}`} ·
          {/if}
          space · ←/→ · ↑/↓ · m · f · F11 · i/o · b{#if hasQueueNav} · n · p · s{/if}
        </span>

        <!-- spacer -->
        <div class="flex-1"></div>

        <button
          type="button"
          onclick={markSegmentIn}
          aria-label="Mark segment in"
          title="Mark in (I)"
          class="rounded-full p-1.5 text-white transition hover:bg-white/15"
          class:text-sky-400={segmentMarkIn != null}
        >
          <BookmarkPlus class="size-5" />
        </button>
        <button
          type="button"
          onclick={() => void saveSegmentAtOut()}
          aria-label="Save segment out"
          title={segmentMarkIn != null ? "Mark out & save (O)" : "Bookmark here (O)"}
          class="rounded-full p-1.5 text-white transition hover:bg-white/15"
        >
          <Bookmark class="size-5" />
        </button>

        <div class="relative">
          <button
            type="button"
            onclick={toggleSegmentsPanel}
            aria-label="Segments"
            title="Segments (B)"
            class="relative rounded-full p-1.5 text-white transition hover:bg-white/15"
            class:text-primary={segmentsPanelOpen}
          >
            <Bookmark class="size-5" />
            {#if sceneSegments.length > 0}
              <span
                class="absolute -right-0.5 -top-0.5 min-w-3.5 rounded-full bg-amber-400 px-0.5 text-center text-[9px] font-bold leading-3.5 text-black"
              >
                {sceneSegments.length}
              </span>
            {/if}
          </button>
          {#if segmentsPanelOpen}
            <div
              class="absolute bottom-full right-0 z-40 mb-2 max-h-56 w-64 overflow-y-auto rounded-lg border border-white/15 bg-black/90 p-2 shadow-xl backdrop-blur"
              role="dialog"
              aria-label="Scene segments"
            >
              <div class="mb-1 flex items-center justify-between gap-2 px-1">
                <span class="text-[11px] font-medium text-white/70">Segments</span>
                <button
                  type="button"
                  class="rounded p-0.5 text-white/40 hover:bg-white/10 hover:text-white"
                  aria-label="Close segments"
                  title="Close (Esc)"
                  onclick={closeFloatingPanels}
                >
                  <X class="size-3.5" />
                </button>
              </div>
              {#if sceneSegments.length === 0}
                <p class="px-1 py-1 text-xs text-white/50">
                  Empty — I then O for a range, or O for a bookmark.
                </p>
              {:else}
                <ul class="space-y-1">
                  {#each sceneSegments as seg (seg.id)}
                    <li class="flex items-center gap-1 rounded px-1 py-0.5 hover:bg-white/10">
                      <button
                        type="button"
                        class="min-w-0 flex-1 truncate text-left text-xs text-white"
                        onclick={() => void seekToSegment(seg)}
                      >
                        <span class="font-mono text-white/60">{fmtTime(seg.start_sec)}</span>
                        {#if seg.end_sec != null}
                          <span class="text-white/40">–{fmtTime(seg.end_sec)}</span>
                        {/if}
                        <span class="ml-1">{seg.label || "Untitled"}</span>
                      </button>
                      <button
                        type="button"
                        class="rounded p-1 text-white/40 hover:bg-white/10 hover:text-red-400"
                        aria-label="Delete segment"
                        onclick={() => void deleteSegment(seg.id)}
                      >
                        <Trash2 class="size-3.5" />
                      </button>
                    </li>
                  {/each}
                </ul>
              {/if}
            </div>
          {/if}
        </div>

        <FavoriteButton
          level={favorite}
          variant="overlay"
          class="rounded-full px-2 py-1 transition hover:bg-white/15"
          onChange={setFavoriteLevel}
        />

        <div class="relative">
          <button
            type="button"
            onclick={togglePlaylistPanel}
            aria-label="Add to playlist"
            title="Add to playlist"
            class="rounded-full p-1.5 text-white transition hover:bg-white/15"
            class:text-primary={playlistPanelOpen}
          >
            <ListPlus class="size-5" />
          </button>

          {#if playlistPanelOpen}
            <div
              class="absolute bottom-full right-0 z-30 mb-2 w-56 rounded-lg border border-white/15 bg-black/90 p-2 shadow-xl"
              role="dialog"
              aria-label="Add to playlist"
            >
              <div class="mb-1.5 text-xs font-medium text-white/70">Add to playlist</div>
              {#if allPlaylists.length > 0}
                <select
                  value=""
                  onchange={(e) => {
                    const v = (e.currentTarget as HTMLSelectElement).value;
                    if (v) {
                      const pl = allPlaylists.find((x) => x.id === v);
                      if (pl) void addCurrentToPlaylist(pl.id, pl.name);
                      (e.currentTarget as HTMLSelectElement).value = "";
                    }
                  }}
                  class="h-8 w-full rounded-md border border-white/20 bg-black/50 px-2 text-xs text-white outline-none focus-visible:ring-2 focus-visible:ring-primary"
                >
                  <option value="">Choose a playlist…</option>
                  {#each allPlaylists as pl (pl.id)}
                    <option value={pl.id}>{pl.name} ({pl.item_count})</option>
                  {/each}
                </select>
              {:else if !showNewPlaylistField}
                <p class="text-xs text-white/50">No playlists yet.</p>
              {/if}

              {#if showNewPlaylistField}
                <div class="mt-1.5 flex gap-1">
                  <input
                    bind:value={newPlaylistName}
                    placeholder="New playlist"
                    class="h-8 min-w-0 flex-1 rounded-md border border-white/20 bg-black/50 px-2 text-xs text-white outline-none focus-visible:ring-2 focus-visible:ring-primary"
                    onkeydown={(e) => {
                      if (e.key === "Enter") void createPlaylistAndAddCurrent();
                      if (e.key === "Escape") {
                        showNewPlaylistField = false;
                        newPlaylistName = "";
                      }
                    }}
                  />
                  <button
                    type="button"
                    class="rounded-md bg-primary px-2 text-xs text-primary-foreground disabled:opacity-40"
                    disabled={!newPlaylistName.trim()}
                    onclick={() => void createPlaylistAndAddCurrent()}
                  >
                    Add
                  </button>
                </div>
              {:else}
                <button
                  type="button"
                  class="mt-1.5 flex items-center gap-1 text-xs text-primary hover:underline"
                  onclick={() => (showNewPlaylistField = true)}
                >
                  <Plus class="size-3" />
                  New playlist
                </button>
              {/if}
            </div>
          {/if}
        </div>

        {#if playlistToast}
          <span class="rounded bg-black/70 px-2 py-0.5 text-xs text-white/90">{playlistToast}</span>
        {/if}

        {#if deleteEnabled}
          <button
            type="button"
            onclick={deleteCurrentScene}
            disabled={deleting}
            aria-label="Delete video"
            title="Delete file permanently"
            class="rounded-full p-1.5 text-red-400 transition hover:bg-white/15 disabled:opacity-40"
          >
            {#if deleting}
              <Loader2 class="size-5 animate-spin" />
            {:else}
              <Trash2 class="size-5" />
            {/if}
          </button>
        {/if}

        <button
          type="button"
          onclick={() => void toggleFullscreen()}
          aria-label={isFullscreen ? "Exit fullscreen" : "Fullscreen"}
          title={isFullscreen ? "Exit fullscreen (F11 / Esc)" : "Fullscreen (F11)"}
          class="rounded-full p-1.5 text-white transition hover:bg-white/15"
        >
          {#if isFullscreen}
            <Minimize2 class="size-5" />
          {:else}
            <Maximize2 class="size-5" />
          {/if}
        </button>

        <button
          type="button"
          onclick={() => void closePlayerWindow()}
          aria-label="Close player"
          title="Close window"
          class="rounded-full p-1.5 text-white transition hover:bg-white/15"
        >
          <X class="size-5" />
        </button>
      </div>
    </div>
  {/if}
</main>

<style>
  /* The WINDOW is opaque (good for wid embedding), but this webview's body
     must be transparent so mpv's video (painting a child window underneath
     the webview) shows through. The global app.css sets a dark body bg;
     override it here for the player window only. */
  :global(body:has(.player-shell)) {
    background: transparent !important;
  }
</style>
