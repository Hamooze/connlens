import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { ConnLensSnapshot, CustomProviderInput, SettingsState } from "./types";

type Listener = (snapshot: ConnLensSnapshot) => void;

export class ApiError extends Error {
  code: string;

  constructor(code: string, message: string) {
    super(message);
    this.code = code;
  }
}

const hasTauri = () =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

let devSnapshot: ConnLensSnapshot = makeDevSnapshot();
const devListeners = new Set<Listener>();

export const api = {
  async getState() {
    if (hasTauri()) return invoke<ConnLensSnapshot>("get_state");
    return devOnly();
  },

  async rescan(provider?: string) {
    if (hasTauri()) return invoke<ConnLensSnapshot>("rescan", { provider });
    devSnapshot = {
      ...devSnapshot,
      lastScan: new Date().toISOString(),
      providerErrors: provider ? [] : devSnapshot.providerErrors,
    };
    emitDev();
    return devSnapshot;
  },

  async updateSettings(settings: SettingsState) {
    if (hasTauri()) return invoke<ConnLensSnapshot>("update_settings", { settings });
    devSnapshot = { ...devSnapshot, settings };
    emitDev();
    return devSnapshot;
  },

  async copyValue(id: string, field: string) {
    if (hasTauri()) return invoke<void>("copy_value", { id, field });
    const connection = devSnapshot.connections.find((row) => row.id === id);
    const text =
      field === "fingerprint"
        ? connection?.fingerprint
        : field === "source_path"
          ? connection?.source.path
          : `${connection?.identity.label}@${connection?.identity.host ?? "local"}`;
    if (navigator.clipboard && text) {
      await navigator.clipboard.writeText(text);
    }
  },

  async remove(id: string) {
    if (hasTauri()) return invoke<ConnLensSnapshot>("remove", { id });
    const connection = devSnapshot.connections.find((row) => row.id === id);
    if (!connection) throw new ApiError("not_found", "Connection was not found");
    if (!connection.removable) {
      throw new ApiError(
        "not_removable",
        "Active auto-detected connections cannot be deleted",
      );
    }
    devSnapshot = {
      ...devSnapshot,
      connections: devSnapshot.connections.filter((row) => row.id !== id),
    };
    emitDev();
    return devSnapshot;
  },

  async openDashboard(id: string) {
    if (hasTauri()) return invoke<string>("open_dashboard", { id });
    const connection = devSnapshot.connections.find((row) => row.id === id);
    if (!connection) throw new ApiError("not_found", "Connection was not found");
    const url = connection.provider === "github"
      ? `https://github.com/${connection.identity.label}`
      : "https://example.com";
    window.open(url, "_blank", "noopener,noreferrer");
    return url;
  },

  async revealSource(id: string) {
    if (hasTauri()) return invoke<string>("reveal_source", { id });
    const connection = devSnapshot.connections.find((row) => row.id === id);
    if (!connection?.source.path) throw new ApiError("path_missing", "This source cannot be revealed");
    return connection.source.path;
  },

  async openExternalUrl(url: string) {
    if (hasTauri()) return invoke<string>("open_external_url", { url });
    window.open(url, "_blank", "noopener,noreferrer");
    return url;
  },

  async addCustomProvider(input: CustomProviderInput) {
    if (hasTauri()) return invoke<ConnLensSnapshot>("add_custom_provider", { input });
    const id = input.id || slugify(input.name);
    const now = new Date().toISOString();
    const host = input.dashboardUrl ? new URL(input.dashboardUrl).host : "custom";
    devSnapshot = {
      ...devSnapshot,
      connections: [
        ...devSnapshot.connections,
        {
          ...row(
            `custom-${id}`,
            id,
            input.name,
            input.name,
            host,
            input.envVars[0] ?? "custom source",
            "unverified",
            false,
            now,
          ),
          source: {
            sourceType: "agent_registered" as const,
            path: input.configPaths[0] ?? input.envVars[0] ?? null,
            descriptorId: id,
          },
          removable: true,
        },
      ],
    };
    emitDev();
    return devSnapshot;
  },

  async purgeMissing() {
    if (hasTauri()) return invoke<number>("purge_missing");
    const before = devSnapshot.connections.length;
    devSnapshot = {
      ...devSnapshot,
      connections: devSnapshot.connections.filter((row) => row.status !== "missing"),
    };
    emitDev();
    return before - devSnapshot.connections.length;
  },

  async resetAppData() {
    if (hasTauri()) return invoke<ConnLensSnapshot>("reset_app_data");
    devSnapshot = { ...makeDevSnapshot(), connections: [] };
    emitDev();
    return devSnapshot;
  },

  async dismissHistoryNotice() {
    if (hasTauri()) return invoke<ConnLensSnapshot>("dismiss_history_reset_notice");
    devSnapshot = { ...devSnapshot, historyResetNotice: false };
    emitDev();
    return devSnapshot;
  },

  async subscribeState(listener: Listener) {
    if (hasTauri()) {
      const dispose = await listen<ConnLensSnapshot>("state://updated", (event) => {
        listener(event.payload);
      });
      return dispose;
    }
    devListeners.add(listener);
    return () => devListeners.delete(listener);
  },

  async startWindowDrag() {
    if (hasTauri()) return getCurrentWindow().startDragging();
  },

  async closeWindow() {
    if (hasTauri()) return getCurrentWindow().hide();
  },
};

function devOnly() {
  if (import.meta.env.DEV) return Promise.resolve(devSnapshot);
  return Promise.reject(
    new ApiError("tauri_unavailable", "ConnLens must run inside Tauri"),
  );
}

function emitDev() {
  for (const listener of devListeners) listener(devSnapshot);
}

function slugify(value: string) {
  return value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 64);
}

function makeDevSnapshot(): ConnLensSnapshot {
  const now = new Date("2026-08-06T09:24:31.000Z").toISOString();
  return {
    schemaVersion: 1,
    lastScan: now,
    watcherHealth: "ok",
    historyResetNotice: false,
    providerErrors: [],
    settings: {
      watchersEnabled: true,
      pollMinutes: 10,
      toastsEnabled: true,
      probesEnabled: true,
      providerToggles: {},
      projectRoots: [],
      theme: "dark",
      collapsedProviders: {},
      showHidden: false,
      historyResetNoticeDismissed: false,
      autostart: false,
    },
    connections: [
      row("gh-dev", "github", "GitHub", "dev", "github.com", "github.com", "active", true, now),
      row("gh-work", "github", "GitHub", "work", "github.com", "github.com", "active", false, now),
      row("aws", "aws", "AWS", "BRDG Production", "123456789012", "prod", "active", false, now),
      row("vercel", "vercel", "Vercel", "vercel.user@example.test", "vercel.com", "team_acme", "active", false, now),
      row("neon", "neon", "Neon DB", "neon.user@example.test", "console.neon.tech", "acct_fixture_1", "active", false, now),
      row("docker", "docker", "Docker", "registry.npmjs.org mirror", "hub.docker.com", "desktop credential store", "active", false, now),
      row("npm", "npm", "npm", "registry.npmjs.org", "registry.npmjs.org", "user npmrc", "active", false, now),
      row("netlify", "netlify", "Netlify", "Netlify (...42fa)", "app.netlify.com", "global config", "changed", false, now),
    ],
  };
}

function row(
  id: string,
  provider: string,
  providerName: string,
  label: string,
  host: string,
  scope: string,
  status: ConnLensSnapshot["connections"][number]["status"],
  active: boolean,
  now: string,
) {
  return {
    id,
    provider,
    providerName,
    identity: {
      label,
      host,
      scope,
      isActiveIdentity: active,
    },
    source: {
      sourceType: "config_file" as const,
      path: `C:\\Users\\dev\\.connlens\\fixtures\\${id}.json`,
      descriptorId: provider,
    },
    status,
    fingerprint: status === "missing" ? null : "sha256:8f3a2c6d",
    firstSeen: now,
    lastSeen: now,
    hidden: false,
    seen: status !== "changed",
    meta: {},
    removable: status === "missing",
  };
}
