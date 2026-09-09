import { expect, test, type Page } from "@playwright/test";
import type { ConnLensSnapshot, Connection } from "../src/lib/types";

const now = "2026-09-10T10:00:00.000Z";
function row(id: string, provider: string, name: string, label: string, status: Connection["status"] = "active"): Connection {
  return {
    id, provider, providerName: name,
    identity: { label, host: `${provider}.example.test`, scope: "Fixture account", isActiveIdentity: true },
    source: { sourceType: "config_file", path: `/fixtures/${provider}/config.json`, descriptorId: provider },
    status, fingerprint: "sha256:12345678", firstSeen: now, lastSeen: now,
    hidden: false, seen: true, meta: {}, removable: status === "missing",
  };
}
const fixture: ConnLensSnapshot = {
  schemaVersion: 1, lastScan: now, watcherHealth: "ok", historyResetNotice: false, providerErrors: [],
  settings: {
    watchersEnabled: true, pollMinutes: 10, toastsEnabled: true, probesEnabled: false,
    providerToggles: {}, projectRoots: [], theme: "dark", collapsedProviders: {}, showHidden: false,
    historyResetNoticeDismissed: false, autostart: false,
  },
  connections: [row("gh-main", "github", "GitHub", "dev@fixture.test"), row("vercel-main", "vercel", "Vercel", "site@fixture.test"), row("old", "neon", "Neon DB", "old@fixture.test", "missing")],
};

async function installIpcFixture(page: Page, platform: string) {
  // This test-only IPC stand-in never starts Tauri, reads local config, changes login items,
  // copies to the real clipboard, opens external URLs, or quits a real application.
  await page.addInitScript(({ snapshot, platform }) => {
    const host = window as unknown as Record<string, any>;
    const calls: { command: string; args: any }[] = [];
    const callbacks = new Map<number, (event: unknown) => void>();
    const listeners = new Map<number, number>();
    let nextId = 1;
    let state = structuredClone(snapshot);
    host.__CONNLENS_TEST__ = {
      calls,
      failNext: null,
      emit(next: typeof snapshot) {
        state = structuredClone(next);
        for (const [id, callbackId] of listeners) callbacks.get(callbackId)?.({ event: "state://updated", id, payload: state });
      },
      state: () => structuredClone(state),
    };
    host.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: (_event: string, id: number) => listeners.delete(id) };
    host.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "popover" }, currentWebview: { label: "popover" } },
      transformCallback(callback: (event: unknown) => void) { const id = nextId++; callbacks.set(id, callback); return id; },
      unregisterCallback(id: number) { callbacks.delete(id); },
      async invoke(command: string, args: any = {}) {
        calls.push({ command, args });
        if (host.__CONNLENS_TEST__.failNext === command) {
          host.__CONNLENS_TEST__.failNext = null;
          throw { code: "fixture_error", message: "Fixture scan unavailable" };
        }
        switch (command) {
          case "get_platform": return platform;
          case "get_state": return structuredClone(state);
          case "rescan": state.lastScan = new Date().toISOString(); return structuredClone(state);
          case "update_settings": state.settings = args.settings; return structuredClone(state);
          case "remove": state.connections = state.connections.filter((row) => row.id !== args.id); return structuredClone(state);
          case "add_custom_provider": {
            const connection = structuredClone(state.connections[0]);
            connection.id = "custom-fixture";
            connection.provider = args.input.id;
            connection.providerName = args.input.name;
            connection.identity.label = args.input.name;
            connection.status = "unverified";
            state.connections.push(connection);
            return structuredClone(state);
          }
          case "dismiss_history_reset_notice": state.historyResetNotice = false; return structuredClone(state);
          case "copy_value": case "quit_app": case "plugin:window|hide": case "plugin:window|start_dragging": return;
          case "plugin:event|listen": { const id = nextId++; listeners.set(id, args.handler); return id; }
          case "plugin:event|unlisten": listeners.delete(args.eventId); return;
          default: throw new Error(`Unexpected native command in browser test: ${command}`);
        }
      },
    };
  }, { snapshot: fixture, platform });
}

test.beforeEach(async ({ page }, testInfo) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("console", (message) => { if (message.type() === "error") errors.push(message.text()); });
  await page.route("**/*", (route) => {
    const url = new URL(route.request().url());
    if (url.hostname !== "127.0.0.1") {
      errors.push(`Unexpected external request: ${url.origin}`);
      return route.abort();
    }
    return route.continue();
  });
  await installIpcFixture(page, String(testInfo.project.metadata.platform));
  await page.goto("/");
  await expect(page).toHaveTitle(/ConnLens/);
  await expect(page.getByRole("main")).toBeVisible();
  const logo = page.getByRole("img", { name: "ConnLens logo", exact: true });
  await expect(logo).toBeVisible();
  await expect.poll(() => logo.evaluate((image: HTMLImageElement) => image.complete && image.naturalWidth > 0)).toBe(true);
  await expect(page.getByText("dev@fixture.test", { exact: true })).toBeVisible();
  expect(errors).toEqual([]);
  (testInfo as any).appErrors = errors;
});

test.afterEach(async ({}, testInfo) => {
  expect((testInfo as any).appErrors ?? []).toEqual([]);
});

test("provider filters and search retain the compact panel layout", async ({ page }, testInfo) => {
  await page.getByRole("button", { name: "GitHub connections", exact: true }).click();
  await expect(page.getByText("dev@fixture.test", { exact: true })).toBeVisible();
  await expect(page.getByText("site@fixture.test", { exact: true })).toBeHidden();
  await page.getByRole("button", { name: "All providers", exact: true }).click();
  await page.getByRole("textbox", { name: "Search connections" }).fill("no-such-fixture");
  await expect(page.getByText("No matching connections", { exact: true })).toBeVisible();
  await page.getByRole("textbox", { name: "Search connections" }).fill("site@fixture");
  await expect(page.getByText("site@fixture.test", { exact: true })).toBeVisible();
  await expect(page.getByText("dev@fixture.test", { exact: true })).toBeHidden();
  await page.getByRole("textbox", { name: "Search connections" }).clear();
  const bounds = await page.getByRole("button", { name: "Quit ConnLens" }).boundingBox();
  expect(bounds).not.toBeNull();
  expect(bounds!.y + bounds!.height).toBeLessThanOrEqual(testInfo.project.use.viewport!.height);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  await page.screenshot({ path: testInfo.outputPath("connections.png") });
});

test("connection details copy through IPC and protect active entries", async ({ page }) => {
  await page.getByRole("button", { name: /dev@fixture\.test/ }).click();
  await expect(page.getByRole("button", { name: "Copy identity", exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Remove entry", exact: true })).toBeDisabled();
  await page.getByRole("button", { name: "Copy identity", exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__CONNLENS_TEST__.calls.some((call: any) => call.command === "copy_value" && call.args.id === "gh-main" && call.args.field === "identity"))).toBe(true);
  await expect(page.getByText("Copied", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: /old@fixture\.test/ }).click();
  await expect(page.getByRole("button", { name: "Remove entry", exact: true })).toBeEnabled();
  await page.getByRole("button", { name: "Remove entry", exact: true }).click();
  await expect(page.getByText("old@fixture.test", { exact: true })).toBeHidden();
});

test("settings persist and watcher events update the list", async ({ page }, testInfo) => {
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  const watcher = page.getByRole("switch", { name: "Watch local files" });
  await expect(watcher).toBeChecked();
  await watcher.uncheck();
  await expect.poll(() => page.evaluate(() => (window as any).__CONNLENS_TEST__.state().settings.watchersEnabled)).toBe(false);
  await page.screenshot({ path: testInfo.outputPath("settings.png") });
  await page.getByRole("button", { name: "Connections", exact: true }).click();
  await page.evaluate(() => {
    const fixture = (window as any).__CONNLENS_TEST__;
    const next = fixture.state();
    next.connections[0].identity.label = "updated@fixture.test";
    fixture.emit(next);
  });
  await expect(page.getByText("updated@fixture.test", { exact: true })).toBeVisible();
  await expect(page.getByText("dev@fixture.test", { exact: true })).toBeHidden();
});

test("custom provider form preserves a failed submission and clears a successful one", async ({ page }) => {
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.locator("summary").filter({ hasText: "Custom providers" }).click();
  const name = page.getByRole("textbox", { name: "Custom provider name", exact: true });
  await name.fill("Fixture CLI");
  await page.getByRole("textbox", { name: "Custom provider ID", exact: true }).fill("fixture-cli");
  await page.getByRole("button", { name: "Add custom provider", exact: true }).click();
  await expect(page.getByText("Add at least one config path or environment variable.", { exact: true })).toBeVisible();
  await page.getByRole("textbox", { name: "Custom provider config paths", exact: true }).fill("~/.fixture-cli/config.json");
  await page.evaluate(() => { (window as any).__CONNLENS_TEST__.failNext = "add_custom_provider"; });
  await page.getByRole("button", { name: "Add custom provider", exact: true }).click();
  await expect(page.getByText("Fixture scan unavailable", { exact: true })).toBeVisible();
  await expect(name).toHaveValue("Fixture CLI");
  await page.getByRole("button", { name: "Add custom provider", exact: true }).click();
  await expect(name).toHaveValue("");
  await expect.poll(() => page.evaluate(() => (window as any).__CONNLENS_TEST__.state().connections.some((row: any) => row.provider === "fixture-cli"))).toBe(true);
});

test("scan errors keep existing data usable and Quit invokes the native action", async ({ page }) => {
  await page.evaluate(() => { (window as any).__CONNLENS_TEST__.failNext = "rescan"; });
  await page.getByRole("button", { name: "Rescan", exact: true }).click();
  await expect(page.getByText("Fixture scan unavailable", { exact: true })).toBeVisible();
  await expect(page.getByText("dev@fixture.test", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Rescan", exact: true }).click();
  await expect(page.getByText("Fixture scan unavailable", { exact: true })).toBeHidden();
  await page.getByRole("button", { name: "Quit ConnLens", exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__CONNLENS_TEST__.calls.some((call: any) => call.command === "quit_app"))).toBe(true);
});
