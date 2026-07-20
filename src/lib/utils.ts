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
