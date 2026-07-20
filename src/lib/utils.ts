import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

/**
 * Merge Tailwind classes intelligently (dedupes conflicting utilities).
 * Used by all shadcn-svelte components and our own.
 */
export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs));
}

/**
 * Tauri command errors arrive as objects (not Error instances), so String(e)
 * yields "[object Object]". Pull the message out properly for display.
 */
export function stringifyError(e: unknown): string {
  if (e == null) return "unknown error";
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  const anyErr = e as Record<string, unknown>;
  if (typeof anyErr.message === "string") return anyErr.message;
  try {
    return JSON.stringify(e);
  } catch {
    return String(e);
  }
}

/** Format seconds as m:ss, or h:mm:ss when ≥ 1 hour. Shared by both players. */
export function fmtTime(s: number): string {
  if (!isFinite(s) || s < 0) s = 0;
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = Math.floor(s % 60);
  return h > 0
    ? `${h}:${String(m).padStart(2, "0")}:${String(sec).padStart(2, "0")}`
    : `${m}:${String(sec).padStart(2, "0")}`;
}
