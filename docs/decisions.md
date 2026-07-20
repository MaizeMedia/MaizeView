# Architectural Decisions

> Append-only log of significant decisions and their rationale. Format: ADR-lite — Decision, Context, Reasoning, Rejected alternatives.

## Index

| ADR | Topic | One-line |
|---|---|---|
| [001](#adr-001--shell-tauri-2-rust) | Shell | Tauri 2 (Rust) — native windows, low overhead, embeds libmpv |
| [002](#adr-002--frontend-svelte-5--vite-multi-entry-not-sveltekit) | Frontend | Plain Svelte 5 + Vite, multi-entry per window type (no SvelteKit) |
| [003](#adr-003--styling-tailwind-4--shadcn-svelte-dark-theme) | Styling | Tailwind 4 + shadcn-svelte, dark theme |
| [004](#adr-004--database-sqlite--fts5-not-postgrespgvector) | Database | SQLite + FTS5 (not Postgres+pgvector) |
| [005](#adr-005--playback-libmpv-render-api-d3d11-hardware-decoded) | Playback | libmpv render API, D3D11, hardware-decoded |
| [006](#adr-006--provenance-tracked-from-phase-1) | Data model | Field-level provenance from day one |
| [007](#adr-007--metadata-servers-stashdb-first-others-sequenced-by-difficulty) | Tagging | StashDB first, others additive |
| [008](#adr-008--favorites-are-a-05-level-not-a-boolean) | Data model | `favorite` is 0–5 (also weighted-shuffle weight) |
| [009](#adr-009--scan-cancel-keeps-partials-no-confirm-dialog) | UX | Scan cancel keeps partials; no confirm dialog |
| [010](#adr-010--weighted-shuffle-by-favorite-level) | Playlists | Weighted draw without replacement; favorite × recency × session cooldown |
| [011](#adr-011--shuffle-is-per-playback-window-not-per-playlist) | Playback | Each playback window owns its own streaming shuffle queue; playlist stores a "shuffle by default" flag only |
| [012](#adr-012--use-tauri-plugin-libmpv-for-playback) | Playback | Use the tauri-plugin-libmpv crate (not hand-rolled libmpv FFI) |

## ADR-012 — Use `tauri-plugin-libmpv` for playback (not hand-rolled libmpv FFI)

**Decision:** Adopt the [`tauri-plugin-libmpv`](https://lib.rs/crates/tauri-plugin-libmpv) crate (v0.3.2, Nov 2025) as the libmpv integration layer, instead of writing our own `libmpv-sys`/`libmpv2` FFI binding and D3D11 host as ADR-005 originally sketched. ADR-005's *technical* choices still hold (libmpv, D3D11-class HW backend, `hwdec=auto-safe`, transparent webview overlay on top, custom HTML controls — not stock OSD); this ADR only changes *who writes the glue*.

**Context:** Phase 3 was originally scoped as a from-scratch embed: load `libmpv-2.dll`, build a Rust host around the render API, share a D3D11 texture with the webview compositor, wire property/command/event IPC to a Svelte overlay. That is the riskiest, most session-expensive part of the whole project (weeks of unsafe FFI + GPU surface-sharing work, with several known pitfalls around `vo`/`hwdec` combos rendering into the wrong surface — mpv #6722). A pre-existing plugin materially changes the build-vs-buy calculus.

**What the plugin gives us (verified from its docs, Nov 2025):**
- Embeds libmpv into a Tauri window with the webview overlaid transparently on top — **exactly the architecture in ADR-005**.
- **Windows: "fully tested"** by the maintainer; ships a setup script (`npx tauri-plugin-libmpv-api setup-lib`) that fetches `libmpv-2.dll` + `libmpv-wrapper.dll` (zhongfly builds).
- Defaults to `vo=gpu-next` + `hwdec=auto-safe` — hardware-accelerated out of the box, matching ADR-005.
- JS API surface is what an overlay needs: `init`, `command`, `setProperty`, `getProperty`, `observeProperties`. Play/pause, time-pos, duration, volume, seek — all there.
- The transparent webview overlay is *ours* to fill with HTML/Svelte — so the custom-controls requirement (favorites button, scrubber with previews, auto-hide) is fully preserved. We are not forced into mpv's stock OSD.

**Reasoning:**
- The plugin removes the single biggest risk in the project (hand-rolled GPU-surface FFI) for free. If it works, Phase 3 drops from "many sessions, may not finish" to "likely one to two sessions."
- It does **not** compromise any user requirement: Windows-only ✓, no Avalonia ✓, custom overlay UI ✓ (webview is ours), hardware-accelerated ✓, real native OS windows ✓ (Tauri window = OS window), multi-window ✓ (one plugin instance per window).
- The custom favorites/scrubber overlay is unaffected — that work lives in Svelte and talks to the plugin's IPC, exactly as it would have talked to a hand-rolled binding.
- Buying dependency on a young plugin is a real cost, but: it's MIT, the surface we use is small and stable (init/command/property/observe), and if it ever rots we can swap in our own FFI later *behind* the same JS-facing API. The overlay code is the valuable part and is plugin-agnostic.

**Rejected:**
- **Hand-rolled `libmpv-sys` FFI + D3D11 host (original ADR-005 plan).** Strictly more work and more risk for the same end result. Keep as the fallback if the plugin turns out to be a dead end on our specific files/hardware.
- **`tauri-plugin-mpv`** (the *other* plugin, v0.2.5). Embeds an mpv *player window*, which fights our custom-overlay requirement. `tauri-plugin-libmpv` uses the library interface (in-process), which is what we want.
- **HTML5 `<video>`.** Already rejected in ADR-005; codec coverage and concurrent-instance caps disqualify it for a large varied library.

**Fallback trigger / migration path:** If we hit a blocker the plugin can't get past (e.g. a specific codec/hwdec combo, a multi-window ownership bug, overlay input routing issues we can't work around), we drop to `libmpv-sys` and re-implement the host behind the same Svelte-facing IPC shape. The overlay Svelte components are written to be plugin-agnostic from day one (they call our own `player` API wrapper, not the plugin's API directly).

**Setup impact:** `docs/setup.md` gains a "libmpv" section: run the plugin's `setup-lib` script to fetch the DLLs (or drop in a manual zhongfly build), and the two DLLs ship next to the exe. Will be documented as part of Phase 3 onboarding.

**Addendum (2026-07-10, post-implementation):** Three corrections to the above, learned by getting playback actually working:

1. **The plugin uses `wid` embedding, NOT the render API.** ADR-005 sketched the render-API approach (D3D11 texture sharing); in reality the plugin passes our window's HWND to mpv via `wid` (legacy `--wid` embedding — see `utils.rs::get_wid`), and mpv creates a child window inside ours to render into. The webview floats transparently on top. The *outcome* (libmpv + HW decode + custom overlay) matches ADR-005; the *mechanism* is the simpler `wid` path, not render-API texture sharing. ADR-005's "render API not legacy --wid" line is superseded by this.

2. **`vo=gpu` + `gpu-api=d3d11`, NOT `vo=direct3d` or `vo=gpu-next`.** The README's suggested `vo=gpu-next` did not paint video with `wid` embedding on our test system; the legacy `vo=direct3d` also failed to paint. **`vo=gpu` + `gpu-api=d3d11`** is what made video appear. Confirmed via the standalone probe in `tools/mpv-probe/`.

3. **`osd_level` must NEVER be set as an mpv init option.** Setting `osd_level=0` (or any value) in `initialOptions` causes `libmpv-wrapper.dll`'s `mpv_wrapper_create` to **deadlock** — the player window spins forever and the app hangs on close. This was the hardest bug to find; it was isolated by building a standalone Rust probe (`tools/mpv-probe/main.rs`) that calls the wrapper directly outside Tauri and bisects options one by one. Use `osc=no` to suppress mpv's stock OSD; that's the correct option and works fine.

**Current state:** Video plays ✅, overlay works ✅. Hardware decode reports `off` on the user's dual-GPU laptop (integrated + dedicated, dynamic switching) even with `hwdec=d3d11va`/`auto-safe` — deferred to Phase 4 (N simultaneous videos) where it matters most.

---

## ADR-011 — Shuffle is per playback window, not per playlist

**Decision:** Shuffle is owned by each **playback window**, not the playlist. A playlist only stores a `shuffle_by_default` flag (set pre-play); each playback window maintains its own independent streaming queue that draws next-via-weighted-shuffle (ADR-010) from its source.

**Context:** User wants N playback windows running simultaneously, each shuffling independently from the same (or different) source — "n playback windows all shuffling independently." The initial `shuffle_playlist` command produced a single static reordered list, which is the wrong model for playback (one fixed order, no independent windows).

**Reasoning:**
- A static reordered list forces one order on everyone; per-window queues let N windows diverge.
- The playlist's job is to *declare* a source + a default; the window's job is to *consume* it.
- `shuffle_by_default` on the playlist lets a window inherit the user's preference when it starts playing, with an in-window toggle to flip it live.
- The current `shuffle_playlist` command stays useful as a one-shot "preview a shuffle" in the playlist view; the per-window streaming queue (Phase 3/4) is the real playback primitive and reuses the same weighted-draw algorithm.

**Schema hook added now:** `playlists.shuffle_by_default INTEGER NOT NULL DEFAULT 0`. Phase 3/4 playback consumes it.

**Implemented later (Phase 3/4):** per-window streaming queue with weighted-draw-next, in-window shuffle toggle, source = playlist or filter result.

---

## ADR-010 — Weighted shuffle by favorite level

**Decision:** Playlist shuffle is a weighted draw without replacement, where each scene's base weight = `max(1, favorite_level)`, then adjusted by **recency** (`last_played_at`) and a short in-window cooldown after a full pass.

**Context:** User wants favoriting to behave like a music app — higher-favorite items surface more often in shuffle, but unfavorited items still appear (just rarely). ADR-008 set favorite as 0–5. Later: VLC-style “sticky set” fatigue on huge playlists — keep favorite bias, down-weight titles played recently so a new pass doesn’t immediately re-hit the same handful.

**Reasoning:**
- `max(1, favorite)` gives every scene a non-zero chance (a favorite-0 scene has weight 1 vs a favorite-5 scene's weight 5 → the favorite is ~5× more likely before recency).
- Draw **without replacement** within a pass; when the queue is exhausted, reset and redraw (not “always index 0”).
- Recency multipliers (never-played boost; steep cut for <1h / <6h / <1d / <7d) fight same-set loops while leaving favorites as the primary preference — unfav to cool a sticky title.
- `record_scene_play` writes `play_count` + `last_played_at` on player load so filters and shuffle share real history.
- Implemented in player (`src/lib/shuffle.ts` + `player/App.svelte`); playlist Play start uses the same weights. Rust `shuffle_playlist` remains a one-shot preview helper.

**Rejected:** uniform random (ignores favorites); storing a shuffle order on the playlist (over-complicated, goes stale on favorite edits); silencing favorite-0 entirely (too aggressive); fighting favorites with anti-stickiness (user wants sticky-to-likes).

---

## ADR-001 — Shell: Tauri 2 (Rust)

**Decision:** Use Tauri 2 as the desktop shell; backend logic in Rust.

**Context:** Required: Windows-only, real native OS windows (one per video), many simultaneous windows, custom overlay UIs, hardware-accelerated playback.

**Reasoning:**
- Tauri 2 spawns genuine OS webview windows per call → matches "real native OS windows" exactly.
- Each window is its own hardware-accelerated WebView2 instance; memory stays low vs. Electron ([Tauri at Scale case study](https://medium.com/@hadiyolworld007/tauri-at-scale-building-multi-window-desktop-apps-without-the-bloat-e17676b906c6), [multi-webview beta](https://v2.tauri.app/blog/tauri-2-0-0-beta/)).
- Rust backend is ideal for scanning/hashing/indexing and for hosting libmpv.
- Inter-window state sync is a solved pattern via a Tauri event bus ([state sync engine](https://www.reddit.com/r/tauri/comments/1r3u2bu/i_built_a_multiwindow_state_sync_engine_for_tauri/)).

**Rejected:**
- **Electron** — fine UI, but ~2–4× RAM per window; painful when running many video windows at once.
- **Native (Qt/C++)** — best perf and codec control, but custom overlay UIs become real work; loses the easy customization the user wants.
- **Avalonia** — user vetoed; its video overlay control support is dubious.

---

## ADR-002 — Frontend: Svelte 5 + Vite (multi-entry), not SvelteKit

**Decision:** Plain Svelte 5 + Vite with multiple HTML entry points (`catalog.html`, `player.html`). No SvelteKit.

**Context:** The Tauri+TS scaffold ships SvelteKit by default.

**Reasoning:**
- SvelteKit assumes one routed app shell; we want **one minimal Vite entry per window type** so each player window loads only the bundle it needs. With many windows open, per-window overhead matters.
- Svelte 5 compiles to a ~1KB runtime with fine-grained reactivity — lowest per-window CPU for live overlay state (seek position, buffer, play state updating every frame).
- No SSR/routing needs; a desktop multi-window app has none of the problems SvelteKit solves.

**Rejected:**
- **SvelteKit** (template default) — wrong model; single-shell routing.
- **React** — heavier per-window runtime.
- **SolidJS** — comparable perf to Svelte 5 but smaller ecosystem.

---

## ADR-003 — Styling: Tailwind 4 + shadcn-svelte, dark theme

**Decision:** Tailwind 4 + shadcn-svelte, dark theme by default.

**Context:** User wants a "modern looking UI and a dark theme"; needs heavy customization (custom overlay controls — favorite button, etc.).

**Reasoning:**
- Tailwind 4 = modern, fast, minimal config.
- shadcn-svelte gives copy-in components we fully own and can customize — not a black-box library. Matches "custom overlay UI" requirement.
- Dark theme is the natural default for a media app.

---

## ADR-004 — Database: SQLite + FTS5 (not Postgres+pgvector)

**Decision:** SQLite via `sqlx` (compile-time-checked queries) + FTS5 full-text search.

**Context:** Cove uses PostgreSQL + pgvector. Stash uses SQLite.

**Reasoning:**
- Single-user local app; SQLite is dramatically simpler to ship (single file, trivial backup, no service).
- FTS5 is plenty for tag/title/performer search at single-user scale.
- We only reach for vectors if/when face/embedding similarity lands in Phase 5 — and even then a sidecar or SQLite vector extension is an option before Postgres.

---

## ADR-005 — Playback: libmpv render API, D3D11, hardware-decoded

**Decision:** Embed libmpv via its render API (Direct3D 11 backend, `hwdec=auto-safe`). Each player window = a Rust-managed libmpv instance rendering to a D3D11 texture, with a transparent overlay HWND (the webview) carrying HTML controls.

**Context:** Adult video libraries are large, codec/container variety is high, and the user wants many simultaneous players.

**Reasoning:**
- MPV handles every codec/container adults throw at it; HTML5 `<video>` would limit coverage.
- Per-instance HW decode (DXVA2/D3D11VA on Windows) scales to many concurrent players.
- MPV's overlay model is overlay-native; the custom controls (favorite button etc.) are just HTML/CSS in the same webview.
- Render API (not legacy `--wid`) is the modern, flexible embedding path.

**References:** [mpv manual](https://mpv.io/manual/stable/), [d3d11 render API #5979](https://github.com/mpv-player/mpv/issues/5979), [embeddable libmpv discussion #16458](https://github.com/mpv-player/mpv/discussions/16458).

**Rejected:**
- **HTML5 `<video>` only** — codec coverage and concurrent-instance limits make it unsuitable for a large varied library.
- **Pure native player** — loses the easy custom overlay UI the user wants.

**Pitfall noted:** some `hwdec`+`vo` combos make libmpv render into its own window instead of the embedded surface (mpv issue #6722). Verify with `hwdec=auto-safe` first; tune later.

---

## ADR-006 — Provenance tracked from Phase 1

**Decision:** Record the *source* of every metadata field (manual / StashDB / FansDB / scraper / etc.) from the very first schema.

**Context:** Cove's standout idea; multi-source tagging arrives in Phase 2+.

**Reasoning:** Building provenance in later means a painful migration. It's cheap to add a `field_provenance` table now andpopulate it even when the only source is "manual".

---

## ADR-007 — Metadata servers: StashDB first, others sequenced by difficulty

**Decision:** Support StashDB first, then additive sources from the [metadata catalog](../plan.md#metadata-sources-catalog), sequenced by integration difficulty: **ThePornDB**, **FansDB**, **JAVStash**, **IAFD**, plus non-API enrichments (filename heuristics, embedded tags, local Stash import) as cheap wins.

**Context:** Stash-box servers share GraphQL + API-key auth. **IAFD** and site scrapers are separate integration paths. **AI / time-segment tagging** (frame samples → suggested tags, segment bookmarks, per-occurrence tags) is a **stretch tier** — Cove is the reference (extensions, segments, faces/pgvector), not a dependency. Filename heuristics mean **parsing paths into catalog fields**, not relocating files (see ADR-013).

**Reasoning:** StashDB alone delivers the bulk of scene-level auto-tagging value. Everything else is additive. AI features must stay local-first, opt-in, provenance-tracked, and review-gated — never silent bulk overwrite.

---

## ADR-013 — Catalog organizes virtually; no automatic on-disk file moves

**Decision:** MaizeView does **not** automatically move, rename-across-volumes, or “sort onto drives” user media as part of metadata, identify, or tagging. Organization is **in the catalog** (filters, tags, playlists, virtual views). Scan paths remain user-owned facts about where files already live.

**Context:** Hoarder libraries often span multiple drives filled near capacity. Cross-volume relocation is copy + verify + delete (temporary 2× space, hours per TB, crash/resume, free-space budgeting). Cove-style renamer / organizer extensions and “update the path in the DB” undersell that rabbit hole. Filename auto-tag (parse path → tags) must not blur into physical sort.

**If a Library organizer is ever productized later, it requires its own plan:** dry-run move manifest, destination capacity + headroom checks, same-volume rename vs cross-volume copy, crash-safe journal, verify-before-delete, DB path update only after success, optional overnight queue. Until that exists, treat on-disk organize as an explicit **non-goal**.

**Reasoning:** Metadata work must stay reversible and non-destructive. Mixing identify with multi-TB juggling risks data loss and support nightmares for little identify upside.

---

## ADR-014 — No busywork curation; StashDB is scene-level only

**Decision:** Product UX and backlog must not assume users will meticulously tag or segment large libraries by hand. Prefer import, identify, heuristics, and (later) AI suggestions with review. **StashDB / stash-box are scene-level** — they do not provide timed “performer at 12:30” data. Manual segment→performer/tag chip UI is **deferred** (schema hooks only). Manual segments as shipped are optional bookmarks + free-text labels.

**Context:** Adult libraries are often 10k+ files. Power-user curation tools look impressive and go unused. Segment performer chips would be busywork without AI/import. Faces/ML remain stretch and need a plan before implementation.

**Reasoning:** Ship value that works at scale without unpaid labor. Leave schema hooks for future AI/import; don’t build chip UIs that only obsessives would fill.

---

## ADR-009 — Scan cancel keeps partials (no confirm dialog)

**Decision:** Cancelling a scan keeps everything written so far. No confirmation dialog. The banner reports "kept N of M files".

**Context:** Initial impl batched all hashing/probing, then did all DB writes — so cancel threw away a whole library's worth of work. A confirm dialog was considered.

**Reasoning (the UI-practice rule):** don't ask the user to take responsibility for a destructive action — make the action non-destructive instead. Confirmations get clicked reflexively.

Two-part fix:
1. **Interleave indexing + writing per batch (~32 files).** Each batch is hashed → probed → written before the next. Cancel between batches loses nothing; cancel mid-batch loses only that batch's hashing.
2. **Idempotent rescan = free resume.** Because the scanner keys on oshash + size+mtime shortcuts, the next scan skips already-indexed files and effectively continues. No pause/resume button needed.

Removed-file detection is skipped on cancel (unscanned batches must not be mis-flagged as removed). Verified by `scan_cancel_keeps_partials` test (100 files: cancel after ~1 batch → 32 kept; resume → all 100).

---

## ADR-008 — Favorites are a 0–5 level, not a boolean

**Decision:** `scenes.favorite` is `INTEGER 0..5` (0 = not favorited), not a boolean. The same value doubles as the weighted-shuffle weight in playlists.

**Context:** User wants to favorite an item "up to 5 times", filter/sort by favorite level, and have higher-favorite items play more often in shuffle mode (like favoriting a song in a music app).

**Reasoning:**
- One column carries both the "is favorited" signal (level > 0) and the per-item weight.
- Weighted shuffle is a pure function of the levels at play time — no extra schema.
- A separate dormant `rating` column stays available for a future "quality score" concept distinct from want-to-see frequency.

**Note:** The initial migration is edited in place (pre-release; no shipped users). Post-v1.0, schema changes become additive migrations.

---

## ADR-015 — User-initiated transcode is permitted; ADR-013 carve-out

**Decision:** ADR-013 forbids *automatic* on-disk moves/renames/sorts. This ADR carves out an explicit exception: **user-initiated, explicitly-confirmed video downscaling (transcoding)** may replace or add files on disk, because it is a deliberate space-saving operation — not metadata-driven sorting.

**Context:** Users with large 4K/UHD libraries want to reclaim space by downscaling to 1080p/720p. This necessarily writes a new video file and (in "replace" mode) deletes the original — which ADR-013's blanket "no on-disk moves" would forbid. The two are distinct: ADR-013 guards against multi-TB cross-volume juggling masquerading as metadata work; transcode is a single-file, same-directory, user-confirmed operation.

**Safety invariants (enforced in `transcode_job.rs`, non-negotiable):**
- The transcoded file is written to a temp path in the **same directory** as the source, so the final move is same-volume and atomic on most filesystems.
- The **original is never touched until the temp is verified** (video stream present, output height ≤ target, duration within 2% of source). Any verification failure deletes the temp and leaves the original intact.
- A **free-space headroom check** aborts before transcode if the destination drive can't fit the output.
- The DB `files` row is updated **only after** the on-disk file is final (replace) or a new row is inserted (keep-both).
- The job is **cancellable**; an in-flight ffmpeg is allowed to finish but no further scenes start.
- "Keep both" mode is fully non-destructive (the transcoded file is added as an additional encoding on the scene, which the schema already supports).
- After replace/keep, previews are regenerated and oshash/phash recomputed so search, duplicate detection, and the grid stay consistent with the new bytes.

**Reasoning:** The destructive-safety properties ADR-013 demands of a future organizer (verify-before-delete, crash-safe, same-volume, DB-only-after-success) are exactly what the transcode pipeline provides per-file. Keeping the operation opt-in via an explicit Convert dialog (with a breakdown, estimated savings, and per-file preview) preserves the "metadata work stays reversible and non-destructive" spirit for everything *except* this one user-requested, space-saving action.

**Related:** ADR-013 (the broad non-goal this narrows). Filenames/tags are rewritten to match the new resolution using `transcode_tokens` — this is catalog metadata, not physical sorting, and is scoped per-scene (other 4K scenes keep their tags).

---

## Open questions (to resolve when relevant)

- **Vector index strategy** *if* Phase 5 face/embedding similarity happens: SQLite vector extension vs. sidecar (e.g. `usearch`/`lancedb`). Plan before any faces work.
- **libmpv / thumbnail format:** largely resolved in Phases 1–3; revisit only if packaging pain appears.
