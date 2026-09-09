import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "../App";
import { api } from "../lib/api";
import { useConnLensStore } from "../lib/store";
import type { Connection, ConnLensSnapshot } from "../lib/types";

type Review = Awaited<ReturnType<typeof api.reviewCleanup>>;
type Result = Awaited<ReturnType<typeof api.executeCleanup>>;
const checkedAt = "2026-09-10T10:00:00.000Z";
const leftover = "/fixtures/neon/old-profile.json";
const shared = "/fixtures/github/hosts.yml";

function connection(id: string, status: Connection["status"]): Connection {
  return {
    id, provider: id === "available" ? "github" : "neon", providerName: id === "available" ? "GitHub" : "Neon",
    identity: { label: `${id}@fixture.test`, host: "example.test", scope: null, isActiveIdentity: status === "active" },
    source: { sourceType: "config_file", path: `/fixtures/${id}.json`, descriptorId: "fixture" },
    status, fingerprint: "sha256:12345678", firstSeen: checkedAt, lastSeen: checkedAt,
    hidden: false, seen: true, meta: {}, removable: status === "missing",
    validation: { availability: status === "missing" ? "missing" : status === "active" ? "available" : "unknown",
      usage: status === "active" ? "selected" : "unknown", checkedAt, reason: "Fixture local evidence", reasonCode: "fixture" },
  };
}

function snapshot(): ConnLensSnapshot {
  return {
    schemaVersion: 1, connections: [connection("available", "active"), connection("old-one", "missing"), connection("old-two", "missing"), connection("unknown", "unverified")],
    lastScan: checkedAt, watcherHealth: "ok", providerErrors: [], historyResetNotice: false,
    settings: { watchersEnabled: true, pollMinutes: 10, toastsEnabled: true, probesEnabled: false,
      providerToggles: {}, projectRoots: [], theme: "dark", collapsedProviders: {}, showHidden: false,
      historyResetNoticeDismissed: false, autostart: false },
  };
}

function review(): Review {
  const current = snapshot();
  return {
    reviewId: "review-1", checkedAt, snapshot: current,
    entries: current.connections.map((row) => ({
      id: row.id, label: row.identity.label, providerName: row.providerName, eligible: row.status === "missing",
      reason: row.id === "available" ? "Still available in local config" : row.id === "unknown" ? "Could not confirm the source is missing" : "Source was confirmed missing",
      ...(row.id === "old-one" ? { fileId: "leftover-file" } : {}),
    })),
    files: [
      { id: "leftover-file", path: leftover, eligible: true, reason: "Reviewed leftover file", sizeBytes: 48 },
      { id: "shared-file", path: shared, eligible: false, reason: "Shared by another detected account", sizeBytes: 120 },
    ],
  };
}

function result(patch: Partial<Result> = {}): Result {
  return { snapshot: snapshot(), removedIds: [], trashedPaths: [], retained: [], fileFailures: [], ...patch };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}

beforeEach(() => {
  useConnLensStore.setState({ snapshot: null, loading: false, toast: null, query: "", expandedId: null, view: "list",
    cleanupReview: null, cleanupResult: null, cleanupLoading: false, cleanupExecuting: false, cleanupTargetId: null, cleanupError: null });
  vi.spyOn(api, "getState").mockResolvedValue(snapshot());
  vi.spyOn(api, "getPlatform").mockResolvedValue("macos");
  vi.spyOn(api, "subscribeState").mockResolvedValue(() => undefined);
  vi.spyOn(api, "reviewCleanup").mockImplementation(async () => review());
  vi.spyOn(api, "executeCleanup").mockImplementation(async () => result());
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

async function openApp() {
  render(<App />);
  await screen.findByText("available@fixture.test", { exact: true });
}

async function openReview() {
  await openApp();
  fireEvent.click(screen.getByRole("button", { name: "Validate and review cleanup" }));
  await screen.findByRole("checkbox", { name: "Select old-one@fixture.test" });
}

function checkbox(name: string) {
  return screen.getByRole("checkbox", { name }) as HTMLInputElement;
}

function submitted() {
  const input = vi.mocked(api.executeCleanup).mock.calls[0][0];
  return { ...input, connectionIds: [...input.connectionIds].sort(), fileIds: [...input.fileIds].sort() };
}

describe("cleanup review", () => {
  it("waits for a fresh review, blocks available and unknown entries, and removes history only by default", async () => {
    const pending = deferred<Review>();
    vi.mocked(api.reviewCleanup).mockReturnValue(pending.promise);
    await openApp();
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(api.executeCleanup).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Review cleanup" }));
    expect(api.reviewCleanup).toHaveBeenCalledTimes(1);
    expect(api.executeCleanup).not.toHaveBeenCalled();
    expect(screen.queryByRole("checkbox", { name: "Select old-one@fixture.test" })).toBeNull();

    await act(async () => pending.resolve(review()));
    expect(checkbox("Select old-one@fixture.test").checked).toBe(true);
    expect(checkbox("Select old-two@fixture.test").checked).toBe(true);
    expect(checkbox("Select available@fixture.test").disabled).toBe(true);
    expect(checkbox("Select unknown@fixture.test").disabled).toBe(true);
    expect(screen.getByText("Still available in local config", { exact: true })).toBeTruthy();
    expect(screen.getByText("Could not confirm the source is missing", { exact: true })).toBeTruthy();
    expect(checkbox("Include leftover files").checked).toBe(false);

    fireEvent.click(screen.getByRole("button", { name: "Remove selected" }));
    await waitFor(() => expect(api.executeCleanup).toHaveBeenCalledTimes(1));
    expect(submitted()).toEqual({ reviewId: "review-1", connectionIds: ["old-one", "old-two"], fileIds: [] });
  });

  it("preselects only the requested missing row and disables removal when nothing is selected", async () => {
    await openApp();
    fireEvent.click(screen.getByRole("button", { name: /old-two@fixture\.test/ }));
    fireEvent.click(screen.getByRole("button", { name: "Review removal" }));
    await screen.findByRole("checkbox", { name: "Select old-two@fixture.test" });
    expect(checkbox("Select old-two@fixture.test").checked).toBe(true);
    expect(checkbox("Select old-one@fixture.test").checked).toBe(false);
    fireEvent.click(checkbox("Select old-two@fixture.test"));
    expect((screen.getByRole("button", { name: "Remove selected" }) as HTMLButtonElement).disabled).toBe(true);
    expect(api.executeCleanup).not.toHaveBeenCalled();
  });

  it.each([true, false])("sends reviewed files only with explicit selection and current opt-in=%s", async (includeFiles) => {
    await openReview();
    fireEvent.click(checkbox("Include leftover files"));
    const file = checkbox(`Move ${leftover} to Trash`);
    expect(file.checked).toBe(false);
    expect(checkbox(`Move ${shared} to Trash`).disabled).toBe(true);
    expect(screen.getByText("Shared by another detected account", { exact: true })).toBeTruthy();
    fireEvent.click(file);
    if (!includeFiles) fireEvent.click(checkbox("Include leftover files"));

    fireEvent.click(screen.getByRole("button", { name: "Remove selected" }));
    await waitFor(() => expect(api.executeCleanup).toHaveBeenCalledTimes(1));
    expect(submitted()).toEqual({ reviewId: "review-1", connectionIds: ["old-one", "old-two"], fileIds: includeFiles ? ["leftover-file"] : [] });
  });

  it("locks the review while executing and ignores a second immediate click", async () => {
    const pending = deferred<Result>();
    vi.mocked(api.executeCleanup).mockReturnValue(pending.promise);
    await openReview();
    const remove = screen.getByRole("button", { name: "Remove selected" });
    act(() => { fireEvent.click(remove); fireEvent.click(remove); });
    expect(api.executeCleanup).toHaveBeenCalledTimes(1);
    expect((remove as HTMLButtonElement).disabled).toBe(true);
    expect(checkbox("Select old-one@fixture.test").disabled).toBe(true);
    expect(checkbox("Include leftover files").disabled).toBe(true);
    await act(async () => pending.resolve(result()));
  });

  it("clears file selection if any associated entry is deselected", async () => {
    const linked = review();
    linked.entries.find((entry) => entry.id === "old-two")!.fileId = "leftover-file";
    vi.mocked(api.reviewCleanup).mockResolvedValueOnce(linked);
    await openReview();
    fireEvent.click(checkbox("Include leftover files"));
    fireEvent.click(checkbox(`Move ${leftover} to Trash`));
    expect(checkbox(`Move ${leftover} to Trash`).checked).toBe(true);
    fireEvent.click(checkbox("Select old-two@fixture.test"));
    expect(checkbox(`Move ${leftover} to Trash`).checked).toBe(false);
    expect(checkbox(`Move ${leftover} to Trash`).disabled).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "Remove selected" }));
    await waitFor(() => expect(api.executeCleanup).toHaveBeenCalledTimes(1));
    expect(submitted()).toEqual({ reviewId: "review-1", connectionIds: ["old-one"], fileIds: [] });
  });

  it("keeps existing connection data when the fresh review cannot complete", async () => {
    vi.mocked(api.reviewCleanup).mockRejectedValueOnce(new Error("Could not check the fixture source"));
    await openApp();
    fireEvent.click(screen.getByRole("button", { name: "Validate and review cleanup" }));
    await screen.findByText("Could not check the fixture source", { exact: true });
    expect(useConnLensStore.getState().snapshot!.connections.map((row) => row.id)).toContain("old-one");
    expect(useConnLensStore.getState().cleanupReview).toBeNull();
    expect(api.executeCleanup).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Review again" })).toBeTruthy();
  });

  it("keeps live watcher state and review choices until native execution returns a fresh result", async () => {
    await openReview();
    const latest = snapshot();
    latest.lastScan = "2026-09-10T10:02:00.000Z";
    latest.connections = latest.connections.map((row) => row.id === "old-two" ? {
      ...row, status: "active", removable: false,
      validation: { availability: "available", usage: "selected", checkedAt: latest.lastScan,
        reason: "Source became available again", reasonCode: "source_present" },
    } : row);
    const listener = vi.mocked(api.subscribeState).mock.calls[0][0];
    act(() => listener(latest));
    expect(useConnLensStore.getState().snapshot).toBe(latest);
    expect(useConnLensStore.getState().cleanupReview?.reviewId).toBe("review-1");
    expect(checkbox("Select old-one@fixture.test").checked).toBe(true);
    expect(checkbox("Select old-two@fixture.test").checked).toBe(true);

    const completed = { ...latest, connections: latest.connections.filter((row) => row.id !== "old-one") };
    vi.mocked(api.executeCleanup).mockResolvedValueOnce(result({ snapshot: completed, removedIds: ["old-one"],
      retained: [{ id: "old-two", reason: "Source became available again" }] }));
    fireEvent.click(screen.getByRole("button", { name: "Remove selected" }));
    await screen.findByText("Source became available again", { exact: true });
    expect(submitted()).toEqual({ reviewId: "review-1", connectionIds: ["old-one", "old-two"], fileIds: [] });
    expect(useConnLensStore.getState().snapshot).toBe(completed);
    expect(useConnLensStore.getState().snapshot?.lastScan).toBe("2026-09-10T10:02:00.000Z");
    expect(useConnLensStore.getState().snapshot?.connections.find((row) => row.id === "old-two")?.validation?.availability).toBe("available");
  });

  it("does not replace newer watcher state with an older delayed review response", async () => {
    const pending = deferred<Review>();
    vi.mocked(api.reviewCleanup).mockReturnValueOnce(pending.promise);
    await openApp();
    fireEvent.click(screen.getByRole("button", { name: "Validate and review cleanup" }));
    const latest = { ...snapshot(), lastScan: "2026-09-10T10:02:00.000Z", connections: [connection("newly-detected", "active")] };
    const listener = vi.mocked(api.subscribeState).mock.calls[0][0];
    act(() => listener(latest));
    await act(async () => pending.resolve(review()));

    expect(useConnLensStore.getState().snapshot).toEqual(latest);
    expect(useConnLensStore.getState().cleanupReview?.reviewId).toBe("review-1");
    expect(checkbox("Select old-one@fixture.test").checked).toBe(true);
    expect(api.executeCleanup).not.toHaveBeenCalled();
  });

  it("keeps newer watcher state when a delayed cleanup result arrives, while showing its operation outcome", async () => {
    const pending = deferred<Result>();
    vi.mocked(api.executeCleanup).mockReturnValueOnce(pending.promise);
    await openReview();
    fireEvent.click(screen.getByRole("button", { name: "Remove selected" }));
    const latest = { ...snapshot(), lastScan: "2026-09-10T10:03:00.000Z", connections: [connection("newly-detected", "active")] };
    const listener = vi.mocked(api.subscribeState).mock.calls[0][0];
    act(() => listener(latest));
    const older = { ...snapshot(), lastScan: "2026-09-10T10:02:00.000Z", connections: [connection("available", "active")] };
    await act(async () => pending.resolve(result({ snapshot: older, removedIds: ["old-one", "old-two"] })));

    expect(useConnLensStore.getState().snapshot).toEqual(latest);
    expect(useConnLensStore.getState().cleanupResult?.removedIds).toEqual(["old-one", "old-two"]);
    expect(screen.getByText("2 entries removed", { exact: true })).toBeTruthy();
    expect(api.executeCleanup).toHaveBeenCalledTimes(1);
  });

  it.each(["success", "error"] as const)("ignores an obsolete review %s after leaving and starting another review", async (outcome) => {
    const abandoned = deferred<Review>();
    const latest = { ...review(), reviewId: "review-2" };
    vi.mocked(api.reviewCleanup).mockReturnValueOnce(abandoned.promise).mockResolvedValueOnce(latest);
    await openApp();
    fireEvent.click(screen.getByRole("button", { name: "Validate and review cleanup" }));
    fireEvent.click(screen.getByRole("button", { name: "Back to connections" }));
    expect(useConnLensStore.getState().cleanupLoading).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "Validate and review cleanup" }));
    await screen.findByRole("checkbox", { name: "Select old-one@fixture.test" });
    await act(async () => {
      if (outcome === "success") abandoned.resolve(review());
      else abandoned.reject(new Error("Abandoned review failed"));
    });

    expect(api.reviewCleanup).toHaveBeenCalledTimes(2);
    expect(useConnLensStore.getState().cleanupReview?.reviewId).toBe("review-2");
    expect(useConnLensStore.getState().cleanupError).toBeNull();
    expect(checkbox("Select old-one@fixture.test").checked).toBe(true);
    expect(screen.queryByText("Abandoned review failed", { exact: true })).toBeNull();
  });

  it("does not restore pre-reset history from an in-flight review", async () => {
    const pending = deferred<Review>();
    vi.mocked(api.reviewCleanup).mockReturnValueOnce(pending.promise);
    const cleared = { ...snapshot(), lastScan: null, connections: [] };
    vi.spyOn(api, "resetAppData").mockResolvedValueOnce(cleared);
    await openApp();
    fireEvent.click(screen.getByRole("button", { name: "Validate and review cleanup" }));
    await act(async () => useConnLensStore.getState().resetAppData());
    await act(async () => pending.resolve(review()));

    expect(useConnLensStore.getState().snapshot).toEqual(cleared);
    expect(useConnLensStore.getState().cleanupReview).toBeNull();
    expect(useConnLensStore.getState().cleanupLoading).toBe(false);
    expect(useConnLensStore.getState().cleanupError).toBeNull();
    expect(api.executeCleanup).not.toHaveBeenCalled();
  });

  it("ignores an older watcher update after a newer scan was already shown", async () => {
    await openApp();
    const listener = vi.mocked(api.subscribeState).mock.calls[0][0];
    const latest = { ...snapshot(), lastScan: "2026-09-10T10:04:00.000Z", connections: [connection("newly-detected", "active")] };
    act(() => { listener(latest); listener(snapshot()); });

    expect(useConnLensStore.getState().snapshot).toEqual(latest);
    expect(screen.getByText("newly-detected@fixture.test", { exact: true })).toBeTruthy();
    expect(screen.queryByText("old-one@fixture.test", { exact: true })).toBeNull();
  });

  it("keeps a newer watcher update when an earlier manual rescan responds late", async () => {
    const pending = deferred<ConnLensSnapshot>();
    vi.spyOn(api, "rescan").mockReturnValueOnce(pending.promise);
    await openApp();
    fireEvent.click(screen.getByRole("button", { name: "Rescan" }));
    const latest = { ...snapshot(), lastScan: "2026-09-10T10:06:00.000Z", connections: [connection("newly-detected", "active")] };
    act(() => vi.mocked(api.subscribeState).mock.calls[0][0](latest));
    await act(async () => pending.resolve({ ...snapshot(), lastScan: "2026-09-10T10:05:00.000Z" }));

    expect(useConnLensStore.getState().snapshot).toEqual(latest);
    expect(useConnLensStore.getState().loading).toBe(false);
    expect(screen.getByText("newly-detected@fixture.test", { exact: true })).toBeTruthy();
  });

  it("preserves connection data after a stale-review rejection and requires a fresh token before retry", async () => {
    vi.mocked(api.executeCleanup).mockRejectedValueOnce({ code: "stale_review", message: "The source changed. Review again." });
    await openReview();
    fireEvent.click(checkbox("Select old-two@fixture.test"));
    fireEvent.click(screen.getByRole("button", { name: "Remove selected" }));
    await screen.findByText("The source changed. Review again.", { exact: true });
    expect(useConnLensStore.getState().cleanupReview).toBeNull();
    expect(useConnLensStore.getState().snapshot!.connections.map((row) => row.id)).toContain("old-one");
    await act(async () => useConnLensStore.getState().executeCleanup(["old-one"], []));
    expect(api.executeCleanup).toHaveBeenCalledTimes(1);
    vi.mocked(api.reviewCleanup).mockResolvedValueOnce({ ...review(), reviewId: "review-2" });
    fireEvent.click(screen.getByRole("button", { name: "Review again" }));
    await waitFor(() => expect(useConnLensStore.getState().cleanupReview?.reviewId).toBe("review-2"));
    expect(api.reviewCleanup).toHaveBeenCalledTimes(2);
    expect(api.executeCleanup).toHaveBeenCalledTimes(1);
  });

  it("shows retained entries and file failures after partial completion without repeating successful work", async () => {
    const latest = snapshot();
    latest.connections = latest.connections.filter((row) => row.id !== "old-one");
    vi.mocked(api.executeCleanup).mockResolvedValueOnce(result({ snapshot: latest, removedIds: ["old-one"],
      retained: [{ id: "old-two", reason: "Source became available again" }],
      fileFailures: [{ id: "leftover-file", reason: "Could not move the reviewed file to Trash" }] }));
    await openReview();
    fireEvent.click(checkbox("Include leftover files"));
    fireEvent.click(checkbox(`Move ${leftover} to Trash`));
    fireEvent.click(screen.getByRole("button", { name: "Remove selected" }));
    await screen.findByText("Source became available again", { exact: true });
    expect(screen.getByText("old-two@fixture.test", { exact: true })).toBeTruthy();
    expect(screen.getByText(leftover, { exact: true })).toBeTruthy();
    expect(screen.getByText("Could not move the reviewed file to Trash", { exact: true })).toBeTruthy();
    expect(useConnLensStore.getState().snapshot!.connections.some((row) => row.id === "old-one")).toBe(false);
    expect(useConnLensStore.getState().snapshot!.connections.some((row) => row.id === "old-two")).toBe(true);
    expect(api.executeCleanup).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "Review again" })).toBeTruthy();
  });
});
