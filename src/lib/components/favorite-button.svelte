<script lang="ts">
  import { Heart } from "@lucide/svelte";
  import { cycleFavoriteLevel } from "$lib/favorite";

  let {
    level,
    onChange,
    size = "md",
    variant = "default",
    class: className = "",
  }: {
    level: number;
    onChange: (next: number) => void | Promise<void>;
    size?: "sm" | "md";
    /** `overlay` = light text on dark video controls */
    variant?: "default" | "overlay";
    class?: string;
  } = $props();

  const iconClass = size === "sm" ? "size-3.5" : "size-4";
  const textClass = size === "sm" ? "text-[11px]" : "text-xs";

  const filled = $derived(level > 0);
  const fillColor = $derived(
    variant === "overlay"
      ? filled
        ? "hsl(var(--primary))"
        : "none"
      : filled
        ? "hsl(var(--primary))"
        : "none",
  );
  const strokeColor = $derived(
    variant === "overlay"
      ? filled
        ? "hsl(var(--primary))"
        : "currentColor"
      : filled
        ? "hsl(var(--primary))"
        : "currentColor",
  );
  const counterClass = $derived(
    variant === "overlay" ? "text-white" : "text-primary",
  );

  async function onClick(e: MouseEvent) {
    e.stopPropagation();
    await onChange(cycleFavoriteLevel(level));
  }

  async function onContextMenu(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    if (level > 0) await onChange(0);
  }
</script>

<button
  type="button"
  class="inline-flex items-center gap-0.5 {className}"
  aria-label={level > 0 ? `Favorite level ${level}` : "Add favorite"}
  title={level > 0
    ? `Favorite ${level} — click to cycle (5 wraps to 0), right-click to clear`
    : "Click to favorite (cycles 0–5)"}
  onclick={onClick}
  oncontextmenu={onContextMenu}
>
  <Heart
    class={iconClass}
    fill={fillColor}
    stroke={strokeColor}
    stroke-width="2"
  />
  {#if level > 0}
    <span class="{textClass} font-semibold tabular-nums {counterClass}">{level}</span>
  {/if}
</button>
