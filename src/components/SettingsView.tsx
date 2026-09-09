import { ChevronRight, Plus } from "lucide-react";
import { memo, useState } from "react";
import type { FormEvent } from "react";
import { api } from "../lib/api";
import { useConnLensStore } from "../lib/store";
import type { SettingsState } from "../lib/types";

export const SettingsView = memo(function SettingsView({ settings }: { settings?: SettingsState }) {
  const { updateSettings, reviewCleanup, resetAppData } = useConnLensStore.getState();
  const [confirmReset, setConfirmReset] = useState(false);
  if (!settings) return <section className="settings-view"><p>Settings are unavailable until ConnLens loads.</p><button onClick={() => void useConnLensStore.getState().load()}>Try again</button></section>;
  return <section className="settings-view" aria-label="Settings">
    <h2>Settings</h2>
    <section className="settings-card">
      <ToggleRow label="Watch local files" checked={settings.watchersEnabled} onChange={(checked) => void updateSettings({ watchersEnabled: checked })} />
      <ToggleRow label="Notifications" checked={settings.toastsEnabled} onChange={(checked) => void updateSettings({ toastsEnabled: checked })} />
      <ToggleRow label="Start at login" checked={settings.autostart} onChange={(checked) => void updateSettings({ autostart: checked })} />
      <label className="field-row">Fallback interval<select aria-label="Fallback scan interval" value={settings.pollMinutes} onChange={(event) => void updateSettings({ pollMinutes: Number(event.currentTarget.value) })}><option value={5}>5 min</option><option value={10}>10 min</option><option value={30}>30 min</option></select></label>
    </section>
    <details className="settings-card custom-provider"><summary><span>Custom providers<small>Add a local config or environment source.</small></span><ChevronRight size={16} /></summary><CustomProviderForm /></details>
    <section className="settings-card data-settings"><h3>Data</h3><button onClick={() => void reviewCleanup()}>Review cleanup<ChevronRight size={14} /></button>{!confirmReset ? <button onClick={() => setConfirmReset(true)}>Reset app data<ChevronRight size={14} /></button> : <div className="reset-confirm"><p>Clear ConnLens history, custom providers and settings? Your source account files stay unchanged.</p><button className="danger-action" onClick={() => { void resetAppData(); setConfirmReset(false); }}>Confirm reset</button><button onClick={() => setConfirmReset(false)}>Cancel</button></div>}</section>
    <div className="privacy-note">Accounts stay on this device.<a href="https://nemu.ae" onClick={(event) => { event.preventDefault(); void api.openExternalUrl("https://nemu.ae").catch(() => useConnLensStore.getState().setToast("Could not open Nemu.")); }}>Made by Nemu</a></div>
  </section>;
});
function ToggleRow({ label, checked, onChange }: { label: string; checked: boolean; onChange: (value: boolean) => void }) {
  return <label className="toggle-row"><span>{label}</span><input type="checkbox" role="switch" aria-label={label} checked={checked} onChange={(event) => onChange(event.currentTarget.checked)} /></label>;
}
function CustomProviderForm() {
  const add = useConnLensStore((state) => state.addCustomProvider);
  const [name, setName] = useState("");
  const [id, setId] = useState("");
  const [url, setUrl] = useState("");
  const [format, setFormat] = useState<"json" | "yaml" | "ini" | "toml">("json");
  const [paths, setPaths] = useState("");
  const [env, setEnv] = useState("");
  const [saving, setSaving] = useState(false);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!paths.trim() && !env.trim()) { useConnLensStore.getState().setToast("Add at least one config path or environment variable."); return; }
    setSaving(true);
    const success = await add({ id: id.trim(), name: name.trim(), dashboardUrl: url.trim() || null, format, configPaths: splitList(paths), envVars: splitList(env) });
    setSaving(false);
    if (success) { setName(""); setId(""); setUrl(""); setPaths(""); setEnv(""); setFormat("json"); }
  }
  return <form className="custom-provider-form" onSubmit={(event) => void submit(event)}>
    <label>Name<input aria-label="Custom provider name" value={name} onChange={(event) => setName(event.target.value)} placeholder="My CLI" required /></label>
    <label>ID<input aria-label="Custom provider ID" value={id} onChange={(event) => setId(event.target.value)} placeholder="my-cli" pattern={"[a-z0-9_\\-]+"} /></label>
    <label className="wide-field">Dashboard URL<input aria-label="Custom provider dashboard URL" type="url" value={url} onChange={(event) => setUrl(event.target.value)} placeholder="https://example.com" /></label>
    <label className="wide-field">Format<select aria-label="Custom provider config format" value={format} onChange={(event) => setFormat(event.target.value as typeof format)}><option value="json">JSON</option><option value="yaml">YAML</option><option value="ini">INI</option><option value="toml">TOML</option></select></label>
    <label className="wide-field">Config paths<textarea aria-label="Custom provider config paths" rows={2} value={paths} onChange={(event) => setPaths(event.target.value)} placeholder="~/.my-cli/config.json" /></label>
    <label className="wide-field">Environment variables<textarea aria-label="Custom provider environment variables" rows={2} value={env} onChange={(event) => setEnv(event.target.value)} placeholder="MY_CLI_TOKEN" /></label>
    <button disabled={saving} type="submit"><Plus size={13} />{saving ? "Adding…" : "Add custom provider"}</button>
  </form>;
}
function splitList(value: string) { return value.split(/[\n,]+/).map((item) => item.trim()).filter(Boolean); }
