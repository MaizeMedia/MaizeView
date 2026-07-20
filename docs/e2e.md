# E2E testing (Playwright + Tauri CDP)

MaizeView E2E tests drive the **real desktop app** on Windows via WebView2 Chrome DevTools Protocol (CDP). Tests use an **isolated SQLite database** and optionally a **sandbox video folder** so your personal library is never touched.

## Quick start

1. **Install browsers** (once):

   ```powershell
   cd C:\path\to\MaizeView
   npm install
   npx playwright install chromium
   ```

2. **Configure sandbox** — copy `e2e/.env.example` → `e2e/.env` and set:

   ```env
   MAIZEVIEW_TEST_LIB=C:\path\to\your\e2e-videos
   ```

   A handful of short `.mp4` / `.mkv` files is enough for unit/integration tests. The full smoke suite has been verified against ~900+ files. Same env var as `scanner_e2e.rs`.

3. **Terminal A — start the app for testing:**

   ```powershell
   npm run e2e:app
   ```

   This sets `MAIZEVIEW_DB_PATH=e2e/.data/maizeview.db` and enables CDP on port **9222**.

4. **Terminal B — run tests:**

   ```powershell
   npm run test:e2e
   ```

   Screenshots for review land in `docs/e2e-reports/`. HTML report: `npm run test:e2e:report`.

## npm scripts

| Script | Purpose |
|--------|---------|
| `npm run e2e:app` | Launch MaizeView with E2E env (isolated DB + CDP) |
| `npm run test:e2e` | Run Playwright tests (app must be running) |
| `npm run test:e2e:smoke` | Full product smoke (catalog + full-smoke specs) |
| `npm run test:e2e:ui` | Playwright UI mode |
| `npm run test:e2e:report` | Open last HTML report |

Set `E2E_AUTO_START=1` in `e2e/.env` to have Playwright launch the app for you (slower, fine for CI).

## What gets tested

| Spec | Requires sandbox | Checks |
|------|------------------|--------|
| `catalog.smoke.spec.ts` | No | Shell loads, sidebar nav |
| `full-smoke.spec.ts` | Yes (`MAIZEVIEW_TEST_LIB`) | Library grid, search/sort/filters (curation gates, saved filters), favorites, scene drawer (Identify/Search, Segments, drawer beside grid), tags, playlists (create, Play, Delete), duplicates, settings (stash-box, Stash import, Appearance accents, identify stats), multiselect, player window open |
| `search-filters.spec.ts` | Yes | Seeds tags/studio/performer; asserts `list_scenes` min_tag_count / exclude / studio / height / identified; UI curation chips; exclude+min tags; saved filter save/apply/delete; text −exclude |

Latest verified: **search-filters 8/8** (2026-07-12). Prefer `data-testid` for playlist Play/Delete (`playlist-play`, `playlist-delete`), Appearance (`appearance-settings`), curation (`min-tag-count`, `curation-gates`, `saved-filters-panel`).

## Environment variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `MAIZEVIEW_TEST_LIB` | — | Folder of test videos |
| `MAIZEVIEW_DB_PATH` | `e2e/.data/maizeview.db` | Isolated SQLite |
| `CDP_URL` | `http://127.0.0.1:9222` | WebView2 debug endpoint |
| `E2E_AUTO_START` | `0` | Auto-launch app from Playwright |
| `E2E_SMOKE_TIMEOUT_MS` | `180000` | Per-test timeout for full smoke suite |

## Limitations

- **Windows + WebView2 only** for full CDP mode (matches MaizeView v1 target).
- **Native folder picker** is not automated — sandbox path is seeded via Tauri IPC.
- **libmpv playback** is not asserted in E2E — smoke test only verifies a `player-*` window opens (via `window.__TAURI__.webviewWindow.getAllWebviewWindows()`). Visual/manual QA still required.
- **Slow paths on large libraries:** invert search previously mis-handled NULL titles (SQL three-valued logic) and double-scanned with correlated EXISTS — fixed 2026-07-11. Prefer narrow terms in smoke; full invert coverage optional.
- Browser-only mock mode (fast CI without Tauri) can be added later if needed.

## Adding tests

- Prefer `data-testid` selectors (see `catalog-search`, `nav-library`, `scene-count`, `stashdb-batch`, `select-mode`).
- Use `invokeCmd()` and `listWindowLabels()` from `e2e/helpers/tauri.ts`.
- Navigation helpers in `e2e/helpers/nav.ts` (`goLibrary`, `goPlaylists`, `openFirstSceneDrawer`, …).
- Save review screenshots with `captureReport(page, "step-name")`.
