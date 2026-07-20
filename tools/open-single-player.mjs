// Open one scene in the normal single player (for quad comparison).
// Usage: node tools/open-single-player.mjs <sceneId>
import { chromium } from "@playwright/test";

const CDP = process.env.CDP_URL ?? "http://127.0.0.1:9222";
const sceneId = process.argv[2];
if (!sceneId) throw new Error("pass a sceneId");
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

const browser = await chromium.connectOverCDP(CDP);
const catalog = browser
  .contexts()
  .flatMap((c) => c.pages())
  .find((p) => p.url().includes("catalog.html"));
if (!catalog) throw new Error("catalog page not found");

await catalog.evaluate(async (sceneId) => {
  const invoke = window.__TAURI__.core.invoke;
  const file = await invoke("scene_file_path", { sceneId });
  if (!file) throw new Error("no playable file for scene " + sceneId);
  const { WebviewWindow } = window.__TAURI__.webviewWindow;
  const params = new URLSearchParams({ sceneId, file });
  new WebviewWindow(`player-${sceneId}`, {
    url: `player.html?${params}`,
    title: "MaizeView — Player",
    width: 960,
    height: 540,
    minWidth: 480,
    minHeight: 270,
    transparent: true,
  });
}, sceneId);
console.log("single player opening for", sceneId);
await wait(4000);
const labels = await catalog.evaluate(async () =>
  (await window.__TAURI__.webviewWindow.getAllWebviewWindows()).map((w) => w.label),
);
console.log("windows:", labels);
await browser.close();
