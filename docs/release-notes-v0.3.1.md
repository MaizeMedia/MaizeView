## Convert performance & reliability

Fixes for **Convert / downscale** so NVIDIA GPUs actually get used, and decode can stay on the GPU when VRAM allows.

- **NVENC probe false-negative** — the encoder probe used a 64×64 test frame, below NVENC’s minimum size, so working GPUs (e.g. RTX 3090) always fell back to **CPU x264**. Probe is now 256×256.
- **CUDA decode + scale** — with NVENC, Convert can use `-hwaccel cuda` + `scale_cuda` so decode/scale/encode stay on the GPU. Progress label shows **NVENC+CUDA** when active.
- **VRAM gate** — CUDA decode only runs if `nvidia-smi` reports free VRAM ≥ a conservative estimate plus a **1 GiB** reserve. Otherwise: software decode + NVENC. If CUDA encode fails mid-file, one automatic software-decode retry.
- **WinGet FFmpeg Shared** — resolves `Gyan.FFmpeg.Shared` as well as Essentials when looking up ffmpeg next to the app / via WinGet.

Quality defaults unchanged (CQ 23, NVENC preset `p5`) — prefer fidelity over a faster preset.

## Also since v0.3.0

- Filename / path heuristics (`filename_parse`), identify term extraction, path match
- Stash-box needs-review UX; unlink / reject / ignore for false positives
- Title-search worthiness gate; pHash persistence fixes
