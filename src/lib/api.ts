import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { CleanupRequest, CleanupResult, CleanupReview, ConnLensSnapshot, CustomProviderInput, SettingsState } from "./types";

type Listener = (snapshot: ConnLensSnapshot) => void;

export class ApiError extends Error {
  code: string;

  constructor(code: string, message: string) {
    super(message);
    this.code = code;
  }
}

export const isNativeApp = () =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export const previewPlatform = (): string => {
  if (typeof navigator === "undefined") return "unknown";
  return /Mac/.test(navigator.platform) ? "macos" : /Win/.test(navigator.platform) ? "windows" : "linux";
};

export const api = {
  async reviewCleanup(): Promise<CleanupReview> {
    if (isNativeApp()) return invoke<CleanupReview>("review_cleanup");
    return (await preview()).reviewCleanup();
  },

  async executeCleanup(request: CleanupRequest): Promise<CleanupResult> {
    if (isNativeApp()) return invoke<CleanupResult>("execute_cleanup", { request });
    return (await preview()).executeCleanup(request);
  },
  async getPlatform(): Promise<string> {
    if (isNativeApp()) return invoke<string>("get_platform").catch(() => previewPlatform());
    return previewPlatform();
  },

  async quitApp() {
    if (isNativeApp()) return invoke<void>("quit_app");
    throw new ApiError("preview_only", "Quit is available in the installed ConnLens app.");
  },
  async getState() {
    if (isNativeApp()) return invoke<ConnLensSnapshot>("get_state");
    return (await preview()).getState();
  },

  async rescan(provider?: string) {
    if (isNativeApp()) return invoke<ConnLensSnapshot>("rescan", { provider });
    return (await preview()).rescan(provider);
  },

  async updateSettings(settings: SettingsState) {
    if (isNativeApp()) return invoke<ConnLensSnapshot>("update_settings", { settings });
    return (await preview()).updateSettings(settings);
  },

  async copyValue(id: string, field: string) {
    if (isNativeApp()) return invoke<void>("copy_value", { id, field });
    return (await preview()).copyValue(id, field);
  },

  async remove(id: string) {
    if (isNativeApp()) return invoke<ConnLensSnapshot>("remove", { id });
    return (await preview()).remove(id);
  },

  async openDashboard(id: string) {
    if (isNativeApp()) return invoke<string>("open_dashboard", { id });
    return (await preview()).openDashboard(id);
  },

  async revealSource(id: string) {
    if (isNativeApp()) return invoke<string>("reveal_source", { id });
    return (await preview()).revealSource(id);
  },

  async openExternalUrl(url: string) {
    if (isNativeApp()) return invoke<string>("open_external_url", { url });
    return (await preview()).openExternalUrl(url);
  },

  async addCustomProvider(input: CustomProviderInput) {
    if (isNativeApp()) return invoke<ConnLensSnapshot>("add_custom_provider", { input });
    return (await preview()).addCustomProvider(input);
  },

  async purgeMissing() {
    if (isNativeApp()) return invoke<number>("purge_missing");
    return (await preview()).purgeMissing();
  },

  async resetAppData() {
    if (isNativeApp()) return invoke<ConnLensSnapshot>("reset_app_data");
    return (await preview()).resetAppData();
  },

  async dismissHistoryNotice() {
    if (isNativeApp()) return invoke<ConnLensSnapshot>("dismiss_history_reset_notice");
    return (await preview()).dismissHistoryNotice();
  },

  async subscribeState(listener: Listener) {
    if (isNativeApp()) {
      return listen<ConnLensSnapshot>("state://updated", (event) => listener(event.payload));
    }
    return (await preview()).subscribeState(listener);
  },

  async startWindowDrag() {
    if (isNativeApp()) return getCurrentWindow().startDragging();
  },

  async closeWindow() {
    if (isNativeApp()) return getCurrentWindow().hide();
  },
};

async function preview() {
  if (import.meta.env.DEV) return (await import("./api.preview")).previewApi;
  throw new ApiError("tauri_unavailable", "ConnLens must run inside Tauri");
}
