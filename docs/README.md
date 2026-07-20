# MaizeView — Project Docs

MaizeView is a Windows desktop app for cataloging, browsing, tagging, and viewing a large local adult video collection. Headline feature: **multiple video players running simultaneously in separate native OS windows**, each with custom overlay controls (favorite button, quick-tag, scrubber with previews).

This `docs/` folder is the source of truth for the project. It is written so that **any future session (AI or human) can resume work without re-deriving context**.

## Reading order

1. **[`../AGENTS.md`](../AGENTS.md)** — agent session start (every chat).
2. **[`plan.md`](./plan.md)** — the full phased implementation plan (approved). Read this first when building.
3. **[`progress.md`](./progress.md)** — what's done, what's in progress, what's next. **Update this every time work completes.** This is the resume point.
4. **[`decisions.md`](./decisions.md)** — architectural decisions and the reasoning behind them, with research links. Append-only.
5. **[`setup.md`](./setup.md)** — toolchain install + dev environment.
6. **[`e2e.md`](./e2e.md)** — Playwright E2E smoke tests (CDP + sandbox library).

Project Cursor rules: `.cursor/rules/capabilities.mdc`, `.cursor/rules/storage-and-git.mdc`.

## Reference projects studied

- **Stash** ([stashapp/stash](https://github.com/stashapp/stash), [stash-box](https://github.com/stashapp/stash-box), [docs](https://docs.stashapp.cc/)) — Go + GraphQL + SQLite + React. File hashing (MD5/oshash), perceptual hashing (phash via collage), fingerprint matching against StashDB.
- **Cove** ([yourcove/cove](https://github.com/yourcove/cove), [yourcove.net](https://yourcove.net/)) — modern successor: .NET 10 + PostgreSQL/pgvector + Vite. Provenance-first tagging, **per-occurrence / segment tagging**, **AI extensions** for metadata acquisition, extension model. See [`plan.md`](./plan.md#metadata-sources-catalog) for what we may adopt vs. defer.

We are **not** copying either; we're mimicking their scanning + tagging capability on a different stack optimized for our specific requirements (Windows-only, multi-window native playback, custom overlay UIs).

**Checkpoint:** v0.1.0 = multi-window playback. **v0.2.0** = metadata Tier 1 + local enrichment + bookmark segments + catalog hygiene + accent themes + E2E harden. Post-v0.2.0: search curation gates, saved filters, FTS5. Faces/AI is the next *planned* stretch, not started.

## Quick orientation

```
ZCodeProject/
├── docs/              ← you are here
├── src/               ← frontend (Svelte 5, multi-entry)
│   ├── catalog/       ← library browser window
│   └── player/        ← per-video window (Phase 3+)
├── src-tauri/         ← Rust backend (Tauri 2)
├── catalog.html       ← Vite entry: catalog window
├── player.html        ← Vite entry: player window
└── package.json
```
