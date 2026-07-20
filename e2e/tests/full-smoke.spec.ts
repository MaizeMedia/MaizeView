import { test, expect } from "../fixtures/catalog";
import { captureReport } from "../helpers/screenshots";
import {
  clearSearchAndFilters,
  closeSceneDrawer,
  ensureSandboxIndexed,
  goLibrary,
  goPlaylists,
  openFirstSceneDrawer,
  waitForDebounce,
} from "../helpers/nav";
import { invokeCmd, listWindowLabels } from "../helpers/tauri";
import { testLibPath } from "../helpers/env";

const smokeTag = `e2e-smoke-tag-${Date.now()}`;
const smokePlaylist = `e2e-smoke-pl-${Date.now()}`;

test.describe.serial("Full product smoke", () => {
  test.skip(!testLibPath(), "Set MAIZEVIEW_TEST_LIB in e2e/.env");

  test.beforeEach(async () => {
    test.setTimeout(Number(process.env.E2E_SMOKE_TIMEOUT_MS ?? 180_000));
  });

  test("01 library indexed and grid visible", async ({ catalogPage }) => {
    const total = await ensureSandboxIndexed(catalogPage);
    expect(total).toBeGreaterThan(0);

    await goLibrary(catalogPage);
    await expect(catalogPage.getByTestId("scene-count")).toContainText(String(total), {
      timeout: 30_000,
    });
    await expect(catalogPage.getByTestId("scene-grid-viewport").getByRole("button").first()).toBeVisible();
    await captureReport(catalogPage, "smoke-01-library-grid");
  });

  test("02 search, sort, and min-favorite filters", async ({ catalogPage }) => {
    await goLibrary(catalogPage);

    await catalogPage.getByTestId("catalog-search").fill(".mp4");
    await waitForDebounce(catalogPage);
    const mp4 = await invokeCmd<{ total: number }>(catalogPage, "list_scenes", {
      args: { search: ".mp4", limit: 5, offset: 0 },
    });
    expect(mp4.total).toBeGreaterThan(0);
    await captureReport(catalogPage, "smoke-02-search");

    await catalogPage.getByTestId("sort-select").selectOption("title");
    await waitForDebounce(catalogPage);
    await expect(catalogPage.getByTestId("scene-grid-viewport").getByRole("button").first()).toBeVisible();

    await catalogPage.locator("#minfav").selectOption("1");
    await waitForDebounce(catalogPage);
    await catalogPage.locator("#minfav").selectOption("0");

    await catalogPage.getByTestId("catalog-search").fill("");
    await waitForDebounce(catalogPage);
    await captureReport(catalogPage, "smoke-02-filters-cleared");
  });

  test("03 filter panel and inline exclude search", async ({ catalogPage }) => {
    await catalogPage.reload();
    await expect(catalogPage.getByTestId("catalog-search")).toBeVisible({ timeout: 30_000 });
    await goLibrary(catalogPage);
    await catalogPage.getByTestId("filter-toggle").click();
    await expect(catalogPage.getByText("Duration (minutes)")).toBeVisible();
    await expect(catalogPage.getByTestId("curation-gates")).toBeVisible();
    await expect(catalogPage.getByTestId("min-tag-count")).toBeVisible();
    await expect(catalogPage.getByTestId("identified-only-filter")).toBeVisible();
    await catalogPage.getByRole("button", { name: "Close filter panel" }).click();

    await catalogPage.getByTestId("saved-filters-toggle").click();
    await expect(catalogPage.getByTestId("saved-filters-panel")).toBeVisible();
    await catalogPage.getByRole("button", { name: "Close saved filters" }).click();

    // Derive a search token from the sandbox catalog — nothing hardcoded,
    // works against any MAIZEVIEW_TEST_LIB.
    const first = await invokeCmd<{ scenes: { title: string | null }[] }>(catalogPage, "list_scenes", {
      args: { limit: 1, offset: 0 },
    });
    const token =
      first.scenes[0]?.title?.split(/\s+/).find((w) => w.length >= 4) ?? "mp4";
    await catalogPage.getByTestId("catalog-search").fill(token);
    await waitForDebounce(catalogPage);
    await expect(catalogPage.getByTestId("scene-grid-viewport").getByRole("button").first()).toBeVisible({
      timeout: 30_000,
    });

    await catalogPage.getByTestId("catalog-search").fill("");
    await waitForDebounce(catalogPage);
    await captureReport(catalogPage, "smoke-03-filter-panel");
  });

  test("04 favorites view", async ({ catalogPage }) => {
    await catalogPage.getByTestId("nav-favorites").click();
    await expect(catalogPage.getByTestId("library-grid")).toBeVisible();
    await captureReport(catalogPage, "smoke-04-favorites");
    await goLibrary(catalogPage);
  });

  test("05 scene drawer metadata and StashDB identify UI", async ({ catalogPage }) => {
    await goLibrary(catalogPage);
    await openFirstSceneDrawer(catalogPage);

    const drawer = catalogPage.getByTestId("scene-drawer");
    await expect(drawer.getByText("Scene details")).toBeVisible();
    await expect(drawer.getByText("Identify")).toBeVisible();
    await expect(drawer.getByRole("button", { name: "Search" })).toBeVisible();
    await expect(drawer.getByText(/Files \(\d+\)/)).toBeVisible();
    await expect(drawer.getByText("Add to playlist")).toBeVisible();
    await expect(drawer.getByText("Segments", { exact: true })).toBeVisible();

    // Drawer sits beside the grid — list must stay visible.
    await expect(catalogPage.getByTestId("scene-grid-viewport")).toBeVisible();
    await expect(catalogPage.getByTestId("scene-grid-viewport").getByRole("button").first()).toBeVisible();

    await captureReport(catalogPage, "smoke-05-scene-drawer");

    await drawer.getByRole("button", { name: "Close" }).click();
    await expect(drawer).not.toBeVisible({ timeout: 10_000 });
  });

  test("06 tags create and filter library", async ({ catalogPage }) => {
    await catalogPage.getByTestId("nav-tags").click();
    await catalogPage.getByRole("button", { name: "New tag" }).click();
    await catalogPage.getByPlaceholder("Tag name").fill(smokeTag);
    await catalogPage.getByRole("button", { name: "Create", exact: true }).click();
    await expect(catalogPage.getByText(smokeTag)).toBeVisible({ timeout: 15_000 });

    await catalogPage.getByRole("button", { name: new RegExp(smokeTag) }).click();
    await goLibrary(catalogPage);
    await waitForDebounce(catalogPage);
    await captureReport(catalogPage, "smoke-06-tag-filter");
    await catalogPage.getByRole("button", { name: "Clear all" }).click();
    await waitForDebounce(catalogPage);
    await catalogPage.getByTestId("nav-tags").click();
  });

  test("07 playlists create and open", async ({ catalogPage }) => {
    await goPlaylists(catalogPage);

    await catalogPage.getByRole("button", { name: "New playlist" }).click();
    await catalogPage.getByPlaceholder("Playlist name").fill(smokePlaylist);
    await catalogPage.getByRole("button", { name: "Create", exact: true }).click();
    // create() opens the new playlist detail view immediately
    await expect(catalogPage.getByRole("heading", { level: 1, name: smokePlaylist })).toBeVisible({
      timeout: 15_000,
    });
    await expect(catalogPage.getByText("This playlist is empty")).toBeVisible();
    await expect(catalogPage.getByRole("button", { name: "Shuffle" })).toBeVisible();
    await expect(catalogPage.getByTestId("playlist-delete")).toBeVisible();
    await expect(catalogPage.getByTestId("playlist-play")).toBeVisible();
    await captureReport(catalogPage, "smoke-07-playlist");
    await catalogPage.getByRole("button", { name: "Back" }).click();
    await expect(catalogPage.getByRole("heading", { name: "Playlists", exact: true })).toBeVisible();
  });

  test("08 duplicates finder loads", async ({ catalogPage }) => {
    await catalogPage.getByTestId("nav-duplicates").click();
    await expect(catalogPage.getByRole("heading", { name: "Duplicates", exact: true })).toBeVisible({
      timeout: 60_000,
    });
    await catalogPage.getByRole("button", { name: "Refresh" }).click();
    await captureReport(catalogPage, "smoke-08-duplicates");
    await goLibrary(catalogPage);
  });

  test("09 settings scan paths and StashDB section", async ({ catalogPage }) => {
    await catalogPage.getByTestId("nav-settings").click();
    await expect(catalogPage.getByRole("heading", { name: "Settings", exact: true })).toBeVisible();
    await expect(catalogPage.getByRole("heading", { name: "Library folders" })).toBeVisible();
    await expect(catalogPage.getByText("Metadata providers (stash-box)")).toBeVisible();
    await expect(catalogPage.getByText("Import from Stash")).toBeVisible();
    await expect(catalogPage.getByRole("button", { name: /Choose Stash database/i })).toBeVisible();
    await expect(catalogPage.getByTestId("appearance-settings")).toBeVisible();
    await expect(catalogPage.getByRole("heading", { name: "Appearance" })).toBeVisible();
    await expect(catalogPage.getByRole("heading", { name: "Playback volume" })).toBeVisible();
    // The configured scan path (basename of MAIZEVIEW_TEST_LIB) shows in Settings.
    const libBase = testLibPath()!.split(/[\\/]/).filter(Boolean).pop()!;
    await expect(catalogPage.getByText(new RegExp(libBase, "i"))).toBeVisible();
    // Identify stats should load (numbers may be 0 on fresh sandbox).
    await expect(catalogPage.getByText(/to run/)).toBeVisible({ timeout: 15_000 });
    await captureReport(catalogPage, "smoke-09-settings");
    await goLibrary(catalogPage);
  });

  test("10 multiselect mode and batch StashDB button", async ({ catalogPage }) => {
    await goLibrary(catalogPage);
    await expect(catalogPage.getByTestId("stashdb-batch")).toBeVisible();
    await catalogPage.getByTestId("select-mode").click();
    await expect(catalogPage.getByText(/selected/)).toBeVisible();
    await expect(catalogPage.getByRole("button", { name: "Identify" })).toBeVisible();
    await catalogPage.getByRole("button", { name: "Done" }).click();
    await expect(catalogPage.getByTestId("stashdb-batch")).toBeVisible();
    await captureReport(catalogPage, "smoke-10-multiselect");
  });

  test("11 player opens from scene drawer", async ({ catalogPage }) => {
    await goLibrary(catalogPage);
    const clearAll = catalogPage.getByRole("button", { name: "Clear all" });
    if (await clearAll.isVisible().catch(() => false)) {
      await clearAll.click();
      await waitForDebounce(catalogPage);
    }
    await expect(catalogPage.getByTestId("scene-grid-viewport").getByRole("button").first()).toBeVisible({
      timeout: 60_000,
    });

    await openFirstSceneDrawer(catalogPage);

    const labelsBefore = await listWindowLabels(catalogPage);
    // The thumbnail overlay also has an aria-label="Play" button; disambiguate by description.
    await catalogPage
      .getByTestId("scene-drawer")
      .getByRole("button", { name: "Play", description: "Play local file" })
      .click();

    await expect
      .poll(async () => {
        const labels = await listWindowLabels(catalogPage);
        // Focus-existing is OK if a player for this scene is already open from a prior run.
        return (
          labels.some((l) => l.startsWith("player-")) &&
          (labels.length > labelsBefore.length ||
            labelsBefore.some((l) => l.startsWith("player-")))
        );
      }, { timeout: 30_000 })
      .toBe(true);

    await captureReport(catalogPage, "smoke-11-player-catalog");
    await closeSceneDrawer(catalogPage);
  });

  test("12 playlist play and delete", async ({ catalogPage }) => {
    const plName = `e2e-play-del-${Date.now()}`;
    await goLibrary(catalogPage);
    const scenes = await invokeCmd<{ scenes: { id: string }[]; total: number }>(catalogPage, "list_scenes", {
      args: { limit: 1, offset: 0 },
    });
    expect(scenes.scenes.length).toBeGreaterThan(0);
    const sceneId = scenes.scenes[0].id;

    const pl = await invokeCmd<{ id: string; name: string }>(catalogPage, "create_playlist", {
      name: plName,
    });
    await invokeCmd(catalogPage, "add_to_playlist", { playlistId: pl.id, sceneId });

    await goPlaylists(catalogPage);
    await catalogPage.getByRole("button", { name: new RegExp(plName) }).click();
    await expect(catalogPage.getByRole("heading", { level: 1, name: plName })).toBeVisible({
      timeout: 15_000,
    });

    const labelsBefore = await listWindowLabels(catalogPage);
    await catalogPage.getByTestId("playlist-play").click();
    await expect
      .poll(async () => {
        const labels = await listWindowLabels(catalogPage);
        return (
          labels.some((l) => l.startsWith("player-")) &&
          (labels.length > labelsBefore.length ||
            labelsBefore.some((l) => l.startsWith("player-")))
        );
      }, { timeout: 30_000 })
      .toBe(true);
    await captureReport(catalogPage, "smoke-12-playlist-play");

    // The delete confirm is now a NATIVE plugin-dialog, which Playwright cannot
    // accept — delete via the command layer instead. The Playlists view's
    // playlist://changed listener must then close the detail pane and refresh
    // the list (verified by the assertions below).
    await invokeCmd(catalogPage, "delete_playlist", { id: pl.id });
    await expect(catalogPage.getByRole("heading", { name: "Playlists", exact: true })).toBeVisible({
      timeout: 15_000,
    });
    await expect(catalogPage.getByRole("button", { name: new RegExp(plName) })).toHaveCount(0);
    await captureReport(catalogPage, "smoke-12-playlist-deleted");
  });
});
