# Dev Environment Setup

> What's installed and how to develop. Reference for any future session.

## Required toolchain (all installed and verified 2026-07-05)

| Tool | Version | Notes |
|---|---|---|
| Rust (stable, MSVC) | 1.96.1 | `~/.cargo/`. Binaries in `~/.cargo/bin/`. |
| cargo | 1.96.1 | `~/.cargo/bin/cargo.exe` |
| Node.js | 25.7.0 | system |
| npm | 11.10.1 | system |
| Git | 2.52.0 | system |
| FFmpeg / FFprobe | 8.1.2 (gyan.dev full) | `$HOME/AppData/Local/Microsoft/WinGet/Packages/Gyan.FFmpeg_*/ffmpeg-8.1.2-full_build/bin/` |
| MSVC Build Tools | 2019 | `C:/Program Files (x86)/Microsoft Visual Studio/2019/BuildTools/` |
| Windows SDK | 10 | `C:/Program Files (x86)/Windows Kits/10/` |
| WebView2 Runtime | 149.x | system (Tauri requirement) |
| libmpv (mpv dev) | `libmpv-2.dll` (zhongfly build, `mpv-dev-lgpl-x86_64`, ~94 MB) | Phase 3 playback. Bundled in `src-tauri/lib/`. See [libmpv setup](#libmpv-setup-phase-3-playback) below. |
| libmpv-wrapper | `libmpv-wrapper.dll` (nini22P build, ~383 KB) | Plugin ↔ libmpv glue DLL. Bundled in `src-tauri/lib/`. |

## Tauri plugins in use

- `tauri-plugin-opener` — link/file opening (template default)
- `tauri-plugin-dialog` — native folder picker for adding scan paths (added Phase 1)
- `tauri-plugin-libmpv` (v0.3.2) — embeds mpv in a window via libmpv; one instance per player window (added Phase 3, ADR-012). JS API: `tauri-plugin-libmpv-api`.

## PATH gotchas in this Git Bash session

`~/.cargo/bin` and the FFmpeg folder are **not** on the inherited shell PATH. Use these prefixes when running in this session:

```bash
# Rust / cargo
export PATH="$HOME/.cargo/bin:$PATH"

# FFmpeg / ffprobe (path may vary slightly by version folder)
FF_BIN="$HOME/AppData/Local/Microsoft/WinGet/Packages/Gyan.FFmpeg_Microsoft.Winget.Source_8wekyb3d8bbwe/ffmpeg-8.1.2-full_build/bin"
export PATH="$FF_BIN:$PATH"
```

> **Note:** Tauri at runtime uses the *system* PATH (not this shell's), so the built app finds ffmpeg/cargo fine. These prefixes are only needed for build/verification commands run from this terminal.

## Dev workflow

```bash
cd C:/Projects/MaizeView
npm install            # first time only
npm run tauri dev      # run the app (launches catalog window)
npm run build          # build frontend only (verifies Vite)
npm run check          # svelte-check (type-check .svelte files)
```

For Rust checks:
```bash
cd src-tauri
PATH="$HOME/.cargo/bin:$PATH" cargo check
```

### ⚠️ Rebuild-lock gotcha

The dev app holds `target/debug/maizeview.exe` open. Before any cargo/tauri build after the app has run, kill it (and stale Vite on the dev port) or cargo fails with "Access is denied (os error 5)":

```bash
taskkill //F //IM maizeview.exe
# plus any stale vite holding the port:
for pid in $(netstat -ano | grep ":1420 " | grep LISTENING | awk '{print $5}'); do taskkill //F //PID $pid; done
```

## Integration tests

Tests live in `src-tauri/tests/scanner_e2e.rs`. They require the test video library and ffprobe on PATH.

**Test library:** generated at `~/maizeview-test-lib/` (3 tiny ffmpeg-generated MP4s: red 3s, green 5s, blue 2s nested in `subdir/`, plus a non-video `readme.txt` to confirm filtering). To regenerate:

```bash
FF_BIN="$HOME/AppData/Local/Microsoft/WinGet/Packages/Gyan.FFmpeg_Microsoft.Winget.Source_8wekyb3d8bbwe/ffmpeg-8.1.2-full_build/bin"
mkdir -p ~/maizeview-test-lib/subdir
"$FF_BIN/ffmpeg.exe" -y -f lavfi -i color=c=red:s=320x240:d=3 -pix_fmt yuv420p ~/maizeview-test-lib/red-scene.mp4
"$FF_BIN/ffmpeg.exe" -f lavfi -i color=c=green:s=640x360:d=5 -pix_fmt yuv420p ~/maizeview-test-lib/green-scene.mkv
"$FF_BIN/ffmpeg.exe" -f lavfi -i color=c=blue:s=1280x720:d=2 -pix_fmt yuv420p ~/maizeview-test-lib/subdir/blue-scene.mp4
echo "not a video" > ~/maizeview-test-lib/readme.txt
```

**Run:**
```bash
cd src-tauri
PATH="$HOME/.cargo/bin:$PATH" cargo test --test scanner_e2e -- --ignored
```

(`--ignored` because the tests are marked `#[ignore]` — they need the test lib + ffprobe.) Override the test-lib location with the `MAIZEVIEW_TEST_LIB` env var if needed.

Current tests:
- `scan_indexes_test_library` — full pipeline + idempotency + preview gen validation.
- `scan_cancel_keeps_partials` — cancel keeps partials, resume reaches full count.

## libmpv setup (Phase 3 playback)

The player needs two DLLs in `src-tauri/lib/`:
- **`libmpv-2.dll`** — the actual mpv core (~94 MB; from [zhongfly/mpv-winbuild](https://github.com/zhongfly/mpv-winbuild/releases), `mpv-dev-lgpl-x86_64-*.7z`).
- **`libmpv-wrapper.dll`** — the plugin↔libmpv glue (~383 KB; from [nini22P/libmpv-wrapper](https://github.com/nini22P/libmpv-wrapper/releases), `libmpv-wrapper-windows-x86_64.zip`).

Both are **already fetched** into `src-tauri/lib/`. To re-fetch (e.g. on a fresh clone or to upgrade):

```bash
npx tauri-plugin-libmpv-api setup-lib
```

That script auto-detects Windows x86_64, downloads both artifacts, extracts the DLLs into `src-tauri/lib/`, and cleans up. (It uses `7z-wasm` for the `.7z` and a plain unzip for the wrapper.)

**Bundling:** `tauri.conf.json` → `bundle.resources: ["lib/**/*"]` ships both DLLs next to the exe. For `tauri dev` they're loaded directly from `src-tauri/lib/` via the build script.

**Manual alternative** (if the script is unavailable): download the two archives from the GitHub release links above, extract `libmpv-2.dll` and `libmpv-wrapper.dll`, and drop both into `src-tauri/lib/`.

> ⚠️ Use the **LGPL** (`mpv-dev-lgpl-*`) build, not the `v3` variant, unless you're certain your CPU supports it. The setup script picks the right one automatically.

## App data locations

| What | Where |
|---|---|
| SQLite DB | `%APPDATA%/MaizeView/maizeview.db` (i.e. `%APPDATA%\MaizeView\`) |
| Preview sprites + VTT | `%LOCALAPPDATA%/MaizeView/previews/` (i.e. `%LOCALAPPDATA%\MaizeView\previews\`) |

To reset the dev DB (e.g. after a schema change — migrations are mutable in place pre-v1.0):
```bash
rm -f ~/AppData/Roaming/MaizeView/maizeview.db*
rm -rf ~/AppData/Local/MaizeView/previews
```

## Adding shadcn-svelte components

```bash
npx shadcn-svelte@latest add <component> --yes
```
Note: the CLI prompts interactively if a component already exists. To avoid the hang, pipe an answer: `printf 'n\n' | npx shadcn-svelte@latest add ...` (declines overwrite). Components land in `src/lib/components/ui/`.

Currently installed: `button`, `input`, `separator`, `tooltip`.

## E2E testing (Playwright)

See **[`e2e.md`](./e2e.md)** for the full guide. Quick version:

```powershell
# Terminal A
npm run e2e:app

# Terminal B (after configuring e2e/.env with MAIZEVIEW_TEST_LIB)
npm run test:e2e:smoke
```

Uses an isolated DB at `e2e/.data/maizeview.db` and CDP on port 9222. Review screenshots in `docs/e2e-reports/`.
