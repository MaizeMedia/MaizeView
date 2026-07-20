import type { Page } from "@playwright/test";

declare global {
  interface Window {
    __TAURI__?: {
      core?: {
        invoke: (cmd: string, payload?: Record<string, unknown>) => Promise<unknown>;
      };
      webviewWindow?: {
        getAllWebviewWindows: () => Promise<Array<{ label: string }>>;
      };
    };
  }
}

/** List Tauri webview window labels (catalog + player windows). */
export async function listWindowLabels(page: Page): Promise<string[]> {
  return page.evaluate(async () => {
    const getAll = window.__TAURI__?.webviewWindow?.getAllWebviewWindows;
    if (!getAll) {
      throw new Error(
        "window.__TAURI__.webviewWindow unavailable. Run with withGlobalTauri enabled.",
      );
    }
    return (await getAll()).map((w) => w.label);
  });
}

/** Invoke a Tauri command inside the catalog webview (CDP mode only). */
export async function invokeCmd<T = unknown>(
  page: Page,
  cmd: string,
  payload: Record<string, unknown> = {},
): Promise<T> {
  return page.evaluate(
    async ({ cmd, payload }) => {
      const invoke = window.__TAURI__?.core?.invoke;
      if (!invoke) {
        throw new Error(
          "Tauri IPC unavailable. Run tests against the real app (npm run e2e:app) with CDP enabled.",
        );
      }
      return invoke(cmd, payload) as Promise<T>;
    },
    { cmd, payload },
  );
}
