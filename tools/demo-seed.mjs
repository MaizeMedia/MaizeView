// Seed the sandbox demo app: add the demo-lib scan path, start scan,
// then wait for scan + preview generation to complete.
import { chromium } from "@playwright/test";
import { tmpdir } from "node:os";
import { join } from "node:path";

const wait = (ms) => new Promise((r) => setTimeout(r, ms));
const DEMO_LIB = join(tmpdir(), "demo-lib");

const browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
const catalog = browser
  .contexts()
  .flatMap((c) => c.pages())
  .find((p) => p.url().includes("catalog.html"));
if (!catalog) throw new Error("no catalog page");

const invoke = (cmd, payload = {}) =>
  catalog.evaluate(
    async ({ cmd, payload }) => window.__TAURI__.core.invoke(cmd, payload),
    { cmd, payload },
  );

// 1. Add scan path (idempotent) + start scan.
const existing = await invoke("list_scan_paths");
if (!existing?.some((p) => p.path === DEMO_LIB)) {
  const added = await invoke("add_scan_path", { args: { path: DEMO_LIB, label: "Demo Footage" } });
  console.log("scan path added:", added?.path);
} else {
  console.log("scan path already present");
}
const job = await invoke("start_scan");
console.log("scan started:", job);

// 2. Wait for scan to finish (scene count reaches 20).
let scenes = 0;
for (let i = 0; i < 120; i++) {
  const counts = await invoke("scene_counts");
  scenes = counts?.total ?? 0;
  if (scenes >= 20) break;
  await wait(2000);
}
console.log("scenes indexed:", scenes);

// 3. Wait for preview generation (thumb_path on most scenes).
for (let i = 0; i < 180; i++) {
  const res = await invoke("list_scenes", {
    args: { page: 1, per_page: 50, sort: "title", view: "all" },
  });
  const withThumb = (res?.scenes ?? []).filter((s) => s.thumb_path).length;
  console.log(`previews: ${withThumb}/${scenes}`);
  if (withThumb >= scenes - 1) break;
  await wait(3000);
}
console.log("done — library is seeded with previews");
await browser.close();
