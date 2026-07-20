import type { Page } from "@playwright/test";
import { testLibPath } from "./env";
import { invokeCmd } from "./tauri";
import { seedSandboxLibrary } from "./seed";

export async function goLibrary(page: Page) {
  await page.getByTestId("nav-library").click();
  await page.getByTestId("library-grid").waitFor({ state: "visible" });
}

/** Open playlists tab and ensure the list view (not a stale detail view). */
export async function goPlaylists(page: Page) {
  await page.getByTestId("nav-playlists").click();
  const back = page.getByRole("button", { name: "Back" });
  for (let i = 0; i < 3; i++) {
    if (!(await back.isVisible().catch(() => false))) break;
    await back.click();
    await page.waitForTimeout(300);
  }
  await page.getByRole("heading", { name: "Playlists", exact: true }).waitFor({
    state: "visible",
    timeout: 15_000,
  });
}

export async function waitForDebounce(page: Page, ms = 800) {
  await page.waitForTimeout(ms);
}

/** Ensure sandbox is indexed; returns scene total. Skips re-seed if already populated. */
export async function ensureSandboxIndexed(page: Page): Promise<number> {
  const lib = testLibPath();
  if (!lib) throw new Error("MAIZEVIEW_TEST_LIB not set in e2e/.env");

  let counts = await invokeCmd<{ total: number; favorites: number }>(page, "scene_counts");
  if (counts.total > 0) return counts.total;

  counts = await seedSandboxLibrary(page, lib);
  return counts.total;
}

export async function openFirstSceneDrawer(page: Page) {
  const grid = page.getByTestId("scene-grid-viewport");
  await grid.getByRole("button").first().click();
  await page.getByTestId("scene-drawer").waitFor({ state: "visible", timeout: 30_000 });
}

export async function closeSceneDrawer(page: Page) {
  await page.getByTestId("scene-drawer").getByRole("button", { name: "Close" }).click();
  await page.getByTestId("scene-drawer").waitFor({ state: "hidden", timeout: 10_000 });
}

export async function clearSearchAndFilters(page: Page) {
  await page.getByTestId("catalog-search").fill("");
  const clearTop = page.getByRole("button", { name: "Clear all" });
  if (await clearTop.isVisible().catch(() => false)) {
    await clearTop.click();
  } else {
    // Open panel only if closed (toggle is not aria-pressed — probe curation gates).
    const gates = page.getByTestId("curation-gates");
    if (!(await gates.isVisible().catch(() => false))) {
      await page.getByTestId("filter-toggle").click();
    }
    const clearFilters = page.getByRole("button", { name: "Clear all filters" });
    if (await clearFilters.isVisible().catch(() => false)) {
      await clearFilters.click();
    }
    const closePanel = page.getByRole("button", { name: "Close filter panel" });
    if (await closePanel.isVisible().catch(() => false)) {
      await closePanel.click();
    } else {
      await page.getByRole("button", { name: "Close", exact: true }).click().catch(() => {});
    }
  }
  // Close saved-filters popover if left open.
  const closeSaved = page.getByRole("button", { name: "Close saved filters" });
  if (await closeSaved.isVisible().catch(() => false)) {
    await closeSaved.click();
  }
  await waitForDebounce(page);
  await goLibrary(page);
}

/** Open the filter popover (no-op if already open). */
export async function openFilterPanel(page: Page) {
  if (await page.getByTestId("curation-gates").isVisible().catch(() => false)) return;
  await page.getByTestId("filter-toggle").click();
  await page.getByTestId("curation-gates").waitFor({ state: "visible", timeout: 10_000 });
}

/** Close filter popover if open. */
export async function closeFilterPanel(page: Page) {
  const closePanel = page.getByRole("button", { name: "Close filter panel" });
  if (await closePanel.isVisible().catch(() => false)) {
    await closePanel.click();
    return;
  }
  if (await page.getByTestId("curation-gates").isVisible().catch(() => false)) {
    await page.getByTestId("filter-toggle").click();
  }
}
