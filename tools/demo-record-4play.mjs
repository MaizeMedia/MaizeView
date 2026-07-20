// Record the 4Play demo GIF: stage the 4 full Blender films in a quad window,
// drive the UI (solo → hover seek thumbnail → pause/resume), and capture the
// window with ffmpeg gdigrab → looping GIF.
import { chromium } from "@playwright/test";
import { spawn, execFileSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";

// ffmpeg from PATH by default; override with the FFMPEG env var if needed.
const FFMPEG = process.env.FFMPEG ?? "ffmpeg";
const OUT_DIR = "docs/readme-assets";
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

const browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
const catalog = browser
  .contexts()
  .flatMap((c) => c.pages())
  .find((p) => p.url().includes("catalog.html"));
if (!catalog) throw new Error("no catalog page");

// 1. The four full films (exclude segments — filter on FILE PATH containing
//    " - Part ", since the title parser strips the year from those names).
const ids = await catalog.evaluate(async () => {
  const invoke = window.__TAURI__.core.invoke;
  const res = await invoke("list_scenes", {
    args: { page: 1, per_page: 50, sort: "title", view: "all" },
  });
  const out = [];
  for (const s of res?.scenes ?? []) {
    const path = await invoke("scene_file_path", { sceneId: s.id });
    if (path && !path.includes(" - Part ")) out.push(s.id);
    if (out.length === 4) break;
  }
  return out;
});
if (ids.length < 4) throw new Error("not enough full films: " + ids.length);
console.log("films staged:", ids.length);

// 2. Stage + open the quad window.
const label = `player-quad-demo-${Date.now()}`;
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

// 3. Find the quad page + wait for all four to play.
let quad = null;
for (let i = 0; i < 40 && !quad; i++) {
  for (const p of browser.contexts().flatMap((c) => c.pages()).filter((p) => p.url().includes("quad.html"))) {
    try {
      const l = await p.evaluate(() => window.__TAURI__?.webviewWindow?.getCurrentWebviewWindow?.().label ?? null);
      if (l === label) { quad = p; break; }
    } catch { /* booting */ }
  }
  if (!quad) await wait(500);
}
if (!quad) throw new Error("quad page never appeared");
await wait(6000); // panes init + claim + first frames

// 4. Window rect for gdigrab.
const rect = await quad.evaluate(async () => {
  const win = window.__TAURI__.window.getCurrentWindow();
  const pos = await win.outerPosition();
  const size = await win.outerSize();
  return { x: pos.x, y: pos.y, w: size.width, h: size.height };
});
console.log("recording rect:", rect);

// 5. Start gdigrab (12 s).
const raw = join(tmpdir(), "quad-demo-raw.mp4");
const gif = `${OUT_DIR}/4play-demo.gif`;
const rec = spawn(FFMPEG, [
  "-y", "-f", "gdigrab", "-framerate", "12",
  "-offset_x", String(Math.round(rect.x)), "-offset_y", String(Math.round(rect.y)),
  "-video_size", `${Math.round(rect.w)}x${Math.round(rect.h)}`,
  "-t", "12", "-i", "desktop",
  "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", raw,
], { stdio: "ignore" });

// 6. Drive the UI while it records.
await wait(1000);
await quad.mouse.move(320, 240); // poke controls
await wait(700);
// Solo Q1 (click its cell center)
const q1Seek = quad.getByLabel("Seek Q1");
const box = await q1Seek.boundingBox();
await quad.mouse.click(box.x + box.width * 0.4, box.y - 120);
await wait(1800);
// Hover the seek bar at ~55% → thumbnail bubble
await quad.mouse.move(box.x + box.width * 0.55, box.y + box.height / 2, { steps: 6 });
await wait(2500);
// Pause all → resume
await quad.getByRole("button", { name: "Pause all" }).click();
await wait(1400);
await quad.getByRole("button", { name: "Resume all" }).click();
await wait(2200);

// 7. Wait for recording to end, then make the GIF.
await new Promise((resolve) => rec.on("close", resolve));
execFileSync("node", ["-e", `require("fs").mkdirSync("${OUT_DIR}", { recursive: true })`]);
execFileSync(FFMPEG, [
  "-y", "-i", raw,
  "-vf", "fps=12,scale=880:-1:flags=lanczos,split[s0][s1];[s0]palettegen=stats_mode=full[p];[s1][p]paletteuse=dither=bayer:bayer_scale=3",
  "-loop", "0", gif,
]);
const { statSync } = await import("node:fs");
console.log("GIF written:", gif, `${(statSync(gif).size / 1e6).toFixed(2)} MB`);

// 8. Close the demo quad window.
await catalog.evaluate(() => window.__TAURI__.core.invoke("close_all_player_windows"));
await browser.close();
