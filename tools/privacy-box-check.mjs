// Verify privacy box collapse cycle + screenshot the sidebar.
import { chromium } from "@playwright/test";

const browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
const catalog = browser
  .contexts()
  .flatMap((c) => c.pages())
  .find((p) => p.url().includes("catalog.html"));
if (!catalog) throw new Error("no catalog page");

await catalog.getByLabel("Collapse privacy box").click();
await catalog.waitForTimeout(400);
const row = await catalog.evaluate(() => !!document.querySelector('[data-testid="privacy-expand"]'));
console.log("collapsed row visible:", row);
console.log("localStorage:", await catalog.evaluate(() => localStorage.getItem("privacyBoxCollapsed")));

await catalog.getByTestId("privacy-expand").click();
await catalog.waitForTimeout(400);
const aside = await catalog.$("aside");
await aside.screenshot({ path: "docs/e2e-reports/privacy-box.png" });
console.log("screenshot saved");
await browser.close();
