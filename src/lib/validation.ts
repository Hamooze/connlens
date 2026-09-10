import type { Connection, ToolPresence } from "./types";

export const availabilityOf = (connection: Connection) => connection.validation?.availability ?? "unknown";
export const availabilityLabel = (connection: Connection) => ({
  available: "Available locally", missing: "Missing locally", unknown: "Could not check",
})[availabilityOf(connection)];
export const usageLabel = (connection: Connection) => ({
  selected: "Selected in local config", referenced: "Referenced locally", unknown: "Usage not confirmed",
})[connection.validation?.usage ?? "unknown"];

export function validationCounts(connections: Connection[]) {
  const counts = { available: 0, missing: 0, unknown: 0 };
  for (const connection of connections) counts[availabilityOf(connection)]++;
  return counts;
}

export function toolPresenceOf(connection: Connection): ToolPresence | undefined {
  const value = connection.meta.toolPresence;
  if (!value || typeof value !== "object") return;
  const tool = value as Partial<ToolPresence>;
  if (!["found", "missing", "not_found", "unknown"].includes(tool.status ?? "")
      || typeof tool.name !== "string" || typeof tool.reason !== "string"
      || typeof tool.checkedAt !== "string" || typeof tool.reasonCode !== "string"
      || (tool.path !== null && typeof tool.path !== "string")) return;
  return tool as ToolPresence;
}

export function toolPresenceLabel(connection: Connection, tool: ToolPresence) {
  const kind = connection.provider === "mcp_servers" ? "Launcher" : "CLI";
  return `${kind} ${{ found: "found", missing: "removed", not_found: "not found", unknown: "unchecked" }[tool.status]}`;
}
