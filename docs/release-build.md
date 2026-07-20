# Building a MaizeView release (Windows)

How to produce the NSIS `.exe` + MSI `.msi` installers and publish a GitHub
release. Latest cut: **v0.3.1** on `main`; this is the runbook to turn a tag
into downloadable installers.

> **Why this guide exists:** early release builds could not be completed on
> the *dev laptop* — `rustc` crashed with `STATUS_ACCESS_VIOLATION` under the
> sustained load of the full dependency graph. The relaxed release profile +
> capped `CARGO_BUILD_JOBS` fixed that. Keep this runbook for future cuts.
> Latest: https://github.com/MaizeMedia/MaizeView/releases/tag/v0.3.1

## Prerequisites (one-time, on the build machine)

1. **Rust (MSVC toolchain):** `winget install Rustlang.Rustup`, then ensure
   `rustup default stable-x86_64-pc-windows-msvc`.
2. **VS 2022 Build Tools** with the *C++ build tools* workload:
   `winget install Microsoft.VisualStudio.2022.BuildTools`.
3. **Node.js LTS:** `winget install OpenJS.NodeJS.LTS`.
4. **FFmpeg** (needed at runtime by the transcode feature, and by integration
   tests): `winget install Gyan.FFmpeg`.
5. **GitHub CLI**, logged in: `winget install GitHub.cli` then `gh auth login`.

## Build steps

```powershell
git clone https://github.com/MaizeMedia/MaizeView.git
cd MaizeView
git checkout main
# Cap parallelism on this machine (see “If the build crashes” below).
$env:CARGO_BUILD_JOBS = "2"
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"

# Restore JS deps + gitignored libmpv DLLs (required to compile the Rust side).
npm install
npx tauri-plugin-libmpv-api setup-lib

# Build the installers (NSIS .exe + MSI .msi).
npm run tauri build
```

The build takes several minutes. When it finishes, the installers are at:

```
src-tauri\target\release\bundle\nsis\MaizeView_0.3.1_x64-setup.exe
src-tauri\target\release\bundle\msi\MaizeView_0.3.1_x64_en-US.msi
```

(Confirm the exact filenames with `dir src-tauri\target\release\bundle`.)

## Optional: verify before releasing

```powershell
cd src-tauri
cargo test --lib                      # unit tests
cargo test --test transcode_e2e -- --ignored   # real-ffmpeg transcode tests
cd ..
npm run build                         # frontend build (Svelte/Vite)
```

## Publish the GitHub release

```powershell
git tag v0.3.1
git push origin v0.3.1

gh release create v0.3.1 `
  src-tauri/target/release/bundle/nsis/MaizeView_0.3.1_x64-setup.exe `
  src-tauri/target/release/bundle/msi/MaizeView_0.3.1_x64_en-US.msi `
  --title "v0.3.1 — Convert NVENC / CUDA fixes" `
  --notes-file docs/release-notes-v0.3.1.md
```

That mirrors how v0.2.0 / v0.3.0 were shipped (NSIS + MSI assets on a GitHub release).

## If the build crashes

If `rustc` crashes with `STATUS_ACCESS_VIOLATION` on this machine too, it's the
same load-instability issue — not the code. Mitigations, in order:

- The release profile in `src-tauri/Cargo.toml` is already the light, stable
  variant (no LTO, `opt-level = 2`, `codegen-units = 16`). Don't restore LTO.
- Cap parallelism: `$env:CARGO_BUILD_JOBS=2` (PowerShell) before building.
- If it still crashes, the durable fix is to build in CI — add a GitHub
  Actions workflow on a Windows runner and stop building locally.
