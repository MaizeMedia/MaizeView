## New: Downscale / Convert

Reclaim disk space by downscaling selected videos to a lower resolution. From
the catalog's bulk-select bar, click **Convert…** to choose a target
resolution (1080p / 720p / 1440p) and control how each is handled:

- **Original file** — replace it (saves space) or keep both encodings.
- **Filename tokens** — rewrite resolution tokens in the filename
  (`4K` → `1080p`), remove them, or leave the filename alone.
- **Resolution tags** — swap the old tag for the target, remove it, or leave
  tags untouched.

The dialog shows a breakdown of the selection by current resolution, an
estimated space savings, and a before→after filename preview. A progress bar
with per-file percent and a cancel button tracks the run.

### Safety

The original file is never touched until its transcode is **verified** (video
stream present, output height ≤ target, duration within 2% of the source). Any
failure deletes the temporary file and leaves the original intact. Free-space
is checked before each transcode. See [ADR-015](../decisions.md) for the full
rationale.

### Details

- Automatic hardware-encoder selection (NVENC / QuickSync / AMF) with a
  software (libx264) fallback — and the picker validates the encoder actually
  works, not just that it's listed, so a stale GPU driver won't break a batch.
- After a transcode, previews and content hashes (oshash / pHash) are
  regenerated so search and duplicate detection stay accurate.
- Existing resolution search (`4K+` filter) and multi-select are reused —
  filter to your 4K library, **Select all**, then **Convert…**.

**Full stack:** Tauri 2 / Rust backend + Svelte 5 frontend. 37 Rust unit tests
pass, plus ignored integration tests that run a real ffmpeg transcode
end-to-end, and Playwright e2e coverage of the dialog.
