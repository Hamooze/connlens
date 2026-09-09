import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { Profiler, useMemo, type ProfilerOnRenderCallback } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useConnLensStore } from "../lib/store";
import type { Connection, ConnLensSnapshot, ProviderGroup } from "../lib/types";
import { ConnectionList } from "./ConnectionList";

function connection(id: string): Connection {
  return {
    id, provider: "github", providerName: "GitHub",
    identity: { label: `${id}@fixture.test`, host: "github.example.test", scope: null, isActiveIdentity: true },
    source: { sourceType: "config_file", path: `/fixtures/${id}.yml`, descriptorId: "github" },
    status: "active", fingerprint: "sha256:12345678",
    firstSeen: "2026-09-10T10:00:00.000Z", lastSeen: "2026-09-10T10:00:00.000Z",
    hidden: false, seen: true, meta: {}, removable: false,
  };
}

function snapshot(): ConnLensSnapshot {
  return {
    schemaVersion: 1, connections: [connection("alice"), connection("bob"), connection("charlie")],
    lastScan: "2026-09-10T10:00:00.000Z", providerErrors: [], watcherHealth: "ok", historyResetNotice: false,
    settings: {
      watchersEnabled: true, pollMinutes: 10, toastsEnabled: true, probesEnabled: false,
      providerToggles: {}, projectRoots: [], theme: "dark", collapsedProviders: {},
      showHidden: false, historyResetNoticeDismissed: false, autostart: false,
    },
  };
}

// Separate real lists give each row a Profiler boundary without replacing any product
// component. The shared provider also exposes provider-wide expansion subscriptions.
function ProfiledRows({ onRender }: { onRender: ProfilerOnRenderCallback }) {
  const connections = useConnLensStore((state) => state.snapshot!.connections);
  const lists = useMemo(() => connections.map((row): ProviderGroup[] => [{
    provider: row.provider, providerName: row.providerName, connections: [row],
  }]), [connections]);
  return lists.map((groups) => <Profiler key={groups[0].connections[0].id} id={groups[0].connections[0].id} onRender={onRender}>
    <ConnectionList groups={groups} />
  </Profiler>);
}

beforeEach(() => {
  useConnLensStore.setState({ snapshot: snapshot(), loading: false, toast: null, query: "", expandedId: null, view: "list" });
});
afterEach(cleanup);

function renderProfiledRows() {
  const commits = vi.fn<ProfilerOnRenderCallback>();
  render(<ProfiledRows onRender={commits} />);
  expect(commits).toHaveBeenCalledTimes(3);
  commits.mockClear();
  return commits;
}

describe("connection rendering", () => {
  it.each([
    ["toast", () => useConnLensStore.getState().setToast("Copied")],
    ["query", () => useConnLensStore.getState().setQuery("fixture")],
    ["loading", () => useConnLensStore.setState({ loading: true })],
  ] as const)("does not commit row renders when only %s changes", (_field, update) => {
    const commits = renderProfiledRows();
    act(update);
    expect(commits).not.toHaveBeenCalled();
    expect(screen.getAllByRole("button", { name: /@fixture\.test/ })).toHaveLength(3);
  });

  it("updates only the rows whose expanded state changes", () => {
    const commits = renderProfiledRows();
    const alice = screen.getByRole("button", { name: /alice@fixture\.test/ });
    const bob = screen.getByRole("button", { name: /bob@fixture\.test/ });

    fireEvent.click(alice);
    expect(commits.mock.calls.map(([id]) => id)).toEqual(["alice"]);
    expect(alice.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByText("/fixtures/alice.yml")).toBeTruthy();
    commits.mockClear();

    fireEvent.click(bob);
    expect(commits.mock.calls.map(([id]) => id).sort()).toEqual(["alice", "bob"]);
    expect(alice.getAttribute("aria-expanded")).toBe("false");
    expect(bob.getAttribute("aria-expanded")).toBe("true");
    expect(screen.queryByText("/fixtures/alice.yml")).toBeNull();
    expect(screen.getByText("/fixtures/bob.yml")).toBeTruthy();
    commits.mockClear();

    fireEvent.click(bob);
    expect(commits.mock.calls.map(([id]) => id)).toEqual(["bob"]);
    expect(bob.getAttribute("aria-expanded")).toBe("false");
  });

  it("renders changed connection props from a new snapshot while preserving expansion", () => {
    const commits = renderProfiledRows();
    fireEvent.click(screen.getByRole("button", { name: /alice@fixture\.test/ }));
    commits.mockClear();
    const current = useConnLensStore.getState().snapshot!;
    const changed: Connection = {
      ...current.connections[0], status: "changed", fingerprint: "sha256:87654321",
      identity: { ...current.connections[0].identity, label: "updated@fixture.test" },
    };

    act(() => useConnLensStore.getState().setSnapshot({ ...current, connections: [changed, ...current.connections.slice(1)] }));

    expect(commits.mock.calls.some(([id]) => id === "alice")).toBe(true);
    expect(screen.queryByText("alice@fixture.test")).toBeNull();
    expect(screen.getByRole("button", { name: /updated@fixture\.test\s*Changed/ }).getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByText("sha256:87654321")).toBeTruthy();
  });
});
