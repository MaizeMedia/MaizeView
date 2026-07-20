# Implementation Plan

> **Status:** Approved. Phases 0–4 complete for core product use. Phase 5 polish partially done; faces/AI stretch deferred (plan before build).
> **Last updated:** 2026-07-12 (search gates + saved filters + FTS)

This is the authoritative phased plan. Each phase has a clear outcome and the next phase is gated on the previous one's outcome.

## Goal

A Windows desktop app that:
1. Scans local folders and catalogs video files.
2. Hashes, fingerprints, and tags them (manual + auto via metadata servers).
3. Lets you browse/search/filter/favorite/playlist them.
4. Plays them with **multiple simultaneous native-OS-window players**, each with custom overlay controls, hardware-accelerated.

## Non-goals (explicitly deferred)
- Cross-platform (Windows only for now).
- Cloud / sync / multi-user.
- A full scraper framework for arbitrary sites (we lean on metadata servers; CommunityScrapers zoo deprioritized).
- **AI / vision / face tagging** — stretch only; requires a written plan (GPU, privacy, review UX) before implementation. Manual timed performer tagging is **not** a substitute.
- **Automatic on-disk file moves / multi-drive “organize library”** — catalog organizes virtually (filters, playlists, tags). Physical relocation across full drives is a separate product if ever scoped (ADR-013).

---

## Metadata strategy (post Tier 1)

Agreed direction after Stash / Cove extension review (2026-07-11):

1. **Deepen identify** — ✅ stash-box fingerprint matching, review-first UX; **waterfall** across keyed boxes (Settings toggle).
2. **Local enrichment** — ✅ path match + embedded tags + local Stash DB import (Settings). **Parse only — never relocate files** (ADR-013).
3. **Segments / bookmarks** — ✅ timed bookmarks + labels (player + drawer + search). **No** manual performer-link UI; StashDB is scene-level only. Schema hooks kept for future AI/import.
4. **Deprioritize** — CommunityScrapers zoo, IAFD-as-core, downloaders, Cove extension host, Patreon AI server dependency.
5. **UI glaze** — ✅ accent presets in Settings (Cove-inspired, low effort); dark-first.
6. **Catalog hygiene** — ✅ prune fileless / missing-on-disk rows; playlist delete; protect offline scan roots.

---

## Metadata sources catalog

Sources to **consider** for enrichment, beyond what is built today. Each needs a short spike: API/ToS, match rate on our library, provenance model, and whether results are scene-level or time-coded.

### Tier 1 — Stash-box GraphQL (same integration pattern)

| Source | Status | Notes |
|--------|--------|-------|
| **StashDB** | ✅ Implemented | Fingerprints + title search; batch/library identify |
| **ThePornDB** | ✅ Implemented | Switchable stash-box preset (Settings); same GraphQL client |
| **FansDB** | ✅ Implemented | Switchable stash-box preset; per-box API keys |
| **JAVStash** | ✅ Implemented | Switchable stash-box preset; JAV-focused instance |

### Tier 2 — Other structured / semi-structured

| Source | Status | Notes |
|--------|--------|-------|
| **IAFD** | Consider | Strong US performer/studio/title catalog; likely scrape or third-party API, not Stash-box |
| **Filename / path heuristics** | ✅ Tuned on real-world libraries (2026-07) | `filename_parse`: scan titles, identify term extract, path match (ancestor folders + media-bucket skip, tag tokens, create-new review), Settings batch path-match (existing catalog). **Episode rule:** folder of mostly bare-number stems → numbers are episodes of the parent series/studio (`Parent - NNN` titles today; later identify = studio+episode, not digit-only search). Optional leftover: folder facet filter. Parse only — ADR-013 |
| **Embedded file metadata** | ✅ Implemented | ffprobe title/artist/comment; scan seeds title; drawer Read tags |
| **Local Stash app export** | ✅ Implemented | Settings → Import Stash database (scenes/tags/performers/studios by oshash/phash/size+duration) |
| **Community lists / CSV** | Consider | User-provided mapping files (hash → tags); good for one-off bulk fixes |
| **On-disk renamer / multi-drive organizer** | Non-goal | See ADR-013 — requires capacity planning + move journal; not metadata work |

### Tier 3 — Scrapers & extensions (higher maintenance)

| Source | Status | Notes |
|--------|--------|-------|
| **Site-specific scrapers** | Consider | Reuse/adapt Stash scraper YAML or Cove extension patterns per studio/site |
| **Cove extensions** | Reference only | Cove's extension model (downloaders, scrapers, AI jobs) is a design reference — not a runtime dependency |

### Tier 4 — AI & local vision *(stretch — plan before code)*

Inspired in part by **Cove** ([yourcove/cove](https://github.com/yourcove/cove)): metadata-server + **AI extension** acquisition, **per-occurrence tagging** (tag applies to a person or moment inside a scene, not the whole file), **segments/sub-scenes**, and **face-linked** organization (pgvector). **Not started** as of v0.2.0 checkpoint — needs a dedicated design pass (ADR-014).

| Capability | Status | Notes |
|------------|--------|-------|
| **Manual time-segment bookmarks** | ✅ Phase 4.5 | Labels + jump-to + search; `performer_id`/`tag_id` columns reserved — **no** chip UI yet |
| **Frame/sample tagging** | Stretch | Extract N frames → local vision → suggested tags; **human review before apply** |
| **Per-occurrence tags** | Stretch | Cove-style: tag attached to a time range + optional performer |
| **Face / embedding similarity** | Stretch | Local ONNX + vector index; privacy-sensitive, opt-in, on-device |
| **Speech / transcript search** | Stretch | Optional Whisper-style local transcript → FTS; time-coded quotes |

**Design constraints for any AI work:** local-first by default, explicit opt-in, provenance on every applied field (`ai_vision`, `ai_whisper`, etc.), review UI for auto-suggestions (never silent bulk overwrite), and GPU cost sanity (batch overnight, not on every scan).

**Reference:** study Cove's extension API and segment model when scoping; do not fork their stack (.NET + Postgres + pgvector) — adapt ideas to our SQLite + Tauri architecture.

---

## Architecture summary

| Layer | Choice |
|---|---|
| Shell / windows | **Tauri 2** (Rust) — real native OS windows, one per player + the catalog |
| Frontend | **Svelte 5 + Vite**, multi-entry (catalog + player) for low per-window overhead |
| Styling | **Tailwind 4 + shadcn-svelte**, dark theme |
| Database | **SQLite** (sqlx, compile-time-checked queries) + **FTS5** full-text search |
| Playback | **libmpv render API**, Direct3D 11 backend, `hwdec=auto-safe` (DXVA2/D3D11VA) |
| Media tooling | **FFmpeg / FFprobe** (probe, thumbnails, sprite+VTT, transcode) |

See [`decisions.md`](./decisions.md) for the *why* behind each choice and the rejected alternatives.

---

## Phase 0 — Setup & toolchain  *(in progress)*

**Outcome:** a runnable empty dark-themed app, repo under version control.

- [x] Verify toolchain: Rust 1.96, Node 25, FFmpeg 8.1.2, MSVC 2019, WebView2 149, Git. (mpv deferred to Phase 3.)
- [x] Clear the dead VN project; build MaizeView here.
- [x] Scaffold Tauri 2 + Svelte 5 + Vite.
- [x] Convert away from SvelteKit to plain Svelte 5 + Vite with **multi-entry** (catalog.html + player.html).
- [x] Verify frontend builds; verify `cargo check` passes.
- [x] Add **Tailwind 4** + **shadcn-svelte** + dark theme tokens.
- [x] Add **Lucide** icons.
- [x] Build a base layout shell (top bar, sidebar, content area) in the catalog window.
- [x] `git init` + initial commit.
- [x] Smoke test: `npm run tauri dev` launches into the empty dark UI.

---

## Phase 1 — Library & scan (core)  *(in progress)*

**Outcome:** a real, browsable, searchable library with thumbnails. No auto-tagging yet.

**Backend (Rust):** ✅ done
- [x] DB schema + migrations (`scenes`, `files`, `fingerprints`, `performers`, `studios`, `tags`, `playlists`, `playlist_items`, `scan_paths`, `scan_runs`, FTS5). Provenance columns from day one. `favorite` is 0–5 (ADR-008).
- [x] Scanner: walkdir + extension filter; rayon-parallel oshash + ffprobe; **interleaved per-batch index+write**; oshash move-detection; size+mtime unchanged-shortcut; removed-file detection. Cooperative cancel keeps partials (ADR-009).
- [x] Preview generation: ffmpeg single thumbnail + sprite + WebVTT; auto-runs after scan; progress events.
- [x] Tauri commands: scan paths CRUD, start_scan + cancel_scan, generate_previews, list_scenes (paginated/filterable by tag+performer/sortable/search over title+path+tags+performers), scene_counts, scene_detail (eager tags/performers/studio), set_favorite(level), backfill_scene_titles.
- [x] CRUD commands for tags / performers / studios + scene assignment; list_tags_with_counts.
- [x] Playlist CRUD + items + reorder + weighted shuffle (ADR-010) + shuffle_by_default (ADR-011).

**Frontend (catalog window):** ✅ done
- [x] Library grid (single-thumbnail, duration/resolution chips, 5-heart favorite control), sort + min-favorite filter, debounced search (title/path/tags/performers), tag+performer filter popover + active-chip strip, multi-select add-to-playlist.
- [x] Scan paths settings (native folder picker), scan trigger, scan-progress banner with cancel.
- [x] Sidebar + topbar + empty states.
- [x] Scene detail drawer: inline title/details edit, favorite, studio dropdown, tag/performer chip editors w/ create-on-the-fly, add-to-playlist, file list.
- [x] Playlists: list/create/reorder (drag)/add-remove + shuffle toggle (= default for playback windows).
- [x] Tags management view: list with counts, create/delete, click-to-filter.

---

## Phase 2 — Metadata & auto-tagging

**Outcome:** Stash parity for scanning + auto-tagging + dedup.

- **pHash:** frame-sample → collage → perceptual hash; **duplicate finder** (Hamming distance) UI.
- **StashDB client:** GraphQL fingerprint match `{phash, oshash, md5, duration}` → candidate scenes; per-field **provenance** (manual vs StashDB vs scraper).
- **Scene Tagger UI:** review candidates, apply title/performers/studio/tags/cover; batch Identify task.
- **Search (structured filters, Stash/Cove-aligned):** free-text + `-exclude` + invert; include/exclude tags (any/all), performers, studios; duration / unplayed / min-favorite; **curation gates** (`min_tag_count`, identified-only, min performers, resolution floor); **saved filters**; title/details via **FTS5**, path/tags/performers/segments via LIKE. No query-box DSL; no full AND/OR criterion trees.

---

## Phase 3 — Playback (single window)

**Outcome:** one polished hardware-accelerated player with custom controls.

- **libmpv render API integration** (Rust host): D3D11 backend, `hwdec=auto-safe`, IPC for commands/properties/events.
- **Player window + custom overlay UI:** minimal modern dark controls — play/pause, scrubber with preview thumbnails, volume, **favorite button**, add-to-playlist, quick-tag, keyboard shortcuts. Overlay auto-hides; surrounding chrome minimal.

---

## Phase 4 — Multi-window playback

**Outcome:** simultaneous multi-window playback — the headline feature.

- **Tauri WebviewWindow-per-video:** open N players as independent native OS windows; window manager (list open, focus, close-all, "open in new window" from grid).
- **Inter-window event bus:** sync favorites/play-count/last-position back to the library in real time; optional "follow" mode.
- Per-window geometry memory + restore-last-layout.

---

## Phase 5 — Polish & power features

**Done (v0.2.0 checkpoint):**
- Tier 2 cheap wins: path match, embedded tags, local Stash import
- Bookmark segments (schema + player + drawer + label search)
- Accent theme presets; catalog hygiene (orphan/missing-file prune)
- E2E smoke expanded and green

**Done (post-v0.2.0 search pass):**
- Curation gates: min tag count, identified-only, min performers, resolution floor
- Studio include/exclude; saved filters; FTS5 for title/details free-text include

**Still open / stretch:**
- IAFD or other Tier 2 only if residual demand
- **Faces / AI** — Plan mode first (local GPU vs cloud, privacy, review UX); populate segments/performer links from models, not manual chips
- Settings: HW-accel tuning, scan scheduling, hotkeys, preview density
- Optional later: 1:1 power tokens in the search box mapping onto existing `ListScenesArgs` (not a general JQL)
- **Not in Phase 5:** automatic cross-drive file relocation (ADR-013); full Stash criterion AND/OR trees; Cove dynamic groups

---

## How to resume

1. **`AGENTS.md`** then the **🚨 Resume hotline** at the top of [`progress.md`](./progress.md) — 30-second "where am I, what's next."
2. Find the next unchecked item in the active phase above.
3. Check [`decisions.md`](./decisions.md) if you're about to make an architectural choice.
4. Check [`setup.md`](./setup.md) / `.cursor/rules/capabilities.mdc` for toolchain/PATH/test-lib/build-lock gotchas.
5. Update `progress.md` when work completes — be honest about what's verified vs. attempted.

## Policies

**Schema mutability:** Pre-v0.2.0, the initial migration was sometimes edited in place. From **v0.2.0 onward**, prefer **additive** migrations for anything that ships to a tagged release. Dev reset still allowed by clearing `%APPDATA%/MaizeView/maizeview.db*`.

**Commit gating:** Don't commit broken. A commit should pass `cargo check`, `npm run build`, and (when scanner/schema is touched) the `scanner_e2e.rs --ignored` test. After UI changes, run `npm run test:e2e:smoke` when the E2E app is available.

**Click-test before moving on:** After UI changes, relaunch the app and have the user click-test before piling on the next feature.

**No busywork curation:** Do not ship features that assume meticulous hand-tagging of large libraries. Prefer crowdsourced/API identity (stash-box) and automated enrichment; timed occurrence data needs AI/import.
