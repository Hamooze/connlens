import { describe, expect, it } from "vitest";
import { filterConnections, groupConnections } from "./connectionView";
import type { Connection } from "./types";

const base: Omit<Connection, "id" | "provider" | "providerName" | "identity" | "status"> = {
  source: { sourceType: "config_file", path: "C:/fixture.json", descriptorId: "x" },
  fingerprint: "sha256:12345678",
  firstSeen: "2026-08-06T10:00:00.000Z",
  lastSeen: "2026-08-06T10:00:00.000Z",
  hidden: false,
  seen: true,
  meta: {},
  removable: false,
};

function row(
  id: string,
  provider: string,
  providerName: string,
  label: string,
  status: Connection["status"],
  active = false,
): Connection {
  return {
    ...base,
    id,
    provider,
    providerName,
    status,
    identity: { label, host: `${provider}.example`, scope: null, isActiveIdentity: active },
  };
}

describe("connection view helpers", () => {
  it("groups and sorts active identities before missing rows", () => {
    const groups = groupConnections([
      row("2", "github", "GitHub", "bob", "missing"),
      row("1", "github", "GitHub", "alice", "active", true),
      row("3", "neon", "Neon", "prod", "active"),
    ]);
    expect(groups.map((group) => group.provider)).toEqual(["github", "neon"]);
    expect(groups[0].connections.map((connection) => connection.identity.label)).toEqual([
      "alice",
      "bob",
    ]);
  });

  it("filters across provider, label, and host", () => {
    const hidden = { ...row("1", "github", "GitHub", "alice", "active"), hidden: true };
    const visible = row("2", "neon", "Neon", "prod", "active");
    expect(filterConnections([hidden, visible], "neon")).toHaveLength(1);
    expect(filterConnections([hidden, visible], "alice")).toHaveLength(1);
  });
});
