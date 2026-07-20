// 4Play M1 live probe — drives the running dev app over CDP (port 9222).
//
// What it does:
//   1. Finds the catalog window, picks the first playlist with >=4 playable
//      items (same eligibility rule as the 4Play button).
//   2. Stages a queue + opens a quad window exactly like openQuad() does.
//   3. Attaches to the quad page, streams console/pageerrors, and polls the
//      on-screen status text until "playing" or "error".
//   4. Polls mpv time-pos via plugin:libmpv to prove decoding is advancing.
//
// Usage: node tools/quad-probe.mjs   (app must already run with CDP enabled)
import { chromium } from "@playwright/test";

const CDP = process.env.CDP_URL ?? "http://127.0.0.1:9222";
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

async function connect() {
  for (let i = 0; i < 45; i++) {
    try {
      return await chromium.connectOverCDP(CDP);
    } catch {
      await wait(1000);
    }
  }
  throw new Error(`cannot connect to ${CDP} — is the app running with remote debugging?`);
}

async function findPage(browser, needle, tries = 30) {
  for (let i = 0; i < tries; i++) {
    const pages = browser.contexts().flatMap((c) => c.pages());
    const hit = pages.find((p) => p.url().includes(needle));
    if (hit) return hit;
    await wait(500);
  }
  return null;
}

const browser = await connect();
console.log("[probe] connected to CDP");

const catalog = await findPage(browser, "catalog.html");
if (!catalog) throw new Error("catalog page not found");
console.log("[probe] catalog:", catalog.url());

// 1. Pick a playlist with >=4 playable items (mirrors playableQuadCount rule).
const plan = await catalog.evaluate(async () => {
  const invoke = window.__TAURI__.core.invoke;
  const playlists = await invoke("list_playlists");
  for (const p of playlists) {
    const items = await invoke("playlist_items", { playlistId: p.id });
    const playable = items.filter((s) => s.file_path);
    if (playable.length >= 4) {
      return {
        name: p.name,
        total: items.length,
        // Stage 6 so the EOF-rotation check has scenes to advance to.
        sceneIds: playable.slice(0, 6).map((s) => s.id),
      };
    }
  }
  return null;
});
if (!plan) {
  console.error("[probe] no playlist with >=4 playable items — 4Play button would be disabled too");
  process.exit(2);
}
console.log(`[probe] playlist "${plan.name}" (${plan.total} items), first 4 playable:`, plan.sceneIds);

// 2. Stage + open the quad window — same calls as openQuad() / openQuadWindow().
// SHUFFLE=1 exercises the weighted (shuffle_by_default) rotation path.
const label = `player-quad-probe-${Date.now()}`;
const shuffle = process.env.SHUFFLE === "1";
await catalog.evaluate(
  async ({ label, ids, shuffle }) => {
    const invoke = window.__TAURI__.core.invoke;
    await invoke("stage_player_queue", {
      label,
      sceneIds: ids,
      startIndex: 0,
      shuffleByDefault: shuffle,
    });
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
  { label, ids: plan.sceneIds, shuffle },
);
console.log("[probe] quad window opening:", label);

// 3. Attach to OUR quad page (older quad windows may still be open — match
//    by Tauri window label, not just URL).
let quad = null;
for (let i = 0; i < 30 && !quad; i++) {
  const pages = browser
    .contexts()
    .flatMap((c) => c.pages())
    .filter((p) => p.url().includes("quad.html"));
  for (const p of pages) {
    try {
      const pageLabel = await p.evaluate(
        () => window.__TAURI__?.webviewWindow?.getCurrentWebviewWindow?.().label ?? null,
      );
      if (pageLabel === label) {
        quad = p;
        break;
      }
    } catch {
      // page still booting — retry next pass
    }
  }
  if (!quad) await wait(500);
}
if (!quad) throw new Error("our quad page never appeared in CDP targets");
quad.on("console", (m) => console.log("[quad console]", m.type(), m.text()));
quad.on("pageerror", (e) => console.log("[quad pageerror]", String(e)));
console.log("[probe] attached to quad page");

// 4. Poll the on-window status text until success/error (max ~45 s).
// M2: the bar shows "Q1:playing  Q2:playing  …" — success = all 4 playing.
let last = "";
let outcome = "timeout";
for (let i = 0; i < 90; i++) {
  const txt = await quad
    .evaluate(() => document.body?.innerText ?? "")
    .catch(() => "");
  const oneLine = txt.replace(/\s+/g, " ").trim();
  if (oneLine && oneLine !== last) {
    console.log(`[status ${(i / 2).toFixed(1)}s]`, oneLine.slice(0, 400));
    last = oneLine;
  }
  if (/error —|no staged queue/i.test(oneLine)) {
    outcome = "error";
    break;
  }
  const playingCount = (oneLine.match(/:playing/g) ?? []).length;
  if (playingCount >= 4) {
    outcome = "playing";
    break;
  }
  await wait(500);
}
console.log("[probe] status outcome:", outcome);

// 5. Prove all four instances decode: sample each instance's time-pos twice.
if (outcome === "playing") {
  const instanceLabels = [0, 1, 2, 3].map((i) => `${label}-q${i}`);
  const sample = () =>
    quad.evaluate(async (labels) => {
      const invoke = window.__TAURI__.core.invoke;
      const out = [];
      for (const windowLabel of labels) {
        try {
          out.push(
            await invoke("plugin:libmpv|get_property", {
              name: "time-pos",
              format: "double",
              windowLabel,
            }),
          );
        } catch (e) {
          out.push(`err: ${e}`);
        }
      }
      return out;
    }, instanceLabels);
  const t0 = await sample();
  await wait(1200);
  const t1 = await sample();
  console.log("[mpv time-pos t0]", t0);
  console.log("[mpv time-pos t1]", t1);
  const advancing = t0.every(
    (v, i) => typeof v === "number" && typeof t1[i] === "number" && t1[i] > v,
  );
  console.log("[probe] all 4 instances advancing:", advancing);
  if (!advancing) outcome = "not-advancing";
}

// 6. EOF rotation: seek q0 to just before the end; it should advance to the
//    5th staged scene (filename changes, time-pos resets low).
if (outcome === "playing" && plan.sceneIds.length > 4) {
  const q0 = `${label}-q0`;
  const read = (name, format) =>
    quad.evaluate(
      async ({ name, format, q0 }) => {
        const invoke = window.__TAURI__.core.invoke;
        return invoke("plugin:libmpv|get_property", { name, format, windowLabel: q0 });
      },
      { name, format, q0 },
    );
  const before = await read("filename", "string");
  const dur = await read("duration", "double");
  console.log("[rotate] q0 before:", before, "duration:", dur);
  if (typeof dur === "number" && dur > 3) {
    await quad.evaluate(
      async ({ q0, pos }) => {
        const invoke = window.__TAURI__.core.invoke;
        await invoke("plugin:libmpv|set_property", { name: "time-pos", value: pos, windowLabel: q0 });
      },
      { q0, pos: dur - 1.5 },
    );
    await wait(7000); // EOF + 2s poll interval + load time
    const after = await read("filename", "string");
    const pos1 = await read("time-pos", "double");
    await wait(1200);
    const pos2 = await read("time-pos", "double");
    console.log("[rotate] q0 after:", after, "time-pos:", pos1, "->", pos2);
    const rotated = Boolean(after && before && after !== before);
    // Regression: rotation must not leave the new file paused (keep-open
    // leaves pause=yes at EOF; it persisted across loadfile).
    const advancingAfter = typeof pos1 === "number" && typeof pos2 === "number" && pos2 > pos1;
    console.log("[probe] EOF rotation:", rotated ? "OK" : "FAILED", "| advancing after:", advancingAfter);
    if (!rotated) outcome = "no-rotation";
    else if (!advancingAfter) outcome = "rotates-paused";
  } else {
    console.log("[rotate] skipped — duration unreadable");
  }
}

// 7. Nav: Next/Prev on Q2.
if (outcome === "playing") {
  const readProp = (inst, name, format) =>
    quad.evaluate(
      async ({ inst, name, format }) => {
        const invoke = window.__TAURI__.core.invoke;
        return invoke("plugin:libmpv|get_property", { name, format, windowLabel: inst });
      },
      { inst, name, format },
    );

  const q1 = `${label}-q1`;
  const before1 = await readProp(q1, "filename", "string");
  await quad.getByLabel("Next scene Q2").click();
  await wait(2500);
  const next1 = await readProp(q1, "filename", "string");
  await quad.getByLabel("Previous scene Q2").click();
  await wait(2500);
  const back1 = await readProp(q1, "filename", "string");
  console.log("[nav] Q2 next:", next1);
  console.log("[nav] Q2 prev:", back1, "(want:", before1, ")");
  const navOk = Boolean(before1 && next1 && back1 && before1 !== next1 && back1 === before1);
  console.log("[nav] next/prev:", navOk ? "OK" : "FAILED");
  if (!navOk) outcome = "nav-failed";
}

// 8. Final window inventory.
const labels = await catalog.evaluate(async () => {
  const all = await window.__TAURI__.webviewWindow.getAllWebviewWindows();
  return all.map((w) => w.label);
});
console.log("[probe] windows:", labels);

await browser.close();
console.log("[probe] done — outcome:", outcome);
process.exit(outcome === "playing" ? 0 : 3);
