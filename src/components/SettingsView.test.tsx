import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { SettingsView } from "./SettingsView";
import { api } from "../lib/api";
import { useConnLensStore } from "../lib/store";
import type { ConnLensSnapshot } from "../lib/types";

const fixture: ConnLensSnapshot = {
  schemaVersion: 1, connections: [], lastScan: null, providerErrors: [], watcherHealth: "ok", historyResetNotice: false,
  settings: { watchersEnabled: true, pollMinutes: 10, toastsEnabled: true, probesEnabled: false,
    providerToggles: {}, projectRoots: [], theme: "dark", collapsedProviders: {}, showHidden: false,
    historyResetNoticeDismissed: false, autostart: false },
};

beforeEach(() => useConnLensStore.setState({ snapshot: fixture, toast: null, loading: false }));
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

it("retains all custom provider fields when the native save fails", async () => {
  vi.spyOn(api, "addCustomProvider").mockRejectedValue(new Error("Provider already exists"));
  render(<SettingsView settings={fixture.settings} />);
  fireEvent.click(screen.getByText("Custom providers"));
  const name = screen.getByLabelText("Custom provider name") as HTMLInputElement;
  const id = screen.getByLabelText("Custom provider ID") as HTMLInputElement;
  const url = screen.getByLabelText("Custom provider dashboard URL") as HTMLInputElement;
  const paths = screen.getByLabelText("Custom provider config paths") as HTMLTextAreaElement;
  const env = screen.getByLabelText("Custom provider environment variables") as HTMLTextAreaElement;
  fireEvent.change(name, { target: { value: "Fixture CLI" } });
  fireEvent.change(id, { target: { value: "fixture-cli" } });
  fireEvent.change(url, { target: { value: "https://example.test/dashboard" } });
  fireEvent.change(paths, { target: { value: "~/.fixture/config.json" } });
  fireEvent.change(env, { target: { value: "FIXTURE_TOKEN" } });
  fireEvent.change(screen.getByLabelText("Custom provider config format"), { target: { value: "toml" } });
  fireEvent.click(screen.getByRole("button", { name: "Add custom provider" }));
  await waitFor(() => expect(useConnLensStore.getState().toast).toBe("Provider already exists"));
  expect(name.value).toBe("Fixture CLI");
  expect(id.value).toBe("fixture-cli");
  expect(url.value).toBe("https://example.test/dashboard");
  expect(paths.value).toBe("~/.fixture/config.json");
  expect(env.value).toBe("FIXTURE_TOKEN");
  expect((screen.getByLabelText("Custom provider config format") as HTMLSelectElement).value).toBe("toml");
  expect((screen.getByRole("button", { name: "Add custom provider" }) as HTMLButtonElement).disabled).toBe(false);
});
