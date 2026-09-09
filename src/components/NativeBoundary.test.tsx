import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import App from "../App";
import { api } from "../lib/api";
import { useConnLensStore } from "../lib/store";

beforeEach(() => {
  useConnLensStore.setState({ snapshot: null, loading: false, toast: null, view: "list", query: "", expandedId: null });
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllEnvs(); });

it("shows a native load failure rather than an empty successful scan", async () => {
  vi.spyOn(api, "getPlatform").mockResolvedValue("macos");
  vi.spyOn(api, "subscribeState").mockResolvedValue(() => undefined);
  vi.spyOn(api, "getState").mockRejectedValue(new Error("Registry could not be read"));
  render(<App />);
  expect(await screen.findByText("Unable to load connections")).toBeTruthy();
  expect(screen.queryByText("No connections detected")).toBeNull();
  expect(screen.getByText("Scan unavailable")).toBeTruthy();
});

describe("production native boundary", () => {
  it("refuses sample data and fake mutation success outside Tauri", async () => {
    vi.stubEnv("DEV", false);
    await expect(api.getState()).rejects.toMatchObject({ code: "tauri_unavailable" });
    await expect(api.rescan()).rejects.toMatchObject({ code: "tauri_unavailable" });
    await expect(api.resetAppData()).rejects.toMatchObject({ code: "tauri_unavailable" });
    await expect(api.copyValue("sample", "identity")).rejects.toMatchObject({ code: "tauri_unavailable" });
  });
});
