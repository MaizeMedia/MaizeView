/** Accent presets — remaps CSS variables on documentElement (data-accent). */
export type AccentPresetId = "maize" | "teal" | "coral" | "slate" | "rose";

export interface AccentPreset {
  id: AccentPresetId;
  label: string;
  /** Preview swatch as CSS color */
  swatch: string;
}

export const ACCENT_PRESETS: AccentPreset[] = [
  { id: "maize", label: "Maize", swatch: "hsl(41 96% 56%)" },
  { id: "teal", label: "Teal", swatch: "hsl(173 58% 45%)" },
  { id: "coral", label: "Coral", swatch: "hsl(12 76% 58%)" },
  { id: "slate", label: "Slate", swatch: "hsl(210 18% 62%)" },
  { id: "rose", label: "Rose", swatch: "hsl(350 55% 58%)" },
];

export const DEFAULT_ACCENT: AccentPresetId = "maize";

export function isAccentPresetId(v: string): v is AccentPresetId {
  return ACCENT_PRESETS.some((p) => p.id === v);
}

/** Apply accent to the current document (catalog or player window). */
export function applyAccentPreset(id: string) {
  const preset = isAccentPresetId(id) ? id : DEFAULT_ACCENT;
  document.documentElement.dataset.accent = preset;
}
