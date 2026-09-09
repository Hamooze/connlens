import type { Connection } from "./types";

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
