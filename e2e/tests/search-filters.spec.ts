import { test, expect } from "../fixtures/catalog";
import { captureReport } from "../helpers/screenshots";
import {
  clearSearchAndFilters,
  closeFilterPanel,
  ensureSandboxIndexed,
  goLibrary,
  openFilterPanel,
  waitForDebounce,
} from "../helpers/nav";
import { invokeCmd } from "../helpers/tauri";
import { testLibPath } from "../helpers/env";

const stamp = Date.now();
const TAG_A = `e2e-search-a-${stamp}`;
const TAG_B = `e2e-search-b-${stamp}`;
const TAG_C = `e2e-search-c-${stamp}`;
const STUDIO = `e2e-studio-${stamp}`;
const PERF = `e2e-perf-${stamp}`;
const SAVED = `e2e-saved-${stamp}`;

type ListResult = { scenes: { id: string }[]; total: number };
type IdName = { id: string; name: string };

async function listScenes(
  page: Parameters<typeof invokeCmd>[0],
  args: Record<string, unknown>,
): Promise<ListResult> {
  return invokeCmd<ListResult>(page, "list_scenes", {
    args: { limit: 10_000, offset: 0, ...args },
  });
}

test.describe.serial("Search filters (curation + saved)", () => {
  test.skip(!testLibPath(), "Set MAIZEVIEW_TEST_LIB in e2e/.env");

  let sceneIds: string[] = [];
  let tagA: IdName;
  let tagB: IdName;
  let tagC: IdName;
  let studio: IdName;
  let performer: IdName;
  let baselineTotal = 0;

  test.beforeEach(async () => {
    test.setTimeout(Number(process.env.E2E_SMOKE_TIMEOUT_MS ?? 180_000));
  });

  test("01 seed tags studios performers on known scenes", async ({ catalogPage }) => {
    baselineTotal = await ensureSandboxIndexed(catalogPage);
    expect(baselineTotal).toBeGreaterThanOrEqual(3);

    const listed = await listScenes(catalogPage, {});
    sceneIds = listed.scenes.slice(0, 3).map((s) => s.id);
    expect(sceneIds.length).toBe(3);

    tagA = await invokeCmd<IdName>(catalogPage, "create_tag", { name: TAG_A });
    tagB = await invokeCmd<IdName>(catalogPage, "create_tag", { name: TAG_B });
    tagC = await invokeCmd<IdName>(catalogPage, "create_tag", { name: TAG_C });
    studio = await invokeCmd<IdName>(catalogPage, "create_studio", { name: STUDIO });
    performer = await invokeCmd<IdName>(catalogPage, "create_performer", { name: PERF });

    // Scene 0: 3 tags + studio + performer (rich)
    for (const tid of [tagA.id, tagB.id, tagC.id]) {
      await invokeCmd(catalogPage, "add_tag_to_scene", { sceneId: sceneIds[0], tagId: tid });
    }
    await invokeCmd(catalogPage, "set_scene_studio", {
      sceneId: sceneIds[0],
      studioId: studio.id,
    });
    await invokeCmd(catalogPage, "add_performer_to_scene", {
      sceneId: sceneIds[0],
      performerId: performer.id,
    });

    // Scene 1: only TAG_A (sparse) — used as exclude-tag false positive without min tags
    await invokeCmd(catalogPage, "add_tag_to_scene", {
      sceneId: sceneIds[1],
      tagId: tagA.id,
    });

    // Scene 2: TAG_A + TAG_B (2 tags), no studio
    await invokeCmd(catalogPage, "add_tag_to_scene", {
      sceneId: sceneIds[2],
      tagId: tagA.id,
    });
    await invokeCmd(catalogPage, "add_tag_to_scene", {
      sceneId: sceneIds[2],
      tagId: tagB.id,
    });

    await goLibrary(catalogPage);
    await captureReport(catalogPage, "search-01-seeded");
  });

  test("02 list_scenes min_tag_count and exclude_tag_ids", async ({ catalogPage }) => {
    const all = await listScenes(catalogPage, {});
    expect(all.total).toBe(baselineTotal);

    // This run's TAG_A appears on all three seeded scenes.
    const withA = await listScenes(catalogPage, { tag_ids: [tagA.id] });
    expect(withA.total).toBeGreaterThanOrEqual(3);
    expect(withA.scenes.some((s) => s.id === sceneIds[0])).toBe(true);
    expect(withA.scenes.some((s) => s.id === sceneIds[1])).toBe(true);
    expect(withA.scenes.some((s) => s.id === sceneIds[2])).toBe(true);

    // Only scene0 received all three tags this run.
    const withAll = await listScenes(catalogPage, {
      tag_ids: [tagA.id, tagB.id, tagC.id],
      tag_match_any: false,
    });
    expect(withAll.total).toBeGreaterThanOrEqual(1);
    expect(withAll.scenes.some((s) => s.id === sceneIds[0])).toBe(true);
    expect(withAll.scenes.some((s) => s.id === sceneIds[1])).toBe(false);

    // Min tags ≥3 + has TAG_A: curated slice (scene0 qualifies; scene1 has only 1 of our tags
    // but may have older tags — require TAG_B too so sparse scene1 drops).
    const curated = await listScenes(catalogPage, {
      min_tag_count: 3,
      tag_ids: [tagA.id, tagB.id],
      tag_match_any: false,
    });
    expect(curated.scenes.some((s) => s.id === sceneIds[0])).toBe(true);
    expect(curated.scenes.some((s) => s.id === sceneIds[1])).toBe(false);

    const excludeOnly = await listScenes(catalogPage, {
      exclude_tag_ids: [tagC.id],
      tag_ids: [tagA.id],
    });
    expect(excludeOnly.scenes.some((s) => s.id === sceneIds[0])).toBe(false);
    expect(excludeOnly.scenes.some((s) => s.id === sceneIds[1])).toBe(true);

    const excludeCurated = await listScenes(catalogPage, {
      exclude_tag_ids: [tagC.id],
      min_tag_count: 3,
      tag_ids: [tagA.id, tagB.id],
      tag_match_any: false,
    });
    expect(excludeCurated.scenes.some((s) => s.id === sceneIds[0])).toBe(false);
    expect(excludeCurated.total).toBeLessThan(excludeOnly.total);
  });

  test("03 list_scenes studio performer resolution identified", async ({ catalogPage }) => {
    const byStudio = await listScenes(catalogPage, { studio_ids: [studio.id] });
    expect(byStudio.total).toBeGreaterThanOrEqual(1);
    expect(byStudio.scenes.some((s) => s.id === sceneIds[0])).toBe(true);

    const exclStudio = await listScenes(catalogPage, { exclude_studio_ids: [studio.id] });
    expect(exclStudio.scenes.some((s) => s.id === sceneIds[0])).toBe(false);

    const minPerf = await listScenes(catalogPage, { min_performer_count: 1 });
    expect(minPerf.scenes.some((s) => s.id === sceneIds[0])).toBe(true);

    // Resolution floor: absurdly high → 0; 1px → most files with height set
    const huge = await listScenes(catalogPage, { min_height: 99_999 });
    expect(huge.total).toBe(0);

    const low = await listScenes(catalogPage, { min_height: 1 });
    expect(low.total).toBeGreaterThan(0);

    // Identified-only is valid even if sandbox has zero applies
    const identified = await listScenes(catalogPage, { identified_only: true });
    expect(identified.total).toBeGreaterThanOrEqual(0);
    expect(identified.total).toBeLessThanOrEqual(baselineTotal);
  });

  test("04 UI curation gates and chips", async ({ catalogPage }) => {
    await goLibrary(catalogPage);
    await clearSearchAndFilters(catalogPage);

    await openFilterPanel(catalogPage);
    await catalogPage.getByTestId("min-tag-count").selectOption("3");
    await closeFilterPanel(catalogPage);
    await waitForDebounce(catalogPage);

    await expect(catalogPage.getByRole("button", { name: /Tags ≥ 3/ })).toBeVisible();

    const viaApi = await listScenes(catalogPage, { min_tag_count: 3 });
    expect(viaApi.total).toBeGreaterThanOrEqual(1);

    await openFilterPanel(catalogPage);
    await catalogPage.getByTestId("identified-only-filter").locator("input").check();
    await catalogPage.getByTestId("min-height").selectOption("720");
    await catalogPage.getByTestId("min-performer-count").selectOption("1");
    await closeFilterPanel(catalogPage);
    await waitForDebounce(catalogPage);

    await expect(catalogPage.getByRole("button", { name: "Identified" })).toBeVisible();
    await expect(catalogPage.getByRole("button", { name: /≥720p/ })).toBeVisible();
    await expect(catalogPage.getByRole("button", { name: /Performers ≥ 1/ })).toBeVisible();
    await captureReport(catalogPage, "search-04-curation-chips");

    await clearSearchAndFilters(catalogPage);
  });

  test("05 UI exclude tag plus min tags", async ({ catalogPage }) => {
    await goLibrary(catalogPage);
    await clearSearchAndFilters(catalogPage);

    await openFilterPanel(catalogPage);
    await catalogPage.getByTestId("min-tag-count").selectOption("3");
    const excludeTagInput = catalogPage.getByPlaceholder("Filter tags to exclude…");
    await excludeTagInput.fill(TAG_C);
    await excludeTagInput.locator("..").getByRole("button", { name: TAG_C, exact: true }).click();
    await closeFilterPanel(catalogPage);
    await waitForDebounce(catalogPage);

    await expect(catalogPage.getByRole("button", { name: new RegExp(`−#${TAG_C}`) })).toBeVisible();
    await expect(catalogPage.getByRole("button", { name: /Tags ≥ 3/ })).toBeVisible();

    const result = await listScenes(catalogPage, {
      exclude_tag_ids: [tagC.id],
      min_tag_count: 3,
    });
    expect(result.scenes.some((s) => s.id === sceneIds[0])).toBe(false);
    await captureReport(catalogPage, "search-05-exclude-curated");

    await clearSearchAndFilters(catalogPage);
  });

  test("06 saved filters save apply delete", async ({ catalogPage }) => {
    await goLibrary(catalogPage);
    await clearSearchAndFilters(catalogPage);

    await openFilterPanel(catalogPage);
    await catalogPage.getByTestId("min-tag-count").selectOption("1");
    await closeFilterPanel(catalogPage);
    await waitForDebounce(catalogPage);

    await catalogPage.getByTestId("saved-filters-toggle").click();
    await expect(catalogPage.getByTestId("saved-filters-panel")).toBeVisible();
    await catalogPage.getByTestId("saved-filter-name").fill(SAVED);
    await catalogPage.getByTestId("saved-filter-save").click();
    await expect(
      catalogPage.getByTestId("saved-filters-panel").getByRole("button", { name: SAVED, exact: true }),
    ).toBeVisible({ timeout: 10_000 });

    await clearSearchAndFilters(catalogPage);
    await expect(catalogPage.getByRole("button", { name: /Tags ≥/ })).toHaveCount(0);

    await catalogPage.getByTestId("saved-filters-toggle").click();
    await catalogPage
      .getByTestId("saved-filters-panel")
      .getByRole("button", { name: SAVED, exact: true })
      .click();
    await waitForDebounce(catalogPage);
    await expect(catalogPage.getByRole("button", { name: /Tags ≥ 1/ })).toBeVisible();
    await captureReport(catalogPage, "search-06-saved-applied");

    await catalogPage.getByTestId("saved-filters-toggle").click();
    await catalogPage.getByRole("button", { name: `Delete ${SAVED}` }).click();
    await expect(
      catalogPage.getByTestId("saved-filters-panel").getByRole("button", { name: SAVED, exact: true }),
    ).toHaveCount(0);
    const closeSaved = catalogPage.getByRole("button", { name: "Close saved filters" });
    if (await closeSaved.isVisible().catch(() => false)) {
      await closeSaved.click();
    }

    await clearSearchAndFilters(catalogPage);
  });

  test("07 UI studio include filter", async ({ catalogPage }) => {
    await goLibrary(catalogPage);
    await clearSearchAndFilters(catalogPage);

    await openFilterPanel(catalogPage);
    const studioInput = catalogPage.getByPlaceholder("Filter studios…").first();
    await studioInput.fill(STUDIO);
    await studioInput.locator("..").getByRole("button", { name: STUDIO, exact: true }).click();
    await closeFilterPanel(catalogPage);
    await waitForDebounce(catalogPage);

    await expect(catalogPage.getByRole("button", { name: new RegExp(`\\+@${STUDIO}`) })).toBeVisible();
    const byStudio = await listScenes(catalogPage, { studio_ids: [studio.id] });
    expect(byStudio.scenes.some((s) => s.id === sceneIds[0])).toBe(true);
    await captureReport(catalogPage, "search-07-studio");

    await clearSearchAndFilters(catalogPage);
  });

  test("08 free-text search still finds tag names", async ({ catalogPage }) => {
    await goLibrary(catalogPage);
    await clearSearchAndFilters(catalogPage);

    await catalogPage.getByTestId("catalog-search").fill(TAG_A);
    await waitForDebounce(catalogPage);
    const hit = await listScenes(catalogPage, { search: TAG_A });
    expect(hit.total).toBeGreaterThanOrEqual(3);
    await captureReport(catalogPage, "search-08-text-tag");

    await catalogPage.getByTestId("catalog-search").fill(`${TAG_A} -${TAG_C}`);
    await waitForDebounce(catalogPage);
    const excl = await listScenes(catalogPage, {
      search: TAG_A,
      search_exclude_terms: [TAG_C],
    });
    // Scene0 matches TAG_A and TAG_C name on tags → exclude term drops it via tag name LIKE
    expect(excl.scenes.some((s) => s.id === sceneIds[0])).toBe(false);
    expect(excl.total).toBeGreaterThanOrEqual(1);

    await catalogPage.getByTestId("catalog-search").fill("");
    await waitForDebounce(catalogPage);
  });

  test("09 ignore-state filter (API + UI)", async ({ catalogPage }) => {
    await goLibrary(catalogPage);
    await clearSearchAndFilters(catalogPage);

    // Ignore one seeded scene via the batch command the multiselect bar uses.
    await invokeCmd(catalogPage, "batch_set_stashdb_ignore", {
      sceneIds: [sceneIds[2]],
      ignored: true,
    });

    const onlyIgnored = await listScenes(catalogPage, { ignored: true });
    expect(onlyIgnored.scenes.some((s) => s.id === sceneIds[2])).toBe(true);
    expect(onlyIgnored.scenes.some((s) => s.id === sceneIds[0])).toBe(false);

    const notIgnored = await listScenes(catalogPage, { ignored: false });
    expect(notIgnored.scenes.some((s) => s.id === sceneIds[2])).toBe(false);
    expect(notIgnored.scenes.some((s) => s.id === sceneIds[0])).toBe(true);
    expect(notIgnored.total).toBe(baselineTotal - 1);

    // UI: panel select drives the chip + grid query.
    await openFilterPanel(catalogPage);
    await catalogPage.getByTestId("ignore-state").selectOption("ignored");
    await closeFilterPanel(catalogPage);
    await waitForDebounce(catalogPage);
    await expect(catalogPage.getByRole("button", { name: "Ignored", exact: true })).toBeVisible();
    await captureReport(catalogPage, "search-09-ignore-state");

    await openFilterPanel(catalogPage);
    await catalogPage.getByTestId("ignore-state").selectOption("not_ignored");
    await closeFilterPanel(catalogPage);
    await waitForDebounce(catalogPage);
    await expect(catalogPage.getByRole("button", { name: "Not ignored", exact: true })).toBeVisible();

    // Cleanup: filter back to Any, unignore the scene for later suites.
    await openFilterPanel(catalogPage);
    await catalogPage.getByTestId("ignore-state").selectOption("any");
    await closeFilterPanel(catalogPage);
    await invokeCmd(catalogPage, "batch_set_stashdb_ignore", {
      sceneIds: [sceneIds[2]],
      ignored: false,
    });
    const restored = await listScenes(catalogPage, {});
    expect(restored.total).toBe(baselineTotal);
  });

  test("10 folder facet (API + UI)", async ({ catalogPage }) => {
    await goLibrary(catalogPage);
    await clearSearchAndFilters(catalogPage);

    type FolderRow = { path: string; name: string; file_count: number };
    const folderList = await invokeCmd<FolderRow[]>(catalogPage, "list_folders");
    expect(folderList.length).toBeGreaterThan(0);
    // Most-represented folder: stable target regardless of sandbox layout.
    const folder = [...folderList].sort((a, b) => b.file_count - a.file_count)[0];

    type GridRow = { id: string; file_path: string | null };
    const res = await invokeCmd<{ scenes: GridRow[]; total: number }>(catalogPage, "list_scenes", {
      args: { limit: 10_000, offset: 0, folder_paths: [folder.path] },
    });
    expect(res.total).toBeGreaterThanOrEqual(1);
    // Scenes ≤ files in the folder (multi-file scenes collapse).
    expect(res.total).toBeLessThanOrEqual(folder.file_count);
    // Recursive match: returned scenes live under the folder prefix.
    expect(
      res.scenes.some((s) => s.file_path != null && s.file_path.startsWith(folder.path)),
    ).toBe(true);

    // UI: facet chips render, selection drives the topbar chip.
    await openFilterPanel(catalogPage);
    const facet = catalogPage.getByTestId("folder-facet");
    await catalogPage.getByPlaceholder("Filter folders…").fill(folder.name);
    await facet.getByRole("button", { name: new RegExp(folder.name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")) }).first().click();
    await closeFilterPanel(catalogPage);
    await waitForDebounce(catalogPage);
    await expect(
      catalogPage.getByRole("button", { name: new RegExp(`\\+/${folder.name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}`) }),
    ).toBeVisible();
    await captureReport(catalogPage, "search-10-folder-facet");

    await clearSearchAndFilters(catalogPage);
  });
});
