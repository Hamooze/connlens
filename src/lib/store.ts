import { create } from "zustand";
import { api, ApiError } from "./api";
import type {
  ConnLensSnapshot,
  CleanupReview,
  CleanupResult,
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
  cleanupReview: CleanupReview | null;
  cleanupResult: CleanupResult | null;
  cleanupTargetId: string | null;
  cleanupLoading: boolean;
  cleanupExecuting: boolean;
  cleanupError: string | null;
  reviewCleanup: (targetId?: string) => Promise<void>;
  executeCleanup: (connectionIds: string[], fileIds: string[]) => Promise<void>;
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
  addCustomProvider: (input: CustomProviderInput) => Promise<boolean>;
  updateSettings: (patch: Partial<SettingsState>) => Promise<void>;
  toggleProviderCollapsed: (provider: string) => void;
  purgeMissing: () => Promise<void>;
  resetAppData: () => Promise<void>;
  dismissHistoryNotice: () => Promise<void>;
}

let settingsQueue = Promise.resolve();
let loadRequest = 0;
let cleanupRequest = 0;

export const useConnLensStore = create<ConnLensStore>((set, get) => ({
  snapshot: null,
  loading: false,
  query: "",
  expandedId: null,
  view: "list",
  toast: null,
  cleanupReview: null,
  cleanupResult: null,
  cleanupTargetId: null,
  cleanupLoading: false,
  cleanupExecuting: false,
  cleanupError: null,

  reviewCleanup: async (targetId) => {
    if (get().cleanupLoading || get().cleanupExecuting) return;
    const request = ++cleanupRequest;
    set({ view: "cleanup", cleanupLoading: true, cleanupReview: null, cleanupResult: null,
      cleanupError: null, cleanupTargetId: targetId ?? null, toast: null });
    try {
      const review = await api.reviewCleanup();
      if (request !== cleanupRequest) return;
      set({ cleanupReview: review, snapshot: freshestSnapshot(get().snapshot, review.snapshot), cleanupLoading: false });
    } catch (error) {
      if (request === cleanupRequest) set({ cleanupLoading: false, cleanupError: messageFor(error) });
    }
  },

  executeCleanup: async (connectionIds, fileIds) => {
    const { cleanupReview, cleanupLoading, cleanupExecuting } = get();
    if (!cleanupReview || cleanupLoading || cleanupExecuting || !connectionIds.length) return;
    set({ cleanupExecuting: true, cleanupError: null, toast: null });
    try {
      const result = await api.executeCleanup({ reviewId: cleanupReview.reviewId, connectionIds, fileIds });
      set({ cleanupResult: { ...result,
        retained: result.retained.map((entry) => ({ ...entry, label: cleanupReview.entries.find((row) => row.id === entry.id)?.label })),
        fileFailures: result.fileFailures.map((file) => ({ ...file, path: cleanupReview.files.find((row) => row.id === file.id)?.path })),
      }, snapshot: freshestSnapshot(get().snapshot, result.snapshot), cleanupReview: null,
        cleanupExecuting: false, expandedId: null });
    } catch (error) {
      // A native review can be consumed even when execution fails. Require a
      // fresh review rather than replaying a possibly partly completed request.
      set({ cleanupReview: null, cleanupExecuting: false, cleanupError: messageFor(error) });
    }
  },

  setSnapshot: (snapshot) => set({ snapshot: freshestSnapshot(get().snapshot, snapshot), loading: false }),
  setQuery: (query) => set({ query }),
  setExpandedId: (expandedId) => set({ expandedId }),
  setView: (view) => {
    if (get().cleanupExecuting) return;
    if (view !== "cleanup") ++cleanupRequest;
    set({ view, ...(view !== "cleanup"
      ? { cleanupReview: null, cleanupResult: null, cleanupError: null, cleanupTargetId: null, cleanupLoading: false } : {}) });
  },
  setToast: (toast) => set({ toast }),

  load: async () => {
    const request = ++loadRequest;
    const before = get().snapshot;
    set({ loading: true });
    try {
      const snapshot = await api.getState();
      if (request === loadRequest && get().snapshot === before) {
        set({ snapshot, loading: false, toast: null });
      }
    } catch (error) {
      if (request === loadRequest && get().snapshot === before) {
        set({ loading: false, toast: messageFor(error) });
      }
    }
  },

  rescan: async (provider) => {
    set({ loading: true });
    try {
      const snapshot = await api.rescan(provider);
      set({ snapshot: freshestSnapshot(get().snapshot, snapshot), loading: false, toast: null });
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
      return true;
    } catch (error) {
      set({ toast: messageFor(error) });
      return false;
    }
  },

  updateSettings: async (patch) => {
    // Read the latest settings inside the queue so rapid toggles cannot overwrite each other.
    settingsQueue = settingsQueue.then(async () => {
      const snapshot = get().snapshot;
      if (!snapshot) return;
      try {
        set({ snapshot: await api.updateSettings({ ...snapshot.settings, ...patch }), toast: "Saved" });
      } catch (error) {
        set({ toast: messageFor(error) });
      }
    });
    await settingsQueue;
  },

  toggleProviderCollapsed: (provider) => {
    settingsQueue = settingsQueue.then(async () => {
      const snapshot = get().snapshot;
      if (!snapshot) return;
      const collapsedProviders = { ...snapshot.settings.collapsedProviders,
        [provider]: !snapshot.settings.collapsedProviders[provider] };
      try {
        set({ snapshot: await api.updateSettings({ ...snapshot.settings, collapsedProviders }) });
      } catch (error) {
        set({ toast: messageFor(error) });
      }
    });
  },

  purgeMissing: async () => {
    try {
      const removed = await api.purgeMissing();
      const before = get().snapshot;
      const snapshot = await api.getState();
      if (get().snapshot === before) set({ snapshot });
      set({ toast: `${removed} missing row${removed === 1 ? "" : "s"} purged` });
    } catch (error) {
      set({ toast: messageFor(error) });
    }
  },

  resetAppData: async () => {
    ++cleanupRequest;
    set({ cleanupReview: null, cleanupResult: null, cleanupError: null, cleanupTargetId: null, cleanupLoading: false });
    // A reset is ordered after pending settings writes so an earlier save cannot restore them.
    settingsQueue = settingsQueue.then(async () => {
      ++loadRequest;
      try {
        set({ snapshot: await api.resetAppData(), expandedId: null, loading: false, toast: "App data reset" });
      } catch (error) {
        set({ toast: messageFor(error) });
      }
    });
    await settingsQueue;
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

// Scan timestamps come from the serialized native registry transaction. A later
// watcher snapshot must survive a delayed response from an earlier transaction.
function freshestSnapshot(current: ConnLensSnapshot | null, incoming: ConnLensSnapshot) {
  const currentTime = Date.parse(current?.lastScan ?? "");
  const incomingTime = Date.parse(incoming.lastScan ?? "");
  return current && Number.isFinite(currentTime) && Number.isFinite(incomingTime) && currentTime > incomingTime ? current : incoming;
}
