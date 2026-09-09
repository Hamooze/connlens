import { ChevronLeft, Grid2X2, Link2, MoreHorizontal, Power, RefreshCw, Search, Settings, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useShallow } from "zustand/react/shallow";
import logo from "./assets/connlens.svg";
import "./App.css";
import { api, isNativeApp, previewPlatform } from "./lib/api";
import { filterConnections, groupConnections } from "./lib/connectionView";
import { formatScanTime } from "./lib/format";
import { useConnLensStore } from "./lib/store";
import { ProviderLogo, popularProviderIds, providerName } from "./components/providers";
import { ConnectionList } from "./components/ConnectionList";
import { SettingsView } from "./components/SettingsView";

function App() {
  const { snapshot, query, loading, view, toast } = useConnLensStore(useShallow((state) => ({
    snapshot: state.snapshot, query: state.query, loading: state.loading, view: state.view, toast: state.toast,
  })));
  const { setQuery, setView, load, setToast } = useConnLensStore.getState();
  const [platform, setPlatform] = useState(previewPlatform);
  const [provider, setProvider] = useState<string | null>(null);
  const [more, setMore] = useState(false);
  const content = useRef<HTMLElement>(null);

  useEffect(() => {
    let active = true;
    void api.getPlatform().then((value) => { if (active) setPlatform(value); });
    const subscription = api.subscribeState((next) => useConnLensStore.getState().setSnapshot(next));
    // Subscribe before reading state so a scan completion cannot be lost.
    void subscription.then(() => { if (active) void load(); }).catch(() => { if (active) void load(); });
    return () => { active = false; void subscription.then((dispose) => dispose?.()).catch(() => undefined); };
  }, [load]);

  useEffect(() => {
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        if (more) setMore(false);
        else if (view === "settings") setView("list");
        else if (query || provider) { setQuery(""); setProvider(null); }
        else void api.closeWindow().catch(() => setToast("Could not hide the window."));
      }
    };
    window.addEventListener("keydown", escape);
    return () => window.removeEventListener("keydown", escape);
  }, [more, view, query, provider, setQuery, setView, setToast]);

  useEffect(() => { if (content.current) content.current.scrollTop = 0; }, [query, provider, view]);
  const filtered = useMemo(() => filterConnections(snapshot?.connections ?? [], query)
    .filter((row) => (!provider || row.provider === provider) && (!row.hidden || snapshot?.settings.showHidden)),
    [query, provider, snapshot?.connections, snapshot?.settings.showHidden]);
  const groups = useMemo(() => groupConnections(filtered).sort((a, b) => {
    const rank = (id: string) => { const index = popularProviderIds.indexOf(id); return index < 0 ? popularProviderIds.length : index; };
    return rank(a.provider) - rank(b.provider) || a.providerName.localeCompare(b.providerName);
  }), [filtered]);
  const allIds = useMemo(() => [...new Set([
    ...popularProviderIds, ...(snapshot?.connections ?? []).map((row) => row.provider),
  ])], [snapshot?.connections]);
  const pick = (id: string | null) => { setProvider(id); setQuery(""); setView("list"); setMore(false); };
  const health = snapshot?.watcherHealth;
  const healthText = loading ? "Scanning local files…" : !snapshot ? "Scan unavailable" : health === "paused" ? "Watching paused" : health === "degraded" ? "Using fallback scans" : "Watching local files";

  return (
    <div className={`window-frame platform-${platform}`}>
      <main className={`app-shell ${view === "settings" ? "is-settings" : ""}`}>
        <header className="titlebar" onMouseDown={(event) => {
          if (event.button === 0 && !(event.target as HTMLElement).closest("button")) void api.startWindowDrag().catch(() => undefined);
        }}>
          {view === "settings" ? <button className="title-control back" aria-label="Back to connections" onClick={() => setView("list")}><ChevronLeft size={17} /></button> : null}
          <div className="brand"><img className="brand-logo" src={logo} alt="ConnLens logo" width={30} height={30} draggable={false} /><h1>ConnLens</h1></div>
          <button className="title-control close" aria-label="Hide ConnLens" title="Hide ConnLens" onClick={() => void api.closeWindow().catch(() => setToast("Could not hide the window."))}><X size={15} /></button>
        </header>

        <nav className="provider-rail" aria-label="Provider filters">
          <button className={!provider && view === "list" ? "selected" : ""} aria-label="All providers" title="All providers" aria-pressed={!provider && view === "list"} onClick={() => pick(null)}><Grid2X2 size={18} /></button>
          {allIds.slice(0, 5).map((id) => <button key={id} className={provider === id && view === "list" ? "selected" : ""} aria-label={`${providerName(id)} connections`} title={providerName(id)} aria-pressed={provider === id && view === "list"} onClick={() => pick(id)}><ProviderLogo provider={id} /></button>)}
          <button className={more || (provider && !allIds.slice(0, 5).includes(provider)) ? "selected" : ""} aria-label="More providers" aria-expanded={more} title="More providers" onClick={() => setMore(!more)}><MoreHorizontal size={19} /></button>
        </nav>
        {more ? <div className="provider-picker" aria-label="More provider filters">{allIds.slice(5).map((id) => <button key={id} onClick={() => pick(id)} aria-label={`${providerName(id)} connections`}><ProviderLogo provider={id} /><span>{providerName(id)}</span></button>)}</div> : null}

        {view === "list" ? <>
          <div className="search-wrap"><Search size={16} /><input aria-label="Search connections" placeholder="Search connections" value={query} onChange={(event) => setQuery(event.currentTarget.value)} />{query ? <button aria-label="Clear search" onClick={() => setQuery("")}><X size={14} /></button> : null}</div>
          <div className="section-heading"><h2>{provider ? providerName(provider) : "Connections"} <span>({filtered.length})</span></h2><button aria-label="Rescan" title="Rescan local files" disabled={loading} onClick={() => void useConnLensStore.getState().rescan()}><RefreshCw size={15} className={loading ? "spinning" : ""} /></button></div>
          <section ref={content} className="content-region" aria-label="Detected connections" aria-busy={loading}>
            {snapshot?.historyResetNotice ? <div className="notice" role="status">History recovered from backup.<button onClick={() => void useConnLensStore.getState().dismissHistoryNotice()}>Dismiss</button></div> : null}
            {snapshot?.providerErrors.length ? <details className="scan-errors"><summary>{snapshot.providerErrors.length} source{snapshot.providerErrors.length === 1 ? " needs" : "s need"} attention</summary>{snapshot.providerErrors.map((error, i) => <p key={`${error.provider}-${i}`}><strong>{providerName(error.provider)}</strong>: {error.message}</p>)}</details> : null}
            {loading && !snapshot ? <div className="skeleton-list" aria-label="Loading connections"><span /><span /><span /></div> : null}
            {!loading && !snapshot ? <div className="empty-state"><Link2 size={26} /><strong>Unable to load connections</strong><p>{toast ?? "Try scanning again."}</p><button onClick={() => void load()}>Try again</button></div> : null}
            {!loading && snapshot && !groups.length ? <div className="empty-state"><Search size={26} /><strong>{query || provider ? "No matching connections" : "No connections detected"}</strong><p>{query || provider ? "Try another provider or clear your search." : "Accounts appear here when a supported local config is found."}</p>{query || provider ? <button onClick={() => pick(null)}>Show all connections</button> : <button onClick={() => void useConnLensStore.getState().rescan()}>Rescan</button>}</div> : null}
            <ConnectionList groups={groups} />
          </section>
          <div className="scan-status" role="status"><span className={`watcher ${health ?? "paused"}`} /><div>{healthText}<small>{!isNativeApp() ? "Preview data · " : ""}{snapshot?.lastScan ? `Last scan: ${formatScanTime(snapshot.lastScan)}` : "No completed scan"}</small></div></div>
        </> : <SettingsView settings={snapshot?.settings} />}
        <footer className="footer"><button onClick={() => setView(view === "list" ? "settings" : "list")}>{view === "list" ? <Settings size={14} /> : <Grid2X2 size={14} />}{view === "list" ? "Settings" : "Connections"}</button><button aria-label="Quit ConnLens" onClick={() => void api.quitApp().catch(() => setToast("Could not quit ConnLens."))}><Power size={14} />Quit</button></footer>
        {toast ? <div className="toast" role="status"><span>{toast}</span><button aria-label="Dismiss message" onClick={() => setToast(null)}><X size={13} /></button></div> : null}
      </main>
    </div>
  );
}
export default App;
