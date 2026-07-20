// Verify the Donate popover: row visible, dialog opens, QR + copy render.
import { chromium } from "@playwright/test";

const browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
const catalog = browser
  .contexts()
  .flatMap((c) => c.pages())
  .find((p) => p.url().includes("catalog.html"));
if (!catalog) throw new Error("no catalog page");

await catalog.reload();
await catalog.waitForTimeout(2000);

const hasRow = await catalog.evaluate(() => !!document.querySelector('[data-testid="donate-open"]'));
console.log("donate row:", hasRow);

await catalog.getByTestId("donate-open").click();
await catalog.waitForTimeout(1200);
const info = await catalog.evaluate(() => {
  const dlg = document.querySelector('[data-testid="donate-dialog"]');
  if (!dlg) return { open: false };
  return {
    open: true,
    hasQr: !!dlg.querySelector('[data-testid="donate-qr"] svg'),
    text: dlg.textContent.replace(/\s+/g, " ").trim().slice(0, 220),
  };
});
console.log(JSON.stringify(info, null, 1));

await catalog.screenshot({ path: "docs/e2e-reports/donate-dialog.png" });
console.log("screenshot saved");
await browser.close();
