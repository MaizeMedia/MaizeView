import { test, expect } from "../fixtures/catalog";
import { captureReport } from "../helpers/screenshots";
import { ensureSandboxIndexed, goLibrary, waitForDebounce } from "../helpers/nav";
import { invokeCmd } from "../helpers/tauri";
import { testLibPath } from "../helpers/env";

// Downscale / Convert dialog e2e.
//
// This suite exercises the dialog UI and the non-mutating `downscale_preview`
// command (breakdown, estimated savings, before→after filename preview). It
// does NOT run a real transcode — that's covered by the ignored Rust
// integration test (`transcode_e2e.rs`) to keep the e2e suite fast and stable.
//
// Requires the running e2e app + MAIZEVIEW_TEST_LIB, like the other suites.

interface SceneGridRow {
  id: string;
  height: number | null;
}

interface DownscalePreviewItem {
  sceneId: string;
  currentHeight: number | null;
  currentPath: string | null;
  wouldSkip: boolean;
  previewFilename: string | null;
}

interface DownscalePreview {
  targetHeight: number;
  total: number;
  wouldTranscode: number;
  skipped: number;
  byResolution: Record<string, number>;
  estimatedBytesSaved: number;
  items: DownscalePreviewItem[];
}

test.describe.serial("Convert / downscale dialog", () => {
  test.skip(!testLibPath(), "Set MAIZEVIEW_TEST_LIB in e2e/.env");

  test.beforeEach(async () => {
    test.setTimeout(Number(process.env.E2E_SMOKE_TIMEOUT_MS ?? 180_000));
  });

  test("downscale_preview returns correct plan shape", async ({ catalogPage }) => {
    await ensureSandboxIndexed(catalogPage);
    await goLibrary(catalogPage);

    // Grab up to 8 real scene IDs + heights from the catalog.
    const list = await invokeCmd<{ scenes: SceneGridRow[]; total: number }>(
      catalogPage,
      "list_scenes",
      { args: { limit: 8, offset: 0 } },
    );
    expect(list.scenes.length).toBeGreaterThan(0);
    const ids = list.scenes.map((s) => s.id);

    // Target 720 — non-mutating. Whatever the source heights, the shape must hold.
    const preview = await invokeCmd<DownscalePreview>(catalogPage, "downscale_preview", {
      sceneIds: ids,
      targetHeight: 720,
    });

    expect(preview.total).toBe(ids.length);
    expect(preview.wouldTranscode + preview.skipped).toBe(preview.total);
    expect(preview.items).toHaveLength(ids.length);
    // Every item carries the target back and a (possibly null) filename preview.
    expect(preview.targetHeight).toBe(720);
    for (const item of preview.items) {
      expect(typeof item.sceneId).toBe("string");
      // A scene taller than 720 should not be flagged as skip; shorter should.
      if (item.currentHeight !== null) {
        expect(item.wouldSkip).toBe(item.currentHeight <= 720);
      }
    }
  });

  test("dialog renders breakdown and option controls", async ({ catalogPage }) => {
    await ensureSandboxIndexed(catalogPage);
    await goLibrary(catalogPage);

    // Clear any leftover search/filter so the grid shows scenes.
    await catalogPage.getByTestId("catalog-search").fill("");
    await waitForDebounce(catalogPage);
    await expect(
      catalogPage.getByTestId("scene-grid-viewport").getByRole("button").first(),
    ).toBeVisible({ timeout: 60_000 });

    // Enter selection mode and select the first two scenes.
    await catalogPage.getByTestId("select-mode").click();
    await expect(catalogPage.getByText(/selected/)).toBeVisible();

    const cards = catalogPage.getByTestId("scene-grid-viewport").getByRole("button");
    await cards.nth(0).click();
    // Ensure at least one is selected (a second card may not always be clickable
    // in a tiny sandbox, so guard rather than require two).
    const selectedCount = await catalogPage
      .getByText(/(\d+)\s+selected/)
      .textContent()
      .catch(() => null);
    expect(selectedCount, "should show a selection count").not.toBeNull();

    // The Convert… button is present and opens the dialog.
    await expect(catalogPage.getByRole("button", { name: /Convert/i })).toBeVisible();
    await catalogPage.getByRole("button", { name: /Convert/i }).click();
    await expect(catalogPage.getByTestId("convert-dialog")).toBeVisible({ timeout: 15_000 });
    await captureReport(catalogPage, "convert-01-dialog-open");

    // Breakdown panel renders (bucket chips keyed by resolution token).
    await expect(catalogPage.getByTestId("convert-breakdown")).toBeVisible();

    // All three target options are selectable.
    const target = catalogPage.getByTestId("convert-target");
    await expect(target).toBeVisible();
    await target.selectOption("1080");

    // Original-handling radios exist.
    await expect(catalogPage.getByTestId("convert-original-replace")).toBeVisible();
    await expect(catalogPage.getByTestId("convert-original-keep")).toBeVisible();

    // Filename + tag radios exist.
    await expect(catalogPage.getByTestId("convert-filename-replace")).toBeVisible();
    await expect(catalogPage.getByTestId("convert-tag-swap")).toBeVisible();
    await captureReport(catalogPage, "convert-02-options");

    // Confirm button is disabled when nothing would transcode OR enabled when
    // something would. We assert only that it exists (its state depends on the
    // sandbox's actual resolutions, which vary).
    await expect(catalogPage.getByTestId("convert-confirm")).toBeVisible();

    // Cancel via Escape closes the dialog without mutating anything.
    await catalogPage.keyboard.press("Escape");
    await expect(catalogPage.getByTestId("convert-dialog")).toBeHidden({ timeout: 10_000 });

    // Exit selection mode to leave a clean grid for later suites.
    await catalogPage.getByRole("button", { name: "Done" }).click().catch(() => {});
    await expect(catalogPage.getByTestId("stashdb-batch")).toBeVisible();
  });
});
