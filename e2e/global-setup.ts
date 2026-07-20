import { chromium } from "@playwright/test";
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import {
  REPO_ROOT,
  PID_FILE,
  cdpUrl,
  e2eDbPath,
  ensureE2eDirs,
  loadE2eEnv,
} from "./helpers/env";

async function cdpReady(): Promise<boolean> {
  try {
    const browser = await chromium.connectOverCDP(cdpUrl());
    await browser.close();
    return true;
  } catch {
    return false;
  }
}

export default async function globalSetup() {
  loadE2eEnv();
  ensureE2eDirs();

  if (await cdpReady()) {
    console.log(`[e2e] CDP ready at ${cdpUrl()} — using running app`);
    return;
  }

  if (process.env.E2E_AUTO_START !== "1") {
    throw new Error(
      [
        "MaizeView is not running with CDP enabled.",
        "Start it in another terminal: npm run e2e:app",
        "Or set E2E_AUTO_START=1 to launch automatically from Playwright.",
      ].join("\n"),
    );
  }

  const dbPath = e2eDbPath();
  fs.mkdirSync(path.dirname(dbPath), { recursive: true });

  const env = {
    ...process.env,
    MAIZEVIEW_DB_PATH: dbPath,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:
      process.env.WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS ?? "--remote-debugging-port=9222",
  };

  console.log(`[e2e] Launching MaizeView (db: ${dbPath})`);
  const child = spawn("npm run tauri dev", {
    cwd: REPO_ROOT,
    env,
    shell: true,
    detached: true,
    stdio: "ignore",
  });
  child.unref();
  if (child.pid) fs.writeFileSync(PID_FILE, String(child.pid));

  const deadline = Date.now() + 180_000;
  while (Date.now() < deadline) {
    if (await cdpReady()) {
      console.log("[e2e] App ready on CDP");
      await new Promise((r) => setTimeout(r, 4000));
      return;
    }
    await new Promise((r) => setTimeout(r, 2000));
  }

  throw new Error(`Timed out waiting for CDP at ${cdpUrl()}`);
}
