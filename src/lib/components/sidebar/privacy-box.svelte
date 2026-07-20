<script lang="ts">
  // Collapsible privacy box above "Scan library" (user request): expanded by
  // default with a one-line privacy nudge + affiliate partner links; collapses
  // to a quiet "Privacy" row that stays one click away. State persists in
  // localStorage. Disclosure line is mandatory (FTC/ASA affiliate rules).
  //
  // Fill in real affiliate URLs after signing up with the providers — keep
  // the list short on purpose (one VPN, one seedbox) so it reads as a
  // recommendation, not an ad block.
  import { Shield, ChevronDown, ChevronUp } from "@lucide/svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";

  const LINKS: { label: string; url: string }[] = [
    { label: "PIA VPN", url: "https://www.privateinternetaccess.com/" },
    { label: "RapidSeedbox", url: "https://members.rapidseedbox.com/aff.php?aff=2279" },
  ];

  const KEY = "privacyBoxCollapsed";
  let collapsed = $state(localStorage.getItem(KEY) === "1");

  function toggle() {
    collapsed = !collapsed;
    localStorage.setItem(KEY, collapsed ? "1" : "0");
  }
</script>

{#if collapsed}
  <button
    type="button"
    onclick={toggle}
    data-testid="privacy-expand"
    title="Privacy tools"
    class="flex w-full items-center gap-3 rounded-md px-3 py-2 text-sm font-medium text-muted-foreground transition-colors hover:bg-accent hover:text-accent-foreground"
  >
    <Shield class="size-4" />
    Privacy
    <ChevronDown class="ml-auto size-3.5 opacity-60" />
  </button>
{:else}
  <div
    class="rounded-md border border-border bg-background/60 px-3 py-2 text-xs text-muted-foreground"
    data-testid="privacy-box"
  >
    <div class="flex items-center gap-1.5">
      <Shield class="size-3.5 shrink-0" />
      <span class="font-medium text-foreground/90">Your ISP can see everything.</span>
      <button
        type="button"
        onclick={toggle}
        title="Collapse"
        aria-label="Collapse privacy box"
        class="ml-auto rounded p-0.5 hover:bg-accent"
      >
        <ChevronUp class="size-3.5" />
      </button>
    </div>
    <p class="mt-1">Respect your privacy while supporting MaizeView with these affiliate links:</p>
    <div class="mt-1 flex flex-col gap-0.5">
      {#each LINKS as link (link.url)}
        <button
          type="button"
          class="text-left text-primary hover:underline"
          onclick={() => void openUrl(link.url)}
        >
          {link.label} →
        </button>
      {/each}
    </div>
  </div>
{/if}
