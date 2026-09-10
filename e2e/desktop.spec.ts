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
    validation: { availability: status === "missing" ? "missing" : "available", usage: status === "missing" ? "unknown" : "selected",
      checkedAt: now, reason: status === "missing" ? "Source was confirmed missing" : "Selected account exists in local config", reasonCode: "fixture" },
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
    let reviewId = 0;
    let state = structuredClone(snapshot);
    host.__CONNLENS_TEST__ = {
      calls,
      failNext: null,
      cleanupMode: "success",
      cleanupFiles: [
        { id: "old-file", path: "/fixtures/neon/leftover.json", eligible: true, reason: "Reviewed leftover file", sizeBytes: 48 },
        { id: "shared-file", path: "/fixtures/github/hosts.yml", eligible: false, reason: "Shared by another detected account", sizeBytes: 120 },
      ],
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
          case "review_cleanup": {
            state.lastScan = new Date().toISOString();
            return { reviewId: `review-${++reviewId}`, checkedAt: state.lastScan, snapshot: structuredClone(state),
              entries: state.connections.map((connection) => ({ id: connection.id, label: connection.identity.label,
                providerName: connection.providerName, eligible: connection.status === "missing",
                reason: connection.status === "missing" ? "Source was confirmed missing" : "Still available in local config",
                ...(connection.id === "old" ? { fileId: "old-file" } : {}) })),
              files: structuredClone(host.__CONNLENS_TEST__.cleanupFiles) };
          }
          case "execute_cleanup": {
            const request = args.request;
            if (request.reviewId !== `review-${reviewId}`) throw { code: "stale_review", message: "Review is out of date" };
            if (host.__CONNLENS_TEST__.cleanupMode === "stale") throw { code: "stale_review", message: "The source changed. Review again." };
            const eligibleIds = state.connections.filter((connection) => connection.status === "missing").map((connection) => connection.id);
            const removedIds = request.connectionIds.filter((id: string) => eligibleIds.includes(id));
            const retained = request.connectionIds.filter((id: string) => !eligibleIds.includes(id)).map((id: string) => ({ id, reason: "Still available in local config" }));
            state.connections = state.connections.filter((connection) => !removedIds.includes(connection.id));
            const files = host.__CONNLENS_TEST__.cleanupFiles.filter((file: any) => request.fileIds.includes(file.id));
            const partial = host.__CONNLENS_TEST__.cleanupMode === "partial";
            return { snapshot: structuredClone(state), removedIds, retained,
              trashedPaths: partial ? [] : files.filter((file: any) => file.eligible).map((file: any) => file.path),
              fileFailures: files.filter((file: any) => partial || !file.eligible).map((file: any) => ({ id: file.id, reason: partial ? "Could not move the reviewed file to Trash" : file.reason })) };
          }
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

test("CLI removal stays separate from account availability and protected cleanup", async ({ page }, testInfo) => {
  await page.evaluate(() => {
    const fixture = (window as any).__CONNLENS_TEST__;
    const next = fixture.state();
    next.connections[0].meta.toolPresence = { status: "missing", name: "gh", path: "/fixtures/bin/gh",
      checkedAt: "2026-09-10T10:00:00.000Z", reasonCode: "observed_executable_missing",
      reason: "The previously found executable is absent. Account configuration is checked separately." };
    fixture.emit(next);
  });
  await expect(page.getByText(/^CLI removed/)).toBeVisible();
  await expect(page.getByText("Config reference remains", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: /dev@fixture.test.*Available/ }).click();
  await expect(page.getByText("Available locally", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Review removal", exact: true }).click();
  await expect(page.getByRole("checkbox", { name: "Select dev@fixture.test", exact: true })).toBeDisabled();
  await expect(page.getByText("gh · CLI removed", { exact: true })).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath("tool-removal-review.png") });
  await page.getByRole("button", { name: "Connections", exact: true }).click();
  await page.evaluate(() => {
    const fixture = (window as any).__CONNLENS_TEST__;
    const next = fixture.state();
    next.connections[0].meta.toolPresence.status = "found";
    fixture.emit(next);
  });
  await expect(page.getByText(/^CLI removed/)).toBeHidden();
  await expect(page.getByRole("button", { name: /dev@fixture.test.*Available/ })).toHaveCount(1);
});

test("removed MCP registrations can be reviewed and forgotten", async ({ page }, testInfo) => {
  await page.evaluate(() => {
    const fixture = (window as any).__CONNLENS_TEST__;
    const next = fixture.state();
    const server = structuredClone(next.connections[0]);
    server.id = "mcp-fixture";
    server.provider = "mcp_servers";
    server.providerName = "MCP servers";
    server.identity = { label: "Local helper", host: null, scope: "Codex", isActiveIdentity: false };
    server.source = { sourceType: "config_file", path: "/fixtures/.codex/config.toml", descriptorId: "mcp_servers" };
    server.meta = { mcpClient: "Codex", mcpTransport: "stdio", mcpCommand: "npx" };
    next.connections.push(server);
    fixture.emit(next);
  });
  await page.getByRole("button", { name: /Local helper.*Registered/ }).click();
  await expect(page.getByRole("button", { name: "Open dashboard", exact: true })).toBeDisabled();
  await page.evaluate(() => {
    const fixture = (window as any).__CONNLENS_TEST__;
    const next = fixture.state();
    const server = next.connections.find((row: any) => row.id === "mcp-fixture");
    server.status = "missing";
    server.validation = { availability: "missing", usage: "unknown", checkedAt: "2026-09-10T10:01:00.000Z",
      reason: "The MCP registration was removed from its configuration.", reasonCode: "mcp_registration_missing" };
    fixture.emit(next);
  });
  await expect(page.getByRole("button", { name: /Local helper.*Removed/ })).toBeVisible();
  await page.getByRole("button", { name: "Review removal", exact: true }).click();
  await expect(page.getByRole("checkbox", { name: "Select Local helper", exact: true })).toBeChecked();
  await page.screenshot({ path: testInfo.outputPath("mcp-removal-review.png") });
  await page.getByRole("button", { name: "Remove selected", exact: true }).click();
  await expect(page.getByText("1 entry removed", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Connections", exact: true }).click();
  await expect(page.getByText("Local helper", { exact: true })).toBeHidden();
  await expect(page.getByText("dev@fixture.test", { exact: true })).toBeVisible();
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

test("connection details copy through IPC and require a fresh cleanup review", async ({ page }) => {
  await page.getByRole("button", { name: /dev@fixture\.test/ }).click();
  await expect(page.getByRole("button", { name: "Copy identity", exact: true })).toBeVisible();
  await expect(page.getByText("Available locally", { exact: true })).toBeVisible();
  await expect(page.getByText("Selected in local config", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Copy identity", exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__CONNLENS_TEST__.calls.some((call: any) => call.command === "copy_value" && call.args.id === "gh-main" && call.args.field === "identity"))).toBe(true);
  await expect(page.getByText("Copied", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Review removal", exact: true }).click();
  await expect(page.getByRole("checkbox", { name: "Select dev@fixture.test", exact: true })).toBeDisabled();
  await expect(page.getByRole("button", { name: "Remove selected", exact: true })).toBeDisabled();
  await page.getByRole("button", { name: "Connections", exact: true }).click();
  await page.getByRole("button", { name: /old@fixture\.test/ }).click();
  await page.getByRole("button", { name: "Review removal", exact: true }).click();
  await expect(page.getByRole("checkbox", { name: "Select old@fixture.test", exact: true })).toBeChecked();
  expect(await page.evaluate(() => (window as any).__CONNLENS_TEST__.calls.filter((call: any) => call.command === "execute_cleanup").length)).toBe(0);
  await page.getByRole("button", { name: "Remove selected", exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__CONNLENS_TEST__.state().connections.some((connection: any) => connection.id === "old"))).toBe(false);
  const calls = await page.evaluate(() => (window as any).__CONNLENS_TEST__.calls);
  expect(calls.filter((call: any) => call.command === "review_cleanup")).toHaveLength(2);
  expect(calls.find((call: any) => call.command === "execute_cleanup").args.request).toEqual({ reviewId: "review-2", connectionIds: ["old"], fileIds: [] });
  expect(calls.some((call: any) => ["remove", "purge_missing"].includes(call.command))).toBe(false);
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
    next.connections[0].validation = { availability: "available", usage: "referenced", checkedAt: "2026-09-10T10:01:00.000Z",
      reason: "Another local profile is selected", reasonCode: "profile_reference" };
    fixture.emit(next);
  });
  await expect(page.getByText("updated@fixture.test", { exact: true })).toBeVisible();
  await expect(page.getByText("dev@fixture.test", { exact: true })).toBeHidden();
  await page.getByRole("button", { name: /updated@fixture\.test/ }).click();
  await expect(page.getByText("Available locally", { exact: true })).toBeVisible();
  await expect(page.getByText("Referenced locally", { exact: true })).toBeVisible();
  await expect(page.getByText("Another local profile is selected", { exact: true })).toBeVisible();
});

test("cleanup files require explicit opt-in and keep shared sources blocked", async ({ page }, testInfo) => {
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("button", { name: "Review cleanup", exact: true }).click();
  const include = page.getByRole("checkbox", { name: "Include leftover files", exact: true });
  await expect(include).not.toBeChecked();
  await include.check();
  const leftover = page.getByRole("checkbox", { name: "Move /fixtures/neon/leftover.json to Trash", exact: true });
  await expect(leftover).not.toBeChecked();
  await expect(page.getByRole("checkbox", { name: "Move /fixtures/github/hosts.yml to Trash", exact: true })).toBeDisabled();
  await expect(page.getByText("Shared by another detected account", { exact: true })).toBeVisible();
  await leftover.check();
  const remove = page.getByRole("button", { name: "Remove selected", exact: true });
  await remove.scrollIntoViewIfNeeded();
  const bounds = await remove.boundingBox();
  expect(bounds!.y + bounds!.height).toBeLessThanOrEqual(testInfo.project.use.viewport!.height);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  await page.screenshot({ path: testInfo.outputPath("cleanup-review.png") });
  await remove.click();
  await expect.poll(() => page.evaluate(() => (window as any).__CONNLENS_TEST__.calls.filter((call: any) => call.command === "execute_cleanup").length)).toBe(1);
  expect(await page.evaluate(() => (window as any).__CONNLENS_TEST__.calls.find((call: any) => call.command === "execute_cleanup").args.request))
    .toEqual({ reviewId: "review-1", connectionIds: ["old"], fileIds: ["old-file"] });
  await expect(page.getByRole("button", { name: "Review again", exact: true })).toBeVisible();
});

test("a stale cleanup review retains data and requires another review before retry", async ({ page }) => {
  await page.evaluate(() => { (window as any).__CONNLENS_TEST__.cleanupMode = "stale"; });
  await page.getByRole("button", { name: "Validate and review cleanup", exact: true }).click();
  await page.getByRole("button", { name: "Remove selected", exact: true }).click();
  await expect(page.getByText("The source changed. Review again.", { exact: true })).toBeVisible();
  expect(await page.evaluate(() => (window as any).__CONNLENS_TEST__.state().connections.some((connection: any) => connection.id === "old"))).toBe(true);
  await page.evaluate(() => { (window as any).__CONNLENS_TEST__.cleanupMode = "success"; });
  await page.getByRole("button", { name: "Review again", exact: true }).click();
  await expect(page.getByRole("checkbox", { name: "Select old@fixture.test", exact: true })).toBeChecked();
  await page.getByRole("button", { name: "Remove selected", exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__CONNLENS_TEST__.calls.filter((call: any) => call.command === "execute_cleanup").length)).toBe(2);
  expect(await page.evaluate(() => (window as any).__CONNLENS_TEST__.calls.filter((call: any) => call.command === "execute_cleanup").map((call: any) => call.args.request.reviewId)))
    .toEqual(["review-1", "review-2"]);
});

test("partial cleanup reports file failures and preserves unrelated connections", async ({ page }, testInfo) => {
  await page.evaluate(() => { (window as any).__CONNLENS_TEST__.cleanupMode = "partial"; });
  await page.getByRole("button", { name: "Validate and review cleanup", exact: true }).click();
  await page.getByRole("checkbox", { name: "Include leftover files", exact: true }).check();
  await page.getByRole("checkbox", { name: "Move /fixtures/neon/leftover.json to Trash", exact: true }).check();
  await page.getByRole("button", { name: "Remove selected", exact: true }).click();
  await expect(page.getByText("Could not move the reviewed file to Trash", { exact: true })).toBeVisible();
  await expect(page.getByText("/fixtures/neon/leftover.json", { exact: true })).toBeVisible();
  const remaining = await page.evaluate(() => (window as any).__CONNLENS_TEST__.state().connections.map((connection: any) => connection.id));
  expect(remaining).toEqual(["gh-main", "vercel-main"]);
  expect(await page.evaluate(() => (window as any).__CONNLENS_TEST__.calls.filter((call: any) => call.command === "execute_cleanup").length)).toBe(1);
  await expect(page.getByRole("button", { name: "Review again", exact: true })).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath("cleanup-result.png") });
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
