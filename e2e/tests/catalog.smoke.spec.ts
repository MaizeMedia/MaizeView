import { test, expect } from "../fixtures/catalog";
import { captureReport } from "../helpers/screenshots";

test.describe("Catalog smoke", () => {
  test("loads the library shell", async ({ catalogPage }) => {
    await expect(catalogPage.getByTestId("catalog-search")).toBeVisible();
    await expect(catalogPage.getByTestId("nav-library")).toBeVisible();
    await expect(catalogPage.getByTestId("library-grid")).toBeVisible();
    await captureReport(catalogPage, "catalog-loaded");
  });

  test("navigates to tags view", async ({ catalogPage }) => {
    await catalogPage.getByTestId("nav-tags").click();
    await expect(catalogPage.getByRole("heading", { name: "Tags", exact: true })).toBeVisible();
    await captureReport(catalogPage, "tags-view");
    await catalogPage.getByTestId("nav-library").click();
    await expect(catalogPage.getByTestId("library-grid")).toBeVisible();
  });
});
