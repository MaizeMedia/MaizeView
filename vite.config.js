import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";
import { resolve } from "node:path";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// Multi-page entry: each Tauri window loads only the bundle it needs.
// - catalog.html  → main library browser window
// - player.html   → one per open video (Phase 3+); present early so the entry exists
// - quad.html     → 4Play M1 spike (4 quadrant panes in one window)
const ROLLUP_INPUT = {
  catalog: resolve(__dirname, "catalog.html"),
  player: resolve(__dirname, "player.html"),
  quad: resolve(__dirname, "quad.html"),
};

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [tailwindcss(), svelte()],

  // Tauri expects a fixed port, fail if it is not available.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // Tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },

  build: {
    // Multiple entry points → one bundle per window type.
    rollupOptions: {
      input: ROLLUP_INPUT,
    },
  },

  resolve: {
    alias: {
      $lib: resolve(__dirname, "src/lib"),
      $components: resolve(__dirname, "src/lib/components"),
    },
  },
}));
