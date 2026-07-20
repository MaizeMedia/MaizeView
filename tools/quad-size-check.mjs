// Verify quad window-size memory: open a quad window, resize it, close it,
// open another, assert the size was restored from localStorage.
import { chromium } from "@playwright/test";

const CDP = process.env.CDP_URL ?? "http://127.0.0.1:9222";
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

const browser = await chromium.connectOverCDP(CDP);
const catalog = browser
  .contexts()
  .flatMap((c) => c.pages())
  .find((p) => p.url().includes("catalog.html"));
if (!catalog) throw new Error("catalog page not found");

async function openQuad(label) {
  await catalog.evaluate(
    async ({ label }) => {
      const invoke = window.__TAURI__.core.invoke;
      const playlists = await invoke("list_playlists");
      let ids = null;
      for (const p of playlists) {
        const items = await invoke("playlist_items", { playlistId: p.id });
        const playable = items.filter((s) => s.file_path);
        if (playable.length >= 4) {
          ids = playable.slice(0, 4).map((s) => s.id);
          break;
        }
      }
      await invoke("stage_player_queue", { label, sceneIds: ids, startIndex: 0, shuffleByDefault: false });
      const { WebviewWindow } = window.__TAURI__.webviewWindow;
      new WebviewWindow(label, {
        url: "quad.html",
        title: "MaizeView — 4Play",
        width: 1280,
        height: 720,
        minWidth: 640,
        minHeight: 360,
        transparent: true,
      });
    },
    { label },
  );
  for (let i = 0; i < 30; i++) {
    for (const p of browser.contexts().flatMap((c) => c.pages()).filter((p) => p.url().includes("quad.html"))) {
      try {
        const l = await p.evaluate(() => window.__TAURI__?.webviewWindow?.getCurrentWebviewWindow?.().label ?? null);
        if (l === label) return p;
      } catch { /* booting */ }
    }
    await wait(500);
  }
  throw new Error("quad page never appeared: " + label);
}

// 1. Open + resize to a distinctive size, wait for the debounced save.
const l1 = `player-quad-size-${Date.now()}`;
const w1 = await openQuad(l1);
await w1.evaluate(async () => {
  const { PhysicalSize } = window.__TAURI__.dpi;
  await window.__TAURI__.window.getCurrentWindow().setSize(new PhysicalSize(1010, 610));
});
await wait(1500); // debounced save (100ms) + slack

// 2. Close the window (Rust close_all handles it).
await catalog.evaluate(() => window.__TAURI__.core.invoke("close_all_player_windows"));
await wait(1500);

// 3. Reopen and measure.
const l2 = `player-quad-size2-${Date.now()}`;
const w2 = await openQuad(l2);
await wait(2000); // restoreWindowSize runs before panes
const size = await w2.evaluate(async () => {
  const s = await window.__TAURI__.window.getCurrentWindow().innerSize();
  return { w: s.width, h: s.height };
});
console.log("restored size:", size);
const ok = Math.abs(size.w - 1010) < 30 && Math.abs(size.h - 610) < 30;
console.log(ok ? "SIZE CHECK OK" : "SIZE CHECK FAILED (wanted ~1010x610)");

await catalog.evaluate(() => window.__TAURI__.core.invoke("close_all_player_windows"));
await browser.close();
process.exit(ok ? 0 : 3);
