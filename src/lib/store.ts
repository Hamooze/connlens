import { create } from "zustand";
import { api, ApiError } from "./api";
import type {
  ConnLensSnapshot,
  CustomProviderInput,
  SettingsState,
  ViewName,
} from "./types";

interface ConnLensStore {
  snapshot: ConnLensSnapshot | null;
  loading: boolean;
  query: string;
  expandedId: string | null;
  view: ViewName;
  toast: string | null;
  setSnapshot: (snapshot: ConnLensSnapshot) => void;
  setQuery: (query: string) => void;
  setExpandedId: (id: string | null) => void;
  setView: (view: ViewName) => void;
  setToast: (toast: string | null) => void;
  load: () => Promise<void>;
  rescan: (provider?: string) => Promise<void>;
  copyValue: (id: string, field: string) => Promise<void>;
  openDashboard: (id: string) => Promise<void>;
  revealSource: (id: string) => Promise<void>;
  removeConnection: (id: string) => Promise<void>;
  addCustomProvider: (input: CustomProviderInput) => Promise<void>;
  updateSettings: (patch: Partial<SettingsState>) => Promise<void>;
  toggleProviderCollapsed: (provider: string) => void;
  purgeMissing: () => Promise<void>;
  resetAppData: () => Promise<void>;
  dismissHistoryNotice: () => Promise<void>;
}

export const useConnLensStore = create<ConnLensStore>((set, get) => ({
  snapshot: null,
  loading: false,
  query: "",
  expandedId: null,
  view: "list",
  toast: null,

  setSnapshot: (snapshot) => set({ snapshot, loading: false }),
  setQuery: (query) => set({ query }),
  setExpandedId: (expandedId) => set({ expandedId }),
  setView: (view) => set({ view }),
  setToast: (toast) => set({ toast }),

  load: async () => {
    set({ loading: true });
    try {
      set({ snapshot: await api.getState(), loading: false });
    } catch (error) {
      set({ loading: false, toast: messageFor(error) });
    }
  },

  rescan: async (provider) => {
    set({ loading: true });
    try {
      set({ snapshot: await api.rescan(provider), loading: false, toast: null });
    } catch (error) {
      set({ loading: false, toast: messageFor(error) });
    }
  },

  copyValue: async (id, field) => {
    try {
      await api.copyValue(id, field);
      set({ toast: "Copied" });
    } catch (error) {
      set({ toast: messageFor(error) });
    }
  },

  openDashboard: async (id) => {
    try {
      const url = await api.openDashboard(id);
      set({ toast: `Opened dashboard: ${url}` });
    } catch (error) {
      set({ toast: messageFor(error) });
    }
  },

  revealSource: async (id) => {
    try {
      const path = await api.revealSource(id);
      set({ toast: `Opened source: ${path}` });
    } catch (error) {
      set({ toast: messageFor(error) });
    }
  },

  removeConnection: async (id) => {
    try {
      set({ snapshot: await api.remove(id), expandedId: null, toast: "Deleted" });
    } catch (error) {
      set({ toast: messageFor(error) });
    }
  },

  addCustomProvider: async (input) => {
    try {
      set({ snapshot: await api.addCustomProvider(input), toast: "Custom provider added" });
    } catch (error) {
      set({ toast: messageFor(error) });
    }
  },

  updateSettings: async (patch) => {
    const snapshot = get().snapshot;
    if (!snapshot) return;
    const settings = { ...snapshot.settings, ...patch };
    try {
      set({ snapshot: await api.updateSettings(settings), toast: "Saved" });
    } catch (error) {
      set({ toast: messageFor(error) });
    }
  },

  toggleProviderCollapsed: (provider) => {
    const snapshot = get().snapshot;
    if (!snapshot) return;
    const collapsedProviders = {
      ...snapshot.settings.collapsedProviders,
      [provider]: !snapshot.settings.collapsedProviders[provider],
    };
    const settings = { ...snapshot.settings, collapsedProviders };
    set({ snapshot: { ...snapshot, settings } });
    void api.updateSettings(settings).catch(() => undefined);
  },

  purgeMissing: async () => {
    try {
      const removed = await api.purgeMissing();
      await get().load();
      set({ toast: `${removed} missing row${removed === 1 ? "" : "s"} purged` });
    } catch (error) {
      set({ toast: messageFor(error) });
    }
  },

  resetAppData: async () => {
    try {
      set({ snapshot: await api.resetAppData(), expandedId: null, toast: "App data reset" });
    } catch (error) {
      set({ toast: messageFor(error) });
    }
  },

  dismissHistoryNotice: async () => {
    try {
      set({ snapshot: await api.dismissHistoryNotice() });
    } catch (error) {
      set({ toast: messageFor(error) });
    }
  },
}));

function messageFor(error: unknown) {
  if (error instanceof ApiError) return error.message;
  if (error && typeof error === "object" && "message" in error) {
    return String((error as { message: unknown }).message);
  }
  return "Something went wrong";
}
