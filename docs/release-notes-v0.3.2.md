## 4Play — watch four videos at once

New **4Play** window: one window playing **four quadrant videos** with its own transport.

- **Open from:** playlist detail toolbar (≥4 playable items — honors the playlist's shuffle-by-default) or the library selection bar (any number of scenes).
- **Per-pane controls:** Prev/Next through the pane's play history, seek bar with **hover thumbnails** (scene sprite previews) and **live scrub**, click-to-solo audio with volume slider.
- **Global controls:** Pause all (Space), Next all (fresh scene on every pane), Close.
- **Rotation:** when a video ends, its pane automatically loads the next scene — in list order, or weighted (favorites × anti-recency, ADR-010) for shuffled playlists.
- Window size is remembered; chrome auto-hides after 3 s idle.
- Quadrant video holds position through window drags/resizes; window closes cleanly (no zombie panes).

## Faster background jobs

- **Previews & pHash ~1.5× faster per file** — previews spawned up to 38 ffmpeg processes per video (pHash: 25); both now use ONE ffmpeg process per video with byte-identical output (parity-tested against the old implementation).
- **Calmer post-scan:** the three background jobs (previews, MD5, pHash) now share one worker budget instead of up to 3× the configured parallelism.
- **MD5:** spinning disks get one full-file reader (concurrent reads thrash HDDs); SSDs stay parallel.
