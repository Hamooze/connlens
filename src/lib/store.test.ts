import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "./api";
import { useConnLensStore } from "./store";
import type { ConnLensSnapshot, SettingsState } from "./types";

function snapshot(patch: Partial<SettingsState> = {}): ConnLensSnapshot {
  return {
    schemaVersion: 1, connections: [], lastScan: "2026-09-10T10:00:00.000Z",
    providerErrors: [], watcherHealth: "ok", historyResetNotice: false,
    settings: {
      watchersEnabled: true, pollMinutes: 10, toastsEnabled: true, probesEnabled: false,
      providerToggles: {}, projectRoots: [], theme: "dark", collapsedProviders: {},
      showHidden: false, historyResetNoticeDismissed: false, autostart: false, ...patch,
    },
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

beforeEach(() => {
  useConnLensStore.setState({ snapshot: snapshot(), loading: false, toast: null, query: "", expandedId: null, view: "list" });
});
afterEach(() => vi.restoreAllMocks());

describe("settings mutation ordering", () => {
  it("preserves independent rapid toggles while the first save is pending", async () => {
    const first = deferred<ConnLensSnapshot>();
    const update = vi.spyOn(api, "updateSettings")
      .mockImplementationOnce(() => first.promise)
      .mockImplementation(async (settings) => snapshot(settings));
    const watching = useConnLensStore.getState().updateSettings({ watchersEnabled: false });
    const notifications = useConnLensStore.getState().updateSettings({ toastsEnabled: false });
    await Promise.resolve();
    expect(update).toHaveBeenCalledTimes(1);
    first.resolve(snapshot({ watchersEnabled: false }));
    await Promise.all([watching, notifications]);
    expect(update.mock.calls[1][0]).toMatchObject({ watchersEnabled: false, toastsEnabled: false });
    expect(useConnLensStore.getState().snapshot?.settings).toMatchObject({ watchersEnabled: false, toastsEnabled: false });
  });

  it("keeps the queue usable after a failed save", async () => {
    vi.spyOn(api, "updateSettings").mockRejectedValueOnce(new Error("Startup is unavailable"))
      .mockImplementation(async (settings) => snapshot(settings));
    await useConnLensStore.getState().updateSettings({ autostart: true });
    expect(useConnLensStore.getState().toast).toBe("Startup is unavailable");
    await useConnLensStore.getState().updateSettings({ toastsEnabled: false });
    expect(useConnLensStore.getState().snapshot?.settings).toMatchObject({ autostart: false, toastsEnabled: false });
  });

  it("does not erase another provider's collapsed state", async () => {
    const calls: SettingsState[] = [];
    vi.spyOn(api, "updateSettings").mockImplementation(async (settings) => { calls.push(settings); return snapshot(settings); });
    useConnLensStore.getState().toggleProviderCollapsed("github");
    useConnLensStore.getState().toggleProviderCollapsed("aws");
    // A queued setting save is a completion barrier for both collapse actions.
    await useConnLensStore.getState().updateSettings({ pollMinutes: 30 });
    expect(calls[1].collapsedProviders).toEqual({ github: true, aws: true });
  });

  it("finishes pending settings writes before resetting app data", async () => {
    const first = deferred<ConnLensSnapshot>();
    vi.spyOn(api, "updateSettings").mockImplementationOnce(() => first.promise);
    const reset = vi.spyOn(api, "resetAppData").mockResolvedValue(snapshot());
    const save = useConnLensStore.getState().updateSettings({ watchersEnabled: false });
    await Promise.resolve();
    const resetting = useConnLensStore.getState().resetAppData();
    const resetCallsBeforeSaveFinishes = reset.mock.calls.length;
    first.resolve(snapshot({ watchersEnabled: false }));
    await Promise.all([save, resetting]);
    expect(resetCallsBeforeSaveFinishes).toBe(0);
    expect(useConnLensStore.getState().snapshot?.settings.watchersEnabled).toBe(true);
  });
});

describe("snapshot freshness", () => {
  it("keeps a newer watcher snapshot if an older initial read resolves afterwards", async () => {
    const read = deferred<ConnLensSnapshot>();
    vi.spyOn(api, "getState").mockImplementationOnce(() => read.promise);
    useConnLensStore.setState({ snapshot: null });
    const loading = useConnLensStore.getState().load();
    const latest = { ...snapshot(), lastScan: "2026-09-10T10:01:00.000Z" };
    useConnLensStore.getState().setSnapshot(latest);
    read.resolve(snapshot());
    await loading;
    expect(useConnLensStore.getState().snapshot?.lastScan).toBe(latest.lastScan);
  });
});
