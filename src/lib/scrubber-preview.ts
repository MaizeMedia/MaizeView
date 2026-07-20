/** Matches preview sprite cells from src-tauri/src/previews.rs */
export const SCRUB_CELL_W = 160;
export const SCRUB_CELL_H = 90;

export interface ScrubberCue {
  start: number;
  end: number;
  x: number;
  y: number;
  w: number;
  h: number;
}

function parseTimestamp(raw: string): number {
  const m = raw.trim().match(/^(\d+):(\d+):(\d+)\.(\d+)$/);
  if (!m) return 0;
  return Number(m[1]) * 3600 + Number(m[2]) * 60 + Number(m[3]) + Number(m[4]) / 1000;
}

/** Parse WebVTT cues with `#xywh=` sprite fragments (MaizeView preview format). */
export function parseScrubberVtt(text: string): ScrubberCue[] {
  const cues: ScrubberCue[] = [];
  const lines = text.split(/\r?\n/);
  for (let i = 0; i < lines.length; i++) {
    const timing = lines[i].match(/(\d{2}:\d{2}:\d{2}\.\d+)\s*-->\s*(\d{2}:\d{2}:\d{2}\.\d+)/);
    if (!timing) continue;
    const payload = lines[i + 1]?.trim() ?? "";
    const xywh = payload.match(/#xywh=(\d+),(\d+),(\d+),(\d+)/);
    if (!xywh) continue;
    cues.push({
      start: parseTimestamp(timing[1]),
      end: parseTimestamp(timing[2]),
      x: Number(xywh[1]),
      y: Number(xywh[2]),
      w: Number(xywh[3]),
      h: Number(xywh[4]),
    });
  }
  return cues;
}

export function cueAtTime(cues: ScrubberCue[], t: number): ScrubberCue | null {
  if (cues.length === 0) return null;
  for (const c of cues) {
    if (t >= c.start && t < c.end) return c;
  }
  const last = cues[cues.length - 1];
  return t >= last.start ? last : cues[0];
}

export function spriteDimensions(cues: ScrubberCue[]): { width: number; height: number } {
  let width = SCRUB_CELL_W;
  let height = SCRUB_CELL_H;
  for (const c of cues) {
    width = Math.max(width, c.x + c.w);
    height = Math.max(height, c.y + c.h);
  }
  return { width, height };
}

/**
 * Preview thumbnail size scales with the player window.
 * ~20% of window width, clamped so small windows stay readable and large
 * windows don't dominate the frame. Always 16:9 to match sprite cells.
 */
export function scrubPreviewDisplaySize(windowWidth: number): { width: number; height: number } {
  const width = Math.round(Math.min(480, Math.max(160, windowWidth * 0.2)));
  const height = Math.round((width * 9) / 16);
  return { width, height };
}

export function scrubPreviewStyle(
  spriteUrl: string,
  cue: ScrubberCue,
  spriteW: number,
  spriteH: number,
  displayW: number,
  displayH: number,
): string {
  const scaleX = displayW / cue.w;
  const scaleY = displayH / cue.h;
  return [
    `width:${displayW}px`,
    `height:${displayH}px`,
    `background-image:url("${spriteUrl}")`,
    `background-size:${spriteW * scaleX}px ${spriteH * scaleY}px`,
    `background-position:${-cue.x * scaleX}px ${-cue.y * scaleY}px`,
    `background-repeat:no-repeat`,
  ].join(";");
}
