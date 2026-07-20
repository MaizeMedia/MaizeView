import fs from "node:fs";
import { execSync } from "node:child_process";
import { disconnectCdpBrowser } from "./fixtures/catalog";
import { PID_FILE, loadE2eEnv } from "./helpers/env";

export default async function globalTeardown() {
  loadE2eEnv();
  await disconnectCdpBrowser();

  if (process.env.E2E_AUTO_START !== "1") return;
  if (!fs.existsSync(PID_FILE)) return;

  const pid = Number(fs.readFileSync(PID_FILE, "utf8").trim());
  fs.unlinkSync(PID_FILE);
  if (!pid) return;

  try {
    if (process.platform === "win32") {
      execSync(`taskkill /PID ${pid} /T /F`, { stdio: "ignore" });
    } else {
      process.kill(pid, "SIGTERM");
    }
    console.log(`[e2e] Stopped app pid ${pid}`);
  } catch {
    // Process may already be gone.
  }
}
