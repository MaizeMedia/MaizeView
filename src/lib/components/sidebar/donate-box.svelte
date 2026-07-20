<script lang="ts">
  // Quiet "Support" row at the very bottom of the sidebar. Opens a small
  // popover: user-clicked Patreon link plus offline QR codes + copyable
  // addresses — no network calls except explicit link clicks.
  //
  // QR is generated locally (qrcode package, pure JS) so nothing leaves the
  // machine. Add more coins by appending to ADDRESSES.
  import { Bitcoin, X, Copy, Check, Coins, Heart } from "@lucide/svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import QRCode from "qrcode";

  const PATREON_URL = "https://www.patreon.com/cw/MaizeMedia";

  interface DonationAddress {
    coin: string;
    /** URI scheme for the QR payload (wallet deep link). */
    scheme: "bitcoin" | "monero";
    address: string;
  }
  const ADDRESSES: DonationAddress[] = [
    { coin: "BTC", scheme: "bitcoin", address: "bc1qrfay5yku3alqzss4m585747wjnl8azas9092nx" },
    {
      coin: "XMR",
      scheme: "monero",
      address:
        "88gJCUAAZ3y3B66Zvbn41EaT9L3QGuRJqNK8Rax7un6y8s4t9qQmFbiD2G1Gy36cJp4hN8Q67eRhz3BG6SfFDcFk1rTxsfC",
    },
  ];

  let open = $state(false);
  let copied = $state<string | null>(null);
  let qrByCoin = $state<Record<string, string>>({});

  async function openDialog() {
    open = true;
    // Generate QR SVGs lazily on first open.
    for (const d of ADDRESSES) {
      if (!qrByCoin[d.coin]) {
        try {
          qrByCoin[d.coin] = await QRCode.toString(`${d.scheme}:${d.address}`, {
            type: "svg",
            margin: 1,
            width: 160,
            color: { dark: "#e4e4e7", light: "#00000000" },
          });
        } catch {
          // QR is a nicety; the copyable address is the fallback
        }
      }
    }
  }

  async function copyAddress(coin: string, address: string) {
    try {
      await navigator.clipboard.writeText(address);
      copied = coin;
      setTimeout(() => (copied = null), 1500);
    } catch {
      // clipboard unavailable — user can still select the text
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") open = false;
  }
</script>

<button
  type="button"
  onclick={() => void openDialog()}
  data-testid="donate-open"
  class="flex w-full items-center gap-3 rounded-md px-3 py-2 text-sm font-medium text-muted-foreground transition-colors hover:bg-accent hover:text-accent-foreground"
>
  <Bitcoin class="size-4" />
  Support
</button>

{#if open}
  <!-- Support popover. Backdrop click or Esc closes. -->
  <div
    class="fixed inset-0 z-[60] flex items-center justify-center bg-black/60"
    role="presentation"
    onclick={(e) => e.target === e.currentTarget && (open = false)}
    onkeydown={onKeydown}
  >
    <div
      class="w-80 rounded-lg border border-border bg-card p-4 shadow-xl"
      role="dialog"
      aria-label="Donate to MaizeView"
      data-testid="donate-dialog"
    >
      <div class="flex items-center justify-between">
        <h3 class="text-sm font-semibold">Support MaizeView</h3>
        <button
          type="button"
          class="rounded p-1 text-muted-foreground hover:bg-accent hover:text-accent-foreground"
          onclick={() => (open = false)}
          aria-label="Close"
        >
          <X class="size-4" />
        </button>
      </div>
      <p class="mt-1 text-xs text-muted-foreground">
        MaizeView is free and open source. If it's useful to you, a small
        donation keeps it going.
      </p>

      <button
        type="button"
        class="mt-3 flex w-full items-center justify-center gap-1.5 rounded-md border border-border bg-background/60 px-3 py-2 text-sm font-medium hover:bg-accent hover:text-accent-foreground"
        onclick={() => void openUrl(PATREON_URL)}
        data-testid="donate-patreon"
      >
        <Heart class="size-4 text-primary" />
        Become a patron on Patreon →
      </button>

      {#each ADDRESSES as d (d.coin)}
        <div class="mt-3 rounded-md border border-border bg-background/60 p-3">
          <div class="flex items-center gap-2 text-sm font-medium">
            {#if d.coin === "BTC"}
              <Bitcoin class="size-4 text-primary" />
            {:else}
              <Coins class="size-4 text-primary" />
            {/if}
            {d.coin}
          </div>
          {#if qrByCoin[d.coin]}
            <div class="mx-auto mt-2 w-40" data-testid="donate-qr">
              {@html qrByCoin[d.coin]}
            </div>
          {/if}
          <div class="mt-2 break-all rounded bg-black/40 px-2 py-1 font-mono text-[11px] text-zinc-300">
            {d.address}
          </div>
          <button
            type="button"
            class="mt-2 flex w-full items-center justify-center gap-1.5 rounded bg-zinc-800 px-2 py-1.5 text-xs hover:bg-zinc-700"
            onclick={() => void copyAddress(d.coin, d.address)}
            data-testid="donate-copy"
          >
            {#if copied === d.coin}
              <Check class="size-3.5 text-lime-400" /> Copied
            {:else}
              <Copy class="size-3.5" /> Copy {d.coin} address
            {/if}
          </button>
        </div>
      {/each}
    </div>
  </div>
{/if}
