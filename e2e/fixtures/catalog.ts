import { test as base, chromium, type Browser, type Page } from "@playwright/test";
import { cdpUrl } from "../helpers/env";

type CatalogFixtures = {
  catalogPage: Page;
};

let sharedBrowser: Browser | null = null;

async function connectCatalogPage(): Promise<Page> {
  if (!sharedBrowser || !sharedBrowser.isConnected()) {
    sharedBrowser = await chromium.connectOverCDP(cdpUrl());
  }
  const context = sharedBrowser.contexts()[0];
  if (!context) throw new Error("No browser context on CDP endpoint");

  const catalog =
    context.pages().find((p) => p.url().includes("catalog")) ?? context.pages()[0];
  if (!catalog) throw new Error("No catalog page found on CDP endpoint");
  await catalog.bringToFront();
  return catalog;
}

export const test = base.extend<CatalogFixtures>({
  catalogPage: async ({}, use) => {
    const page = await connectCatalogPage();
    await use(page);
  },
});

export { expect } from "@playwright/test";

export async function disconnectCdpBrowser() {
  if (sharedBrowser) {
    await sharedBrowser.close().catch(() => {});
    sharedBrowser = null;
  }
}

/** All pages on the CDP browser (catalog + player windows). */
export async function allCdpPages(): Promise<Page[]> {
  if (!sharedBrowser || !sharedBrowser.isConnected()) {
    sharedBrowser = await chromium.connectOverCDP(cdpUrl());
  }
  return sharedBrowser.contexts().flatMap((c) => c.pages());
}
