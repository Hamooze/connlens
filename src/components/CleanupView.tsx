import { CheckCheck, LoaderCircle, RefreshCw, Trash2 } from "lucide-react";
import { useState } from "react";
import { useShallow } from "zustand/react/shallow";
import { formatTimestamp } from "../lib/format";
import { useConnLensStore } from "../lib/store";
import type { CleanupReview } from "../lib/types";
import { toolPresenceLabel, toolPresenceOf, validationCounts } from "../lib/validation";

export function CleanupView() {
  const { review, result, loading, executing, error, targetId } = useConnLensStore(useShallow((s) => ({
    review: s.cleanupReview, result: s.cleanupResult, loading: s.cleanupLoading,
    executing: s.cleanupExecuting, error: s.cleanupError, targetId: s.cleanupTargetId,
  })));
  const reviewAgain = () => void useConnLensStore.getState().reviewCleanup(targetId ?? undefined);
  if (loading) return <section className="cleanup-view cleanup-loading" aria-busy="true"><LoaderCircle className="spinning" size={22} /><h2>Checking local sources…</h2><p>Validating availability and reviewing associated files.</p></section>;
  if (review) return <CleanupReviewForm key={review.reviewId} review={review} targetId={targetId} executing={executing} />;
  return <section className="cleanup-view cleanup-outcome" aria-label="Cleanup results">
    <h2>{error ? "Review needed" : "Cleanup results"}</h2>
    {error ? <p className="cleanup-error" role="alert">{error}</p> : null}
    {result ? <><div className="cleanup-summary"><CheckCheck size={20} /><strong>{result.removedIds.length} {result.removedIds.length === 1 ? "entry" : "entries"} removed</strong><span>{result.trashedPaths.length} {result.trashedPaths.length === 1 ? "file" : "files"} moved to Trash</span></div>
      {result.trashedPaths.map((path) => <p className="cleanup-path" key={path}>{path}</p>)}
      {result.retained.length ? <h3>Kept in history</h3> : null}
      {result.retained.map((item) => <div key={item.id}><strong className="cleanup-result-label">{item.label ?? item.id}</strong><p>{item.reason}</p></div>)}
      {result.fileFailures.length ? <h3>Files kept on disk</h3> : null}
      {result.fileFailures.map((item) => <div key={item.id}><strong className="cleanup-result-label cleanup-path">{item.path ?? item.id}</strong><p>{item.reason}</p></div>)}
    </> : null}
    <button className="cleanup-primary" onClick={reviewAgain}><RefreshCw size={13} />Review again</button>
  </section>;
}

function CleanupReviewForm({ review, targetId, executing }: { review: CleanupReview; targetId: string | null; executing: boolean }) {
  const [selected, setSelected] = useState(() => new Set(review.entries.filter((entry) => entry.eligible && (!targetId || targetId === entry.id)).map((entry) => entry.id)));
  const [includeFiles, setIncludeFiles] = useState(false);
  const [files, setFiles] = useState<Set<string>>(() => new Set());
  const counts = validationCounts(review.snapshot.connections);
  const toolIssues = [...new Map(review.snapshot.connections.flatMap((connection) => {
    const tool = toolPresenceOf(connection);
    return tool && tool.status !== "found" ? [[tool.name, { connection, tool }] as const] : [];
  })).values()];
  const canSelectFile = (fileId: string) => {
    const linked = review.entries.filter((entry) => entry.fileId === fileId);
    return linked.length > 0 && linked.every((entry) => entry.eligible && selected.has(entry.id));
  };
  const toggleEntry = (id: string, checked: boolean) => {
    setSelected((before) => { const next = new Set(before); if (checked) next.add(id); else next.delete(id); return next; });
    const fileId = review.entries.find((entry) => entry.id === id)?.fileId;
    if (!checked && fileId) setFiles((before) => { const next = new Set(before); next.delete(fileId); return next; });
  };
  return <section className="cleanup-view" aria-label="Cleanup review" aria-busy={executing}>
    <div className="cleanup-scroll">
      <h2>Validate & clean up</h2>
      <div className="validation-summary"><span><b>{counts.available}</b> Available</span><span><b>{counts.missing}</b> Missing</span><span><b>{counts.unknown}</b> Unchecked</span></div>
      <p className="cleanup-note">Checked {formatTimestamp(review.checkedAt)}. Exact duplicate records are consolidated during validation. Recent use and sign-in validity are not checked.</p>
      {toolIssues.length ? <><h3>Tool checks</h3><div className="cleanup-items">{toolIssues.map(({ connection, tool }) => <div className="cleanup-item protected" key={tool.name}><span><strong>{tool.name} · {toolPresenceLabel(connection, tool)}</strong><span>{tool.reason}</span><small>A remaining account config or MCP registration stays protected.</small></span></div>)}</div></> : null}
      <h3>Connection history</h3><p className="cleanup-note">Select confirmed-missing entries to remove from ConnLens.</p>
      <div className="cleanup-items">{review.entries.some((entry) => entry.eligible) ? review.entries.filter((entry) => entry.eligible).map((entry) => <label className={`cleanup-item ${entry.eligible ? "" : "protected"}`} key={entry.id}>
        <input type="checkbox" aria-label={`Select ${entry.label}`} checked={selected.has(entry.id)} disabled={!entry.eligible || executing} onChange={(event) => toggleEntry(entry.id, event.currentTarget.checked)} />
        <span><strong>{entry.label}</strong><small>{entry.providerName}</small><span>{entry.reason}</span></span>
      </label>) : <p>No confirmed-missing entries to remove.</p>}</div>
      <label className="cleanup-file-toggle"><input type="checkbox" aria-label="Include leftover files" checked={includeFiles} disabled={executing} onChange={(event) => { setIncludeFiles(event.currentTarget.checked); setFiles(new Set()); }} /><span>Include leftover files<small>Individually select associated files to move to Trash.</small></span></label>
      {includeFiles ? <div className="cleanup-items">{review.files.length ? review.files.map((file) => <label className={`cleanup-item ${file.eligible ? "" : "protected"}`} key={file.id}>
        <input type="checkbox" aria-label={`Move ${file.path} to Trash`} disabled={!file.eligible || !canSelectFile(file.id) || executing} checked={files.has(file.id)} onChange={(event) => { const checked = event.currentTarget.checked; setFiles((before) => { const next = new Set(before); if (checked) next.add(file.id); else next.delete(file.id); return next; }); }} />
        <span><strong className="cleanup-path">{file.path}</strong><span>{file.reason}</span>{file.eligible && !canSelectFile(file.id) ? <small>Select all associated entries first.</small> : null}</span>
      </label>) : <p className="cleanup-note">No associated files are available for cleanup.</p>}</div> : null}
      {review.entries.some((entry) => !entry.eligible) ? <><h3>Protected entries</h3><div className="cleanup-items">{review.entries.filter((entry) => !entry.eligible).map((entry) => <label className={`cleanup-item ${entry.eligible ? "" : "protected"}`} key={entry.id}>
        <input type="checkbox" aria-label={`Select ${entry.label}`} checked={selected.has(entry.id)} disabled={!entry.eligible || executing} onChange={(event) => toggleEntry(entry.id, event.currentTarget.checked)} />
        <span><strong>{entry.label}</strong><small>{entry.providerName}</small><span>{entry.reason}</span></span>
      </label>)}</div></> : null}
    </div>
    <div className="cleanup-submit"><small>{selected.size} {selected.size === 1 ? "entry" : "entries"} · {includeFiles ? files.size : 0} files selected</small><button className="cleanup-primary" disabled={executing || !selected.size} onClick={() => void useConnLensStore.getState().executeCleanup([...selected], includeFiles ? [...files].filter(canSelectFile) : [])}>{executing ? <LoaderCircle size={14} className="spinning" /> : <Trash2 size={14} />}Remove selected</button><span>{executing ? "Rechecking sources and completing cleanup…" : "Sources are checked again before removal."}</span></div>
  </section>;
}
