import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const E2E_DIR = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
export const REPO_ROOT = path.resolve(E2E_DIR, "..");
export const E2E_DATA_DIR = path.join(E2E_DIR, ".data");
export const E2E_REPORTS_DIR = path.join(REPO_ROOT, "docs", "e2e-reports");
export const PID_FILE = path.join(E2E_DATA_DIR, "app.pid");
export const DEFAULT_DB_PATH = path.join(E2E_DATA_DIR, "maizeview.db");

/** Load `e2e/.env` into process.env (simple KEY=VALUE parser). */
export function loadE2eEnv() {
  const envFile = path.join(E2E_DIR, ".env");
  if (!fs.existsSync(envFile)) return;
  for (const line of fs.readFileSync(envFile, "utf8").split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;
    const eq = trimmed.indexOf("=");
    if (eq <= 0) continue;
    const key = trimmed.slice(0, eq).trim();
    let value = trimmed.slice(eq + 1).trim();
    if (
      (value.startsWith('"') && value.endsWith('"')) ||
      (value.startsWith("'") && value.endsWith("'"))
    ) {
      value = value.slice(1, -1);
    }
    if (!(key in process.env)) process.env[key] = value;
  }
}

export function cdpUrl(): string {
  return process.env.CDP_URL ?? "http://127.0.0.1:9222";
}

export function e2eDbPath(): string {
  return process.env.MAIZEVIEW_DB_PATH ?? DEFAULT_DB_PATH;
}

export function testLibPath(): string | null {
  const raw = process.env.MAIZEVIEW_TEST_LIB?.trim();
  if (!raw) return null;
  const resolved = path.resolve(raw);
  if (!fs.existsSync(resolved) || !fs.statSync(resolved).isDirectory()) {
    throw new Error(`MAIZEVIEW_TEST_LIB is not a directory: ${resolved}`);
  }
  return resolved;
}

export function ensureE2eDirs() {
  fs.mkdirSync(E2E_DATA_DIR, { recursive: true });
  fs.mkdirSync(E2E_REPORTS_DIR, { recursive: true });
}
