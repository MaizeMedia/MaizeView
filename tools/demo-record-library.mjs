// Record the library demo GIF: grid of 20 previewed scenes, scroll, hover,
// search filter — captured with ffmpeg gdigrab → looping GIF.
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

await catalog.getByTestId("nav-library").click().catch(() => {});
await wait(1500);

const rect = await catalog.evaluate(async () => {
  const win = window.__TAURI__.window.getCurrentWindow();
  const pos = await win.outerPosition();
  const size = await win.outerSize();
  return { x: pos.x, y: pos.y, w: size.width, h: size.height };
});
console.log("recording rect:", rect);

const raw = join(tmpdir(), "library-demo-raw.mp4");
const gif = `${OUT_DIR}/library-demo.gif`;
const rec = spawn(FFMPEG, [
  "-y", "-f", "gdigrab", "-framerate", "10",
  "-offset_x", String(Math.round(rect.x)), "-offset_y", String(Math.round(rect.y)),
  "-video_size", `${Math.round(rect.w)}x${Math.round(rect.h)}`,
  "-t", "11", "-i", "desktop",
  "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", raw,
], { stdio: "ignore" });

await wait(800);
// Scroll the grid down slowly, then back up.
for (let i = 0; i < 4; i++) {
  await catalog.mouse.wheel(0, 400);
  await wait(450);
}
for (let i = 0; i < 4; i++) {
  await catalog.mouse.wheel(0, -400);
  await wait(450);
}
// Search for "sintel" — grid filters live.
const search = catalog.getByTestId("catalog-search");
await search.click();
await search.pressSequentially("sintel", { delay: 90 });
await wait(2200);
// Clear it again.
await search.fill("");
await wait(1800);

await new Promise((resolve) => rec.on("close", resolve));
execFileSync("node", ["-e", `require("fs").mkdirSync("${OUT_DIR}", { recursive: true })`]);
execFileSync(FFMPEG, [
  "-y", "-i", raw,
  "-vf", "fps=10,scale=640:-1:flags=lanczos,split[s0][s1];[s0]palettegen=stats_mode=full[p];[s1][p]paletteuse=dither=bayer:bayer_scale=3",
  "-loop", "0", gif,
]);
const { statSync } = await import("node:fs");
console.log("GIF written:", gif, `${(statSync(gif).size / 1e6).toFixed(2)} MB`);
await browser.close();
