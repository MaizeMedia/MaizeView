import fs from "node:fs";
import path from "node:path";
import type { Page } from "@playwright/test";
import { E2E_REPORTS_DIR } from "./env";

/** Save a named screenshot under docs/e2e-reports/ for human review. */
export async function captureReport(page: Page, name: string) {
  fs.mkdirSync(E2E_REPORTS_DIR, { recursive: true });
  const safe = name.replace(/[^\w.-]+/g, "-");
  const file = path.join(E2E_REPORTS_DIR, `${safe}.png`);
  await page.screenshot({ path: file, fullPage: false });
  return file;
}
