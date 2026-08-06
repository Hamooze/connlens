import {
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  CircleAlert,
  Clipboard,
  Copy,
  ExternalLink,
  FolderOpen,
  MoreVertical,
  RefreshCw,
  Search,
  Settings,
  ShieldCheck,
  Trash2,
  X,
} from "lucide-react";
import {
  siCloudflare,
  siDocker,
  siGithub,
  siGitlab,
  siGooglecloud,
  siNeon,
  siNetlify,
  siNpm,
  siSentry,
  siShopify,
  siStripe,
  siSupabase,
  siVercel,
} from "simple-icons";
import type { SimpleIcon } from "simple-icons";
import type { ComponentType, FormEvent, MouseEvent } from "react";
import { useEffect, useMemo, useState } from "react";
import "./App.css";
import { api } from "./lib/api";
import {
  filterConnections,
  groupConnections,
  statusLabel,
} from "./lib/connectionView";
import { formatScanTime, formatTimestamp } from "./lib/format";
import { useConnLensStore } from "./lib/store";
import type {
  Connection,
  ConnectionStatus,
  ProviderGroup,
  SettingsState,
} from "./lib/types";

interface ProviderVisual {
  name: string;
  color: string;
  icon?: SimpleIcon;
  monogram?: string;
}

const providerVisuals: Record<string, ProviderVisual> = {
  github: { name: "GitHub", color: "#f5f5f5", icon: siGithub },
  gitlab: { name: "GitLab", color: "#fc6d26", icon: siGitlab },
  aws: { name: "AWS", color: "#ff9900", monogram: "AWS" },
  azure: { name: "Azure", color: "#50a7ff", monogram: "AZ" },
  vercel: { name: "Vercel", color: "#ffffff", icon: siVercel },
  neon: { name: "Neon DB", color: "#00e599", icon: siNeon },
  docker: { name: "Docker", color: "#2496ed", icon: siDocker },
  npm: { name: "npm", color: "#cb3837", icon: siNpm },
  gcloud: { name: "Google Cloud", color: "#4285f4", icon: siGooglecloud },
  netlify: { name: "Netlify", color: "#00c7b7", icon: siNetlify },
  cloudflare: { name: "Cloudflare", color: "#f38020", icon: siCloudflare },
  stripe: { name: "Stripe", color: "#635bff", icon: siStripe },
  shopify: { name: "Shopify", color: "#95bf47", icon: siShopify },
  supabase: { name: "Supabase", color: "#3ecf8e", icon: siSupabase },
  sentry: { name: "Sentry", color: "#fb4226", icon: siSentry },
};

const popularProviderIds = [
  "github",
  "aws",
  "vercel",
  "neon",
  "docker",
  "npm",
  "gitlab",
  "gcloud",
  "azure",
  "netlify",
  "cloudflare",
  "stripe",
  "supabase",
  "shopify",
  "sentry",
];

function App() {
  const {
    snapshot,
    query,
    loading,
    expandedId,
    view,
    toast,
    setQuery,
    setExpandedId,
    setView,
    load,
    setToast,
  } = useConnLensStore();

  useEffect(() => {
    load();
    const unlisten = api.subscribeState((next) => {
      useConnLensStore.getState().setSnapshot(next);
    });
    return () => {
      void unlisten.then((dispose) => dispose?.());
    };
  }, [load]);

  const filtered = useMemo(
    () => filterConnections(snapshot?.connections ?? [], query),
    [query, snapshot?.connections],
  );
  const groups = useMemo(() => groupConnections(filtered), [filtered]);
  const connectedCounts = useMemo(() => {
    const counts = new Map<string, number>();
    for (const connection of snapshot?.connections ?? []) {
      counts.set(connection.provider, (counts.get(connection.provider) ?? 0) + 1);
    }
    return counts;
  }, [snapshot?.connections]);

  return (
    <main className="app-shell">
      <Header view={view} onBack={() => setView("list")} />

      {view === "list" ? (
        <>
          <SearchBar value={query} onChange={setQuery} />
          <ProviderRail counts={connectedCounts} onPick={setQuery} />
          <section className="content-region" aria-label="Detected connections">
            {loading && !snapshot ? <SkeletonList /> : null}
            {!loading && snapshot?.historyResetNotice ? <HistoryNotice /> : null}
            {!loading && groups.length === 0 ? (
              <EmptyState hasQuery={query.trim().length > 0} query={query} />
            ) : null}
            {groups.map((group) => (
              <ProviderSection
                key={group.provider}
                group={group}
                expandedId={expandedId}
                onExpand={setExpandedId}
              />
            ))}
          </section>
          <Footer />
        </>
      ) : (
        <SettingsView settings={snapshot?.settings} />
      )}

      {toast ? (
        <button className="toast" type="button" onClick={() => setToast(null)}>
          {toast}
        </button>
      ) : null}
    </main>
  );
}

function Header({ view, onBack }: { view: string; onBack: () => void }) {
  const startDrag = (event: MouseEvent<HTMLElement>) => {
    if (event.button === 0) void api.startWindowDrag();
  };

  return (
    <header className="titlebar" onMouseDown={startDrag}>
      {view === "settings" ? (
        <button
          className="title-control"
          type="button"
          onMouseDown={(event) => event.stopPropagation()}
          onClick={onBack}
          aria-label="Back"
        >
          <ChevronLeft size={17} />
        </button>
      ) : (
        <div className="brand-mark" aria-hidden="true">
          <span />
          <span />
          <span />
        </div>
      )}
      <h1>{view === "settings" ? "Settings" : "ConnLens"}</h1>
      <button
        className="title-control close"
        type="button"
        onMouseDown={(event) => event.stopPropagation()}
        onClick={() => void api.closeWindow()}
        aria-label="Close ConnLens"
        title="Close"
      >
        <X size={17} />
      </button>
    </header>
  );
}

function SearchBar({
  value,
  onChange,
}: {
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <div className="search-wrap">
      <Search size={17} />
      <input
        aria-label="Search connections"
        value={value}
        onChange={(event) => onChange(event.currentTarget.value)}
        placeholder="Search providers, identities, hosts..."
      />
    </div>
  );
}

function ProviderRail({
  counts,
  onPick,
}: {
  counts: Map<string, number>;
  onPick: (value: string) => void;
}) {
  return (
    <section className="provider-rail" aria-label="Popular providers">
      {popularProviderIds.map((id) => {
        const visual = providerVisuals[id];
        return (
          <button key={id} type="button" onClick={() => onPick(visual.name)}>
            <ProviderLogo provider={id} />
            <span>{visual.name}</span>
            <b>{counts.get(id) ?? 0}</b>
          </button>
        );
      })}
    </section>
  );
}

function ProviderSection({
  group,
  expandedId,
  onExpand,
}: {
  group: ProviderGroup;
  expandedId: string | null;
  onExpand: (id: string | null) => void;
}) {
  const collapsed =
    useConnLensStore.getState().snapshot?.settings.collapsedProviders[
      group.provider
    ] ?? false;
  const counts = group.connections.reduce(
    (acc, row) => {
      acc[row.status] = (acc[row.status] ?? 0) + 1;
      return acc;
    },
    {} as Record<ConnectionStatus, number>,
  );

  return (
    <section className="provider-section">
      <button
        className="provider-heading"
        type="button"
        onClick={() =>
          useConnLensStore.getState().toggleProviderCollapsed(group.provider)
        }
      >
        {collapsed ? <ChevronRight size={15} /> : <ChevronDown size={15} />}
        <ProviderLogo provider={group.provider} />
        <strong>{displayProviderName(group.provider, group.providerName)}</strong>
        <span className="count">({group.connections.length})</span>
        <span className="status-summary">
          {counts.active ? <b>{counts.active} active</b> : null}
          {counts.changed ? <em>{counts.changed} changed</em> : null}
          {counts.missing ? <i>{counts.missing} missing</i> : null}
        </span>
      </button>

      {!collapsed
        ? group.connections.map((connection) => (
            <ConnectionRow
              key={connection.id}
              connection={connection}
              expanded={expandedId === connection.id}
              onExpand={() =>
                onExpand(expandedId === connection.id ? null : connection.id)
              }
            />
          ))
        : null}
    </section>
  );
}

function ConnectionRow({
  connection,
  expanded,
  onExpand,
}: {
  connection: Connection;
  expanded: boolean;
  onExpand: () => void;
}) {
  const sourcePath = connection.source.path ?? "local registry";
  const status = statusLabel(connection.status);

  return (
    <article className={`connection-row ${expanded ? "expanded" : ""}`}>
      <div className="row-main">
        <button
          className="row-expand-button"
          type="button"
          onClick={onExpand}
          aria-expanded={expanded}
        >
          <StatusDot status={connection.status} seen={connection.seen} />
          <ProviderLogo provider={connection.provider} />
          <span className="identity">
            <strong title={connection.identity.label}>{connection.identity.label}</strong>
            <small title={connection.identity.scope ?? connection.identity.host ?? ""}>
              {connection.identity.scope ?? connection.identity.host ?? "local"}
            </small>
          </span>
          <span className="host" title={connection.identity.host ?? connection.identity.scope ?? ""}>
            {connection.identity.host ?? connection.identity.scope ?? "-"}
          </span>
          <span className={`status-chip ${connection.status}`}>{status}</span>
        </button>
        <RowAction
          label="Open dashboard"
          icon={ExternalLink}
          onClick={() => useConnLensStore.getState().openDashboard(connection.id)}
        />
        <RowAction
          label="Copy identity"
          icon={Copy}
          onClick={() => useConnLensStore.getState().copyValue(connection.id, "identity")}
        />
        <span className="more" aria-hidden="true">
          <MoreVertical size={16} />
        </span>
      </div>

      {expanded ? (
        <div className="row-details">
          <dl>
            <div>
              <dt>Source</dt>
              <dd title={sourcePath}>{sourcePath}</dd>
            </div>
            <div>
              <dt>Fingerprint</dt>
              <dd>{connection.fingerprint ?? "none"}</dd>
            </div>
            <div>
              <dt>Scope</dt>
              <dd>{connection.identity.scope ?? "default"}</dd>
            </div>
            <div>
              <dt>Last seen</dt>
              <dd>{formatTimestamp(connection.lastSeen)}</dd>
            </div>
          </dl>
          <div className="action-strip">
            <button
              type="button"
              onClick={() =>
                useConnLensStore.getState().copyValue(connection.id, "fingerprint")
              }
            >
              <Clipboard size={15} />
              Copy Fingerprint
            </button>
            <button
              type="button"
              onClick={() => useConnLensStore.getState().openDashboard(connection.id)}
            >
              <ExternalLink size={15} />
              Open Dashboard
            </button>
            <button
              type="button"
              disabled={connection.source.sourceType !== "config_file"}
              onClick={() => useConnLensStore.getState().revealSource(connection.id)}
            >
              <FolderOpen size={15} />
              Reveal Source
            </button>
            <button
              className="danger-action"
              type="button"
              onClick={() => useConnLensStore.getState().removeConnection(connection.id)}
            >
              <Trash2 size={15} />
              Delete
            </button>
          </div>
        </div>
      ) : null}
    </article>
  );
}

function RowAction({
  label,
  icon: Icon,
  onClick,
}: {
  label: string;
  icon: ComponentType<{ size?: number }>;
  onClick: () => void;
}) {
  return (
    <button
      className="row-icon-button"
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
    >
      <Icon size={16} />
    </button>
  );
}

function ProviderLogo({ provider }: { provider: string }) {
  const visual = providerVisuals[provider] ?? {
    name: provider,
    color: "#d7d7d7",
    monogram: provider.slice(0, 2).toUpperCase(),
  };

  return (
    <span className="provider-logo" style={{ color: visual.color }} aria-hidden="true">
      {visual.icon ? (
        <svg viewBox="0 0 24 24" role="img">
          <path d={visual.icon.path} />
        </svg>
      ) : (
        <span>{visual.monogram}</span>
      )}
    </span>
  );
}

function displayProviderName(provider: string, fallback: string) {
  return providerVisuals[provider]?.name ?? fallback;
}

function StatusDot({
  status,
  seen,
}: {
  status: ConnectionStatus;
  seen: boolean;
}) {
  return <span className={`status-dot ${status} ${seen ? "" : "unseen"}`} />;
}

function Footer() {
  const snapshot = useConnLensStore((state) => state.snapshot);
  const setView = useConnLensStore((state) => state.setView);
  const rescan = useConnLensStore((state) => state.rescan);
  const health = snapshot?.watcherHealth ?? "ok";
  const healthText =
    health === "paused" ? "Watcher paused" : health === "degraded" ? "Watcher degraded" : "Watcher healthy";
  return (
    <footer className="footer">
      <span className="scan-stamp">
        <b>Scan complete</b>
        <time>{formatScanTime(snapshot?.lastScan)}</time>
      </span>
      <span className={`watcher ${health}`} aria-label={healthText} title={healthText} />
      <button type="button" onClick={() => rescan()} aria-label="Rescan" title="Rescan">
        <RefreshCw size={16} />
      </button>
      <button type="button" onClick={() => setView("settings")} aria-label="Settings">
        <Settings size={17} />
      </button>
    </footer>
  );
}

function EmptyState({ hasQuery, query }: { hasQuery: boolean; query: string }) {
  const rescan = useConnLensStore((state) => state.rescan);
  return (
    <div className="empty-state">
      <ShieldCheck size={28} />
      <strong>{hasQuery ? `No matches for '${query}'` : "No connections detected"}</strong>
      <p>
        {hasQuery
          ? "Clear search to restore the full list."
          : "ConnLens scans local developer account files only."}
      </p>
      <button type="button" onClick={() => rescan()}>
        <RefreshCw size={15} />
        Rescan
      </button>
    </div>
  );
}

function SkeletonList() {
  return (
    <div className="skeleton-list">
      {Array.from({ length: 7 }).map((_, index) => (
        <span key={index} />
      ))}
    </div>
  );
}

function HistoryNotice() {
  return (
    <div className="notice">
      <CircleAlert size={15} />
      Registry history recovered from backup.
      <button
        type="button"
        onClick={() => useConnLensStore.getState().dismissHistoryNotice()}
      >
        Dismiss
      </button>
    </div>
  );
}

function SettingsView({ settings }: { settings?: SettingsState }) {
  const updateSettings = useConnLensStore((state) => state.updateSettings);
  const purgeMissing = useConnLensStore((state) => state.purgeMissing);
  const resetAppData = useConnLensStore((state) => state.resetAppData);
  const current = settings;

  if (!current) {
    return <SkeletonList />;
  }

  return (
    <section className="settings-view">
      <section>
        <h2>Scan Controls</h2>
        <ToggleRow
          label="Watchers"
          checked={current.watchersEnabled}
          onChange={(checked) => updateSettings({ watchersEnabled: checked })}
        />
        <ToggleRow
          label="Toasts"
          checked={current.toastsEnabled}
          onChange={(checked) => updateSettings({ toastsEnabled: checked })}
        />
        <ToggleRow
          label="Probe enrichment"
          checked={current.probesEnabled}
          onChange={(checked) => updateSettings({ probesEnabled: checked })}
        />
        <label className="field-row">
          Poll interval
          <select
            value={current.pollMinutes}
            onChange={(event) =>
              updateSettings({ pollMinutes: Number(event.currentTarget.value) })
            }
          >
            <option value={5}>5 min</option>
            <option value={10}>10 min</option>
            <option value={30}>30 min</option>
          </select>
        </label>
      </section>

      <CustomProviderForm />

      <section>
        <h2>Data</h2>
        <button type="button" onClick={() => purgeMissing()}>
          Purge Missing
        </button>
        <button className="danger" type="button" onClick={() => resetAppData()}>
          Reset App Data
        </button>
      </section>

      <div className="powered-by" aria-label="Powered by Nemu.ae">
        <span>Powered by</span>
        <a
          href="https://nemu.ae"
          target="_blank"
          rel="noreferrer"
          aria-label="Open Nemu.ae"
          onClick={(event) => {
            event.preventDefault();
            void api.openExternalUrl("https://nemu.ae");
          }}
        >
          <img src="/nemulogo_withouttxt.png" alt="" />
          <span>nemu.ae</span>
        </a>
      </div>
    </section>
  );
}

function CustomProviderForm() {
  const addCustomProvider = useConnLensStore((state) => state.addCustomProvider);
  const [name, setName] = useState("");
  const [providerId, setProviderId] = useState("");
  const [dashboardUrl, setDashboardUrl] = useState("");
  const [format, setFormat] = useState<"json" | "yaml" | "ini" | "toml">("json");
  const [paths, setPaths] = useState("");
  const [envVars, setEnvVars] = useState("");

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    await addCustomProvider({
      id: providerId.trim(),
      name: name.trim(),
      dashboardUrl: dashboardUrl.trim() || null,
      format,
      configPaths: splitList(paths),
      envVars: splitList(envVars),
    });
    setName("");
    setProviderId("");
    setDashboardUrl("");
    setPaths("");
    setEnvVars("");
    setFormat("json");
  };

  return (
    <section>
      <h2>Custom Sources</h2>
      <form className="custom-provider-form" onSubmit={submit}>
        <label>
          Name
          <input
            aria-label="Custom provider name"
            value={name}
            onChange={(event) => setName(event.currentTarget.value)}
            placeholder="Weird CLI"
            required
          />
        </label>
        <label>
          ID
          <input
            aria-label="Custom provider ID"
            value={providerId}
            onChange={(event) => setProviderId(event.currentTarget.value)}
            placeholder="weird-cli"
          />
        </label>
        <label className="wide-field">
          Dashboard URL
          <input
            aria-label="Custom provider dashboard URL"
            value={dashboardUrl}
            onChange={(event) => setDashboardUrl(event.currentTarget.value)}
            placeholder="https://example.com"
          />
        </label>
        <label>
          Format
          <select
            aria-label="Custom provider config format"
            value={format}
            onChange={(event) =>
              setFormat(event.currentTarget.value as "json" | "yaml" | "ini" | "toml")
            }
          >
            <option value="json">JSON</option>
            <option value="yaml">YAML</option>
            <option value="ini">INI</option>
            <option value="toml">TOML</option>
          </select>
        </label>
        <label className="wide-field">
          Config paths
          <textarea
            aria-label="Custom provider config paths"
            value={paths}
            onChange={(event) => setPaths(event.currentTarget.value)}
            placeholder="%USERPROFILE%/.weird/config.json"
            rows={3}
          />
        </label>
        <label className="wide-field">
          Env vars
          <textarea
            aria-label="Custom provider environment variables"
            value={envVars}
            onChange={(event) => setEnvVars(event.currentTarget.value)}
            placeholder="WEIRD_API_KEY"
            rows={2}
          />
        </label>
        <button type="submit">Add Custom Provider</button>
      </form>
    </section>
  );
}

function splitList(value: string) {
  return value
    .split(/[\n,]+/)
    .map((item) => item.trim())
    .filter(Boolean);
}

function ToggleRow({
  label,
  checked,
  provider,
  count,
  onChange,
}: {
  label: string;
  checked: boolean;
  provider?: string;
  count?: number;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="toggle-row">
      <span>
        {provider ? <ProviderLogo provider={provider} /> : null}
        {label}
      </span>
      {typeof count === "number" ? <b>{count}</b> : null}
      <input
        type="checkbox"
        checked={checked}
        onChange={(event) => onChange(event.currentTarget.checked)}
      />
    </label>
  );
}

export default App;
