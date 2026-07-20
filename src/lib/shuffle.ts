/**
 * Weighted shuffle with favorite preference + anti-recency (ADR-010 evolved).
 *
 * - Base weight = max(1, favorite) — higher♥ still wins more often.
 * - Recently played titles are down-weighted so a short session doesn't
 *   keep redrawing the same handful after a pass resets.
 * - Never-played gets a mild boost.
 */

export interface ShuffleMeta {
  favorite: number;
  /** ISO timestamp or null if never played. */
  lastPlayedAt: string | null;
}

/** Ages (ms) → multipliers applied on top of favorite weight. */
const RECENCY_BANDS: { maxAgeMs: number; mult: number }[] = [
  { maxAgeMs: 60 * 60 * 1000, mult: 0.08 }, // < 1h
  { maxAgeMs: 6 * 60 * 60 * 1000, mult: 0.2 }, // < 6h
  { maxAgeMs: 24 * 60 * 60 * 1000, mult: 0.4 }, // < 1d
  { maxAgeMs: 7 * 24 * 60 * 60 * 1000, mult: 0.7 }, // < 7d
];

const NEVER_PLAYED_MULT = 1.35;
/** In-window cooldown after a pass resets (recently seen this session). */
const SESSION_COOLDOWN_MULT = 0.12;

export function recencyMultiplier(lastPlayedAt: string | null | undefined, nowMs = Date.now()): number {
  if (!lastPlayedAt) return NEVER_PLAYED_MULT;
  const t = Date.parse(lastPlayedAt);
  if (!Number.isFinite(t)) return 1;
  const age = Math.max(0, nowMs - t);
  for (const band of RECENCY_BANDS) {
    if (age < band.maxAgeMs) return band.mult;
  }
  return 1;
}

export function shuffleWeight(
  meta: ShuffleMeta | undefined,
  opts?: { sessionCooldown?: boolean; nowMs?: number },
): number {
  const favorite = Math.max(0, meta?.favorite ?? 0);
  let w = Math.max(1, favorite);
  w *= recencyMultiplier(meta?.lastPlayedAt ?? null, opts?.nowMs);
  if (opts?.sessionCooldown) w *= SESSION_COOLDOWN_MULT;
  // Floor so a pool of "all recently played" still has relative favorite odds.
  return Math.max(0.05, w);
}

/**
 * Weighted pick among `candidates`. Returns index into candidates, or -1 if empty.
 */
export function weightedPickIndex(
  candidates: string[],
  weightFor: (id: string) => number,
): number {
  if (candidates.length === 0) return -1;
  const weights = candidates.map((id) => Math.max(0.05, weightFor(id)));
  const total = weights.reduce((a, b) => a + b, 0);
  let r = Math.random() * total;
  for (let i = 0; i < candidates.length; i++) {
    r -= weights[i];
    if (r <= 0) return i;
  }
  return candidates.length - 1;
}

export function weightedPickId(
  candidates: string[],
  weightFor: (id: string) => number,
): string | null {
  const i = weightedPickIndex(candidates, weightFor);
  return i < 0 ? null : candidates[i];
}

/** How many recent plays to keep cool after a full-queue pass. */
export function sessionCooldownSize(queueLen: number): number {
  if (queueLen <= 1) return 0;
  return Math.min(40, Math.max(3, Math.floor(queueLen * 0.15)));
}
