// Player API wrapper.
//
// This module is the *only* place that knows about tauri-plugin-libmpv. The
// overlay UI (src/player/App.svelte + components) talks to this wrapper, never
// to the plugin directly. That keeps the overlay code plugin-agnostic: if we
// ever swap libmpv for a hand-rolled binding (ADR-012 fallback path), only this
// file changes.
//
// Architecture (ADR-005 / ADR-012):
//   - Each video plays in its own native OS Tauri window (label `player-<sceneId>`).
//   - libmpv paints into that window's surface; the webview is transparent on
//     top, so our HTML/Svelte overlay floats above the video.
//   - One mpv instance per window, managed by the plugin.

import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { invoke } from "@tauri-apps/api/core";
import {
  init as mpvInit,
  command as mpvCommand,
  setProperty as mpvSetProperty,
  getProperty as mpvGetProperty,
  observeProperties as mpvObserve,
  destroy as mpvDestroy,
  type MpvConfig,
  type MpvEventFromProperties,
  type MpvFormat,
  type MpvObservableProperty,
  type UnlistenFn as MpvUnlisten,
} from "tauri-plugin-libmpv-api";
import type { UnlistenFn } from "@tauri-apps/api/event";

// ─── window management ──────────────────────────────────────────────────

export interface OpenPlayerOpts {
  /** The scene to start playing at. */
  sceneId: string;
  /**
   * Absolute filesystem path of the file to play. Required for single-scene
   * playback. When `sceneIds` is provided (queue mode), this is optional —
   * the player resolves each scene's path on demand via scene_file_path.
   */
  filePath?: string;
  /**
   * Optional queue: the full ordered list of scene IDs this window will play
   * through (ADR-011). `sceneId` must be in this list and is the start point.
   * If omitted, the window plays just `sceneId`.
   */
  sceneIds?: string[];
  /** Inherit shuffle default from the source playlist (ADR-011). */
  shuffleByDefault?: boolean;
  /** When true, always create a new window instead of focusing an existing one. */
  forceNewWindow?: boolean;
  /** Optional window title (defaults to "MaizeView — Player"). */
  title?: string;
  /** Optional width/height hint. Defaults to 960×540. */
  width?: number;
  height?: number;
}

/**
 * Open a native OS player window. Idempotent per sceneId: if a window for this
 * scene already exists, it's just focused.
 *
 * Queue mode: pass `sceneIds` to give the window a play-through queue. The
 * queue is staged via the Rust-side `stage_player_queue` command (keyed by
 * window label) because webviews are isolated and URL query strings have
 * length limits. The player window claims it on mount via `claim_player_queue`.
 */
export async function openPlayerWindow(opts: OpenPlayerOpts): Promise<WebviewWindow> {
  const label = opts.forceNewWindow
    ? `player-${opts.sceneId}-${Date.now()}`
    : `player-${opts.sceneId}`;
  const existing = opts.forceNewWindow ? null : await WebviewWindow.getByLabel(label);
  if (existing) {
    await existing.setFocus();
    return existing;
  }

  // Stage the queue (if any) before the window opens, so it's available when
  // the player mounts and claims it.
  if (opts.sceneIds && opts.sceneIds.length > 0) {
    const startIndex = Math.max(0, opts.sceneIds.indexOf(opts.sceneId));
    await invoke("stage_player_queue", {
      label,
      sceneIds: opts.sceneIds,
      startIndex,
      shuffleByDefault: opts.shuffleByDefault ?? false,
    });
  }

  // Pass only the starting sceneId + file via URL (short). The rest of the
  // queue comes through the Rust stash.
  const urlParams = new URLSearchParams();
  urlParams.set("sceneId", opts.sceneId);
  if (opts.filePath) urlParams.set("file", opts.filePath);
  if (opts.sceneIds && opts.sceneIds.length > 0) {
    // Signal that a staged queue is waiting. The sceneIds themselves are NOT
    // in the URL (too long for some queues); the player claims them from Rust.
    urlParams.set("hasQueue", "1");
  }
  const url = `player.html?${urlParams.toString()}`;

  const win = new WebviewWindow(label, {
    url,
    title: opts.title ?? "MaizeView — Player",
    width: opts.width ?? 960,
    height: opts.height ?? 540,
    minWidth: 480,
    minHeight: 270,
    // Transparent so mpv (embedding via `wid` into a child window of this
    // window) shows through the webview wherever our overlay CSS is
    // transparent. We previously disabled this thinking it caused the init
    // hang, but the probe proved the hang was `osd_level=0`, not transparency.
    // The README requires transparent:true for embedding to work.
    transparent: true,
  });

  // WebviewWindow creation is async; the constructor returns immediately and
  // emits events. Wait for it to actually exist before resolving.
  return new Promise((resolve, reject) => {
    let settled = false;
    const onCreated = () => {
      if (settled) return;
      settled = true;
      resolve(win);
    };
    const onError = (e: unknown) => {
      if (settled) return;
      settled = true;
      reject(e);
    };
    win.once("tauri://created", onCreated);
    win.once("tauri://error", onError);
  });
}

/** Close every open player window (`player-*` labels). Returns how many closed. */
export async function closeAllPlayerWindows(): Promise<number> {
  // Rust-side close — catalog JS cannot close sibling windows under Tauri ACL.
  return invoke<number>("close_all_player_windows");
}

// ─── 4Play ──────────────────────────────────────────────────────────────

export interface OpenQuadOpts {
  /**
   * Window label — must be the same label the queue was staged under
   * (`player-quad-*`, which matches the `player-*` capability glob, so the
   * window gets libmpv permissions).
   */
  label: string;
  /** Optional window title (defaults to "MaizeView — 4Play"). */
  title?: string;
}

/**
 * Open the 4Play window: one transparent window running quad.html, which
 * creates 4 quadrant child HWNDs (quad_create_panes), embeds four mpv
 * instances into the top-level window, and fits each over a quadrant. The
 * caller must stage the queue FIRST via stagePlayerQueue under the same
 * label — quad.html claims it on mount.
 */
export async function openQuadWindow(opts: OpenQuadOpts): Promise<WebviewWindow> {
  const win = new WebviewWindow(opts.label, {
    url: "quad.html",
    title: opts.title ?? "MaizeView — 4Play",
    width: 1280,
    height: 720,
    minWidth: 640,
    minHeight: 360,
    // Transparent for the same overlay-over-mpv sandwich as openPlayerWindow.
    transparent: true,
  });

  // Same created/error handshake as openPlayerWindow.
  return new Promise((resolve, reject) => {
    let settled = false;
    const onCreated = () => {
      if (settled) return;
      settled = true;
      resolve(win);
    };
    const onError = (e: unknown) => {
      if (settled) return;
      settled = true;
      reject(e);
    };
    win.once("tauri://created", onCreated);
    win.once("tauri://error", onError);
  });
}

/**
 * Open 4Play over an explicit scene list: stages the WHOLE list (the quad
 * window plays up to 4 at once and rotates through the rest on EOF), then
 * opens the window. `shuffleByDefault` makes the rotation order weighted
 * (ADR-010) instead of list order. Shared by the playlist toolbar and the
 * library selection bar.
 */
export async function openQuadWithScenes(
  sceneIds: string[],
  shuffleByDefault = false,
): Promise<void> {
  if (sceneIds.length === 0) return;
  const label = `player-quad-${Date.now()}`;
  await stagePlayerQueue(label, sceneIds, 0, shuffleByDefault);
  await openQuadWindow({ label });
}

// ─── cross-window queue handoff (player-side) ───────────────────────────

export interface StagedQueue {
  scene_ids: string[];
  start_index: number;
  shuffle_by_default: boolean;
}

/** Player-side: claim the staged queue for this window (called once on mount). */
export async function claimPlayerQueue(label: string): Promise<StagedQueue | null> {
  return invoke<StagedQueue | null>("claim_player_queue", { label });
}

/**
 * Catalog-side: stage a queue for a not-yet-opened window under an explicit
 * label. openPlayerWindow stages inline for its own labels; this wrapper is
 * for windows that manage their own label (the 4Play quad window).
 */
export async function stagePlayerQueue(
  label: string,
  sceneIds: string[],
  startIndex = 0,
  shuffleByDefault = false,
): Promise<void> {
  return invoke("stage_player_queue", {
    label,
    sceneIds,
    startIndex,
    shuffleByDefault,
  });
}

/** Resolve the playable file path for a scene (used by the player to advance). */
export async function sceneFilePath(sceneId: string): Promise<string | null> {
  return invoke<string | null>("scene_file_path", { sceneId });
}

export interface SceneScrubPreview {
  sprite_path: string;
  vtt_text: string;
}

/** Sprite path + VTT text for scrubber hover previews (reads VTT via Rust). */
export async function sceneScrubPreview(sceneId: string): Promise<SceneScrubPreview | null> {
  return invoke<SceneScrubPreview | null>("scene_scrub_preview", { sceneId });
}

// ─── libmpv host (one per player window) ─────────────────────────────────

/**
 * The mpv properties we want to observe for a live overlay: playback state,
 * position, duration, volume, mute, the active file, and the hardware-decode
 * backend (so we can confirm HW accel is actually engaged — ADR-005).
 *
 * The optional `'none'` marker tells the plugin's TS types that a property
 * may legitimately be null (e.g. `time-pos` is null when nothing is loaded).
 */
export const PLAYER_OBSERVED_PROPERTIES = [
  ["pause", "flag"],
  ["time-pos", "double", "none"],
  ["duration", "double", "none"],
  ["volume", "double"],
  ["mute", "flag"],
  ["filename", "string", "none"],
  // Requested hwdec backend (e.g. "d3d11va"). May be set even when HW isn't active.
  ["hwdec", "string", "none"],
  // Actually-active hw decoder — empty string means software decode. Use this for the overlay.
  ["hwdec-current", "string", "none"],
  ["eof-reached", "flag"],
] as const satisfies MpvObservableProperty[];

export type PlayerObservedProperty = (typeof PLAYER_OBSERVED_PROPERTIES)[number];
export type PlayerObservedEvent = MpvEventFromProperties<PlayerObservedProperty>;

/**
 * Default mpv config per ADR-005 / ADR-012.
 *
 * NOTE on the vo/force-window choice: the plugin embeds mpv via the legacy
 * `wid` option (it passes our window's HWND to mpv, which creates a child
 * window to render into). With `wid` set:
 *   - Do NOT set `force-window=yes` — it makes mpv create its OWN window,
 *     which deadlocks against the embedded child window (observed: spinner
 *     never clears, app hangs on close). `wid` already implies "always have
 *     a surface."
 *   - Prefer `vo=gpu` over `vo=gpu-next` for embedded mode. `gpu-next` is
 *     newer/fancier but has shown render-thread stalls in `wid` embedding on
 *     Windows. `gpu` is the battle-tested embedded path.
 *   - `hwdec=auto-safe` lets mpv pick DXVA2/D3D11VA — the whole point.
 *   - `keep-open=yes` keeps the file loaded after EOF so the user can scrub.
 *   - `osc=no` + `osd_level=0`: our Svelte overlay replaces mpv's stock OSD.
 *
 * If we later move to the render API (D3D11 texture sharing) per the original
 * ADR-005 sketch, `force-window` + `gpu-next` become viable again — but that
 * requires the plugin to expose render-API hooks it currently doesn't.
 */
export const DEFAULT_MPV_CONFIG: MpvConfig = {
  initialOptions: {
    // `vo=gpu` + `gpu-api=d3d11` is the modern Windows render path: it uses
    // Direct3D 11 for output (the most reliable on Windows) AND properly
    // reports `hwdec` (which `vo=direct3d`, a legacy VO, does not — it showed
    // "HW: off" even when decode worked). The standalone probe confirmed
    // `gpu` + `d3d11` initializes without hanging.
    vo: "gpu",
    "gpu-api": "d3d11",
    // auto-safe probes DXVA2/D3D11VA and falls back gracefully. d3d11va alone
    // can fail silently on some GPU configs; auto-safe is the better default.
    hwdec: "auto-safe",
    "keep-open": "yes",
    // `osc=no` disables mpv's stock on-screen-controller (we draw our own).
    // DO NOT add `osd_level=0` here — it's a runtime property, and setting it
    // as an init option makes libmpv-wrapper's mpv_wrapper_create deadlock
    // (verified via tools/mpv-probe). `osc=no` is the correct way to suppress
    // mpv's UI.
    osc: "no",
  },
  observedProperties: PLAYER_OBSERVED_PROPERTIES,
};

/**
 * A handle to a single window's mpv instance. Created by `createPlayer()`.
 * Holds the unlisten fn for property observation so it can be torn down on
 * window close.
 */
export interface PlayerHandle {
  /** Observe mpv property changes (pause, time-pos, hwdec, …). Returns unlisten. */
  onPropertyChange: (cb: (e: PlayerObservedEvent) => void) => Promise<MpvUnlisten>;
  /** Load a file by absolute path. Replaces the current playlist entry. */
  loadFile: (path: string) => Promise<void>;
  /** Toggle pause. */
  togglePause: () => Promise<void>;
  /** Set paused state explicitly. */
  setPaused: (paused: boolean) => Promise<void>;
  /** Seek to an absolute time (seconds) or by a relative offset. */
  seek: (seconds: number, mode?: "absolute" | "relative") => Promise<void>;
  /** Set volume 0–100. */
  setVolume: (volume: number) => Promise<void>;
  /** Toggle mute. */
  toggleMute: () => Promise<void>;
  /** Set mute explicitly. */
  setMuted: (muted: boolean) => Promise<void>;
  /** Stop playback and release the current file. */
  stop: () => Promise<void>;
  /** Read a property. */
  getProperty: <T extends MpvFormat>(name: string, format: T) => Promise<unknown>;
  /** Stop the mpv instance for this window (called on window close). */
  destroy: () => Promise<void>;
}

/**
 * Boot an mpv instance in the current player window. Must be called from
 * inside a player window (one that was opened via `openPlayerWindow`).
 */
export async function createPlayer(config: MpvConfig = DEFAULT_MPV_CONFIG): Promise<PlayerHandle> {
  // `init` resolves with the window label it attached to.
  await mpvInit(config);

  return {
    onPropertyChange: (cb) => mpvObserve(PLAYER_OBSERVED_PROPERTIES, cb),

    loadFile: (path) => mpvCommand("loadfile", [path]),

    togglePause: async () => {
      const paused = await mpvGetProperty<boolean>("pause", "flag");
      await mpvSetProperty("pause", !paused);
    },
    setPaused: (paused) => mpvSetProperty("pause", paused),

    seek: (seconds, mode = "absolute") => mpvCommand("seek", [seconds, mode]),

    setVolume: (volume) => mpvSetProperty("volume", Math.max(0, Math.min(100, volume))),
    toggleMute: async () => {
      const muted = await mpvGetProperty<boolean>("mute", "flag");
      await mpvSetProperty("mute", !muted);
    },
    setMuted: (muted) => mpvSetProperty("mute", muted),

    getProperty: (name, format) => mpvGetProperty(name, format),

  /** Stop playback and release the current file handle. */
  stop: () => mpvCommand("stop"),

  destroy: () => mpvDestroy(),
  };
}

// Re-export the plugin's UnlistenFn type for callers that need it.
export type { UnlistenFn };
