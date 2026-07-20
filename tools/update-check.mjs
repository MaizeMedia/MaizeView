// Live check of Settings → About → Check for updates.
import { chromium } from "@playwright/test";

const browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
const catalog = browser
  .contexts()
  .flatMap((c) => c.pages())
  .find((p) => p.url().includes("catalog.html"));
if (!catalog) throw new Error("no catalog page");

// Go to Settings.
await catalog.getByTestId("nav-settings").click();
await catalog.waitForTimeout(1500);

const version = await catalog.evaluate(() => {
  const el = document.querySelector('[data-testid="about-section"]');
  return el ? el.textContent.replace(/\s+/g, " ").trim().slice(0, 120) : null;
});
console.log("about section:", version);

await catalog.getByRole("button", { name: "Check for updates" }).click();
await catalog.waitForTimeout(4000);

const result = await catalog.evaluate(() => {
  const el = document.querySelector('[data-testid="about-section"]');
  return el ? el.textContent.replace(/\s+/g, " ").trim() : null;
});
console.log("after check:", result);
await browser.close();
