export type ConnectionStatus = "active" | "changed" | "missing" | "unverified";
export type WatcherHealth = "ok" | "degraded" | "paused";
export type ViewName = "list" | "settings" | "cleanup";
export type Availability = "available" | "missing" | "unknown";

export interface ToolPresence {
  status: "found" | "missing" | "not_found" | "unknown";
  name: string;
  path: string | null;
  checkedAt: string;
  reason: string;
  reasonCode: string;
}

export interface ConnectionValidation {
  availability: Availability;
  usage: "selected" | "referenced" | "unknown";
  checkedAt: string | null;
  reason: string;
  reasonCode: string;
}

export interface Identity {
  label: string;
  host: string | null;
  scope: string | null;
  isActiveIdentity: boolean;
}

export interface ConnectionSource {
  sourceType: "config_file" | "credential_manager" | "env_var" | "cli" | "agent_registered";
  path: string | null;
  descriptorId: string | null;
}

export interface Connection {
  id: string;
  provider: string;
  providerName: string;
  identity: Identity;
  source: ConnectionSource;
  status: ConnectionStatus;
  fingerprint: string | null;
  firstSeen: string;
  lastSeen: string;
  hidden: boolean;
  seen: boolean;
  meta: Record<string, unknown>;
  removable: boolean;
  validation?: ConnectionValidation;
}

export interface CleanupReview {
  reviewId: string;
  checkedAt: string;
  snapshot: ConnLensSnapshot;
  entries: { id: string; label: string; providerName: string; eligible: boolean; reason: string; fileId?: string | null }[];
  files: { id: string; path: string; eligible: boolean; reason: string; sizeBytes?: number | null }[];
}

export interface CleanupRequest {
  reviewId: string;
  connectionIds: string[];
  fileIds: string[];
}

export interface CleanupResult {
  snapshot: ConnLensSnapshot;
  removedIds: string[];
  trashedPaths: string[];
  retained: { id: string; reason: string; label?: string }[];
  fileFailures: { id: string; reason: string; path?: string }[];
}

export interface SettingsState {
  watchersEnabled: boolean;
  pollMinutes: number;
  toastsEnabled: boolean;
  probesEnabled: boolean;
  providerToggles: Record<string, boolean>;
  projectRoots: string[];
  theme: "system" | "light" | "dark";
  collapsedProviders: Record<string, boolean>;
  showHidden: boolean;
  historyResetNoticeDismissed: boolean;
  autostart: boolean;
}

export interface CustomProviderInput {
  id: string;
  name: string;
  dashboardUrl: string | null;
  configPaths: string[];
  envVars: string[];
  format: "json" | "yaml" | "ini" | "toml";
}

export interface ProviderError {
  provider: string;
  code: string;
  message: string;
  detail: string | null;
}

export interface ConnLensSnapshot {
  schemaVersion: 1;
  connections: Connection[];
  settings: SettingsState;
  lastScan: string | null;
  providerErrors: ProviderError[];
  watcherHealth: WatcherHealth;
  historyResetNotice: boolean;
}

export interface ProviderGroup {
  provider: string;
  providerName: string;
  connections: Connection[];
}
