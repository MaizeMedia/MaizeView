// Verify the empty-pane path: open a quad window with only 3 staged scenes
// (Q4 starts empty), click its "Load next", and assert Q4 starts playing.
import { chromium } from "@playwright/test";

const CDP = process.env.CDP_URL ?? "http://127.0.0.1:9222";
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

const browser = await chromium.connectOverCDP(CDP);
const catalog = browser
  .contexts()
  .flatMap((c) => c.pages())
  .find((p) => p.url().includes("catalog.html"));
if (!catalog) throw new Error("catalog page not found");

const ids = await catalog.evaluate(async () => {
  const invoke = window.__TAURI__.core.invoke;
  const playlists = await invoke("list_playlists");
  for (const p of playlists) {
    const items = await invoke("playlist_items", { playlistId: p.id });
    const playable = items.filter((s) => s.file_path);
    if (playable.length >= 3) return playable.slice(0, 3).map((s) => s.id);
  }
  return null;
});
if (!ids) throw new Error("no playlist with >=3 playable items");

const label = `player-quad-probe3-${Date.now()}`;
await catalog.evaluate(
  async ({ label, ids }) => {
    const invoke = window.__TAURI__.core.invoke;
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
  { label, ids },
);
console.log("opened 3-scene quad:", label);

let quad = null;
for (let i = 0; i < 30 && !quad; i++) {
  for (const p of browser.contexts().flatMap((c) => c.pages()).filter((p) => p.url().includes("quad.html"))) {
    try {
      const l = await p.evaluate(() => window.__TAURI__?.webviewWindow?.getCurrentWebviewWindow?.().label ?? null);
      if (l === label) { quad = p; break; }
    } catch { /* booting */ }
  }
  if (!quad) await wait(500);
}
if (!quad) throw new Error("quad page never appeared");

await wait(3000); // boot + 3 inits
const loadBtn = quad.getByLabel("Load next scene into Q4");
console.log("Load next visible:", (await loadBtn.count()) > 0);
await loadBtn.click();
await wait(3500);

const file = await quad.evaluate(
  async ({ label }) => {
    const invoke = window.__TAURI__.core.invoke;
    return invoke("plugin:libmpv|get_property", {
      name: "filename",
      format: "string",
      windowLabel: `${label}-q3`,
    });
  },
  { label },
);
console.log("Q4 after Load next:", file);
console.log(file ? "CHECK OK — empty pane loads and plays" : "CHECK FAILED — Q4 has no file");

await browser.close();
process.exit(file ? 0 : 3);
