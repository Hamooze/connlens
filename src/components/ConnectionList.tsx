import { ChevronDown, ChevronRight, Copy, ExternalLink, FolderOpen, Trash2 } from "lucide-react";
import { memo, useMemo } from "react";
import { formatTimestamp } from "../lib/format";
import { useConnLensStore } from "../lib/store";
import type { Connection, ProviderGroup } from "../lib/types";
import { availabilityLabel, availabilityOf, toolPresenceLabel, toolPresenceOf, usageLabel } from "../lib/validation";
import { ProviderLogo, displayProviderName } from "./providers";


export const ConnectionList = memo(function ConnectionList({ groups }: { groups: ProviderGroup[] }) {
  return groups.map((group) => <ProviderSection key={group.provider} group={group} />);
});
const ProviderSection = memo(function ProviderSection({ group }: { group: ProviderGroup }) {
  const collapsed = useConnLensStore((state) => state.snapshot?.settings.collapsedProviders[group.provider] ?? false);
  const toggle = useConnLensStore((state) => state.toggleProviderCollapsed);
  const labels = useMemo(() => {
    const counts = new Map<string, number>();
    for (const row of group.connections) counts.set(row.identity.label, (counts.get(row.identity.label) ?? 0) + 1);
    return counts;
  }, [group.connections]);
  return <section className="provider-section">
    <button className="provider-heading" onClick={() => toggle(group.provider)} aria-expanded={!collapsed} aria-label={`${displayProviderName(group.provider, group.providerName)} group`}>
      <ProviderLogo provider={group.provider} /><strong>{displayProviderName(group.provider, group.providerName)}</strong><span className="count">({group.connections.length})</span>{collapsed ? <ChevronRight size={14} /> : <ChevronDown size={14} />}
    </button>
    {!collapsed ? group.connections.map((connection) => <ConnectionRow key={connection.id} connection={connection} showContext={connection.provider === "mcp_servers" || (labels.get(connection.identity.label) ?? 0) > 1} />) : null}
  </section>;
});
const ConnectionRow = memo(function ConnectionRow({ connection, showContext }: { connection: Connection; showContext: boolean }) {
  const expanded = useConnLensStore((state) => state.expandedId === connection.id);
  const { setExpandedId, copyValue, openDashboard, revealSource, reviewCleanup } = useConnLensStore.getState();
  const tool = toolPresenceOf(connection);
  const mcp = connection.provider === "mcp_servers";
  const context = [connection.identity.scope, connection.source.path].filter(Boolean).join(" · ");
  return <article className={`connection-row ${expanded ? "expanded" : ""}`}>
    <button className="row-expand-button" aria-expanded={expanded} onClick={() => setExpandedId(expanded ? null : connection.id)}>
      <span className="identity-wrap"><span className="identity" title={connection.identity.label}>{connection.identity.label}</span>{showContext && context ? <small className="identity-context" title={context}>{context}</small> : null}</span><span className={`connection-status ${availabilityOf(connection)}`}><span className="status-dot" />{availabilityOf(connection) === "available" ? connection.status === "changed" ? "Changed" : mcp ? connection.meta.mcpDisabled === true ? "Disabled" : "Registered" : "Available" : availabilityOf(connection) === "missing" ? mcp ? "Removed" : "Missing" : "Unchecked"}</span>
    </button>
    {tool && tool.status !== "found" && availabilityOf(connection) !== "missing" ? <p className="tool-attention" title={tool.reason}>{toolPresenceLabel(connection, tool)}<span>{mcp ? "Registration remains" : "Config reference remains"}</span></p> : null}
    {expanded ? <div className="row-details">
      <dl><div><dt>Availability</dt><dd>{availabilityLabel(connection)}</dd></div><div><dt>Usage</dt><dd>{usageLabel(connection)}</dd></div><div><dt>Checked</dt><dd>{connection.validation?.checkedAt ? formatTimestamp(connection.validation.checkedAt) : "Awaiting validation"}</dd></div><div><dt>Evidence</dt><dd>{connection.validation?.reason ?? "Run validation to check this saved entry."}</dd></div><div><dt>Host</dt><dd>{connection.identity.host ?? "Local"}</dd></div><div><dt>Scope</dt><dd>{connection.identity.scope ?? "Default"}</dd></div><div><dt>Source</dt><dd>{connection.source.path ?? "Local registry"}</dd></div><div><dt>Fingerprint</dt><dd className="fingerprint">{connection.fingerprint ?? "None"}</dd></div><div><dt>Last seen</dt><dd>{formatTimestamp(connection.lastSeen)}</dd></div></dl>
      {tool ? <dl className="tool-details"><div><dt>{mcp ? "Launcher" : "CLI"}</dt><dd>{toolPresenceLabel(connection, tool)} · {tool.name}</dd></div><div><dt>Tool check</dt><dd>{tool.reason}</dd></div>{tool.path ? <div><dt>Tool path</dt><dd>{tool.path}</dd></div> : null}<div><dt>Checked</dt><dd>{formatTimestamp(tool.checkedAt)}</dd></div></dl> : mcp ? <p className="cleanup-note">{typeof connection.meta.mcpCommand === "string" ? "Launcher has not been checked yet. Registration does not prove the server is running." : "Remote endpoint is recorded locally. Connection and runtime use are not checked."}</p> : null}
      <div className="action-strip"><button onClick={() => void copyValue(connection.id, "identity")}><Copy size={13} />Copy identity</button><button onClick={() => void copyValue(connection.id, "fingerprint")} disabled={!connection.fingerprint}><Copy size={13} />Copy fingerprint</button><button disabled={mcp} onClick={() => void openDashboard(connection.id)}><ExternalLink size={13} />Open dashboard</button><button disabled={connection.source.sourceType !== "config_file" || !connection.source.path} onClick={() => void revealSource(connection.id)}><FolderOpen size={13} />Reveal source</button><button className="danger-action" onClick={() => void reviewCleanup(connection.id)}><Trash2 size={13} />Review removal</button></div>
    </div> : null}
  </article>;
});
