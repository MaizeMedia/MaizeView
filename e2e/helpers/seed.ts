import type { Page } from "@playwright/test";
import { invokeCmd } from "./tauri";

interface ScanPathRow {
  id: string;
  path: string;
}

interface SceneCounts {
  total: number;
  favorites: number;
}

/** Clear scan paths and index the sandbox folder via real Tauri commands. */
export async function seedSandboxLibrary(
  page: Page,
  libPath: string,
  timeoutMs = Number(process.env.E2E_SCAN_TIMEOUT_MS ?? 900_000),
) {
  const existing = await invokeCmd<ScanPathRow[]>(page, "list_scan_paths");
  for (const row of existing) {
    await invokeCmd(page, "remove_scan_path", { id: row.id });
  }

  await invokeCmd(page, "add_scan_path", {
    args: { path: libPath, label: "E2E sandbox" },
  });
  await invokeCmd(page, "start_scan");

  const deadline = Date.now() + timeoutMs;
  let lastTotal = -1;
  let stableReads = 0;

  while (Date.now() < deadline) {
    const counts = await invokeCmd<SceneCounts>(page, "scene_counts");
    if (counts.total === lastTotal) stableReads += 1;
    else {
      stableReads = 0;
      lastTotal = counts.total;
    }
    if (counts.total > 0 && stableReads >= 3) return counts;
    await page.waitForTimeout(2000);
  }

  throw new Error(`Timed out waiting for sandbox scan to index ${libPath}`);
}
