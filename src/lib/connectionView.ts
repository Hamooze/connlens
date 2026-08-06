import type { Connection, ConnectionStatus, ProviderGroup } from "./types";

const statusOrder: Record<ConnectionStatus, number> = {
  active: 0,
  changed: 1,
  unverified: 2,
  missing: 3,
};

export function filterConnections(
  connections: Connection[],
  query: string,
) {
  const normalized = query.trim().toLowerCase();
  return connections
    .filter((connection) => {
      if (!normalized) return true;
      return [
        connection.provider,
        connection.providerName,
        connection.identity.label,
        connection.identity.host,
        connection.identity.scope,
        connection.source.path,
      ]
        .filter(Boolean)
        .join(" ")
        .toLowerCase()
        .includes(normalized);
    });
}

export function groupConnections(connections: Connection[]): ProviderGroup[] {
  const groups = new Map<string, ProviderGroup>();
  for (const connection of connections) {
    const key = connection.provider;
    if (!groups.has(key)) {
      groups.set(key, {
        provider: connection.provider,
        providerName: connection.providerName,
        connections: [],
      });
    }
    groups.get(key)?.connections.push(connection);
  }

  return Array.from(groups.values())
    .map((group) => ({
      ...group,
      connections: [...group.connections].sort(sortConnections),
    }))
    .sort((a, b) => a.providerName.localeCompare(b.providerName));
}

function sortConnections(a: Connection, b: Connection) {
  return (
    statusOrder[a.status] - statusOrder[b.status] ||
    Number(b.identity.isActiveIdentity) - Number(a.identity.isActiveIdentity) ||
    a.identity.label.localeCompare(b.identity.label)
  );
}

export function statusLabel(status: ConnectionStatus) {
  switch (status) {
    case "active":
      return "active";
    case "changed":
      return "changed";
    case "missing":
      return "missing";
    case "unverified":
      return "check";
  }
}
