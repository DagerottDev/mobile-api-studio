import { invoke } from "../api/invoke";
import { useCallback, useEffect, useMemo, useState } from "react";
import type {
  AppContextEvidence,
  BodyDifference,
  CallComparison,
  DifferenceKind,
  EndpointComparison,
  HeaderDifference,
  QueryDifference,
  SessionComparison,
  SessionDiagnostics,
} from "../compareTypes";
import type { CaptureSession } from "../types";

export function CompareView() {
  const [sessions, setSessions] = useState<CaptureSession[]>([]);
  const [baselineId, setBaselineId] = useState("");
  const [candidateId, setCandidateId] = useState("");
  const [comparison, setComparison] = useState<SessionComparison | null>(null);
  const [selectedEndpointKey, setSelectedEndpointKey] = useState<string | null>(null);
  const [selectedOccurrence, setSelectedOccurrence] = useState(0);
  const [showChangedOnly, setShowChangedOnly] = useState(true);
  const [diagnosticSide, setDiagnosticSide] = useState<"baseline" | "candidate">("candidate");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refreshSessions = useCallback(async () => {
    try {
      const next = await invoke<CaptureSession[]>("list_sessions");
      const usable = next.filter((session) => session.status !== "active");
      setSessions(usable);
      setBaselineId((current) => current || usable[1]?.id || usable[0]?.id || "");
      setCandidateId((current) => current || usable[0]?.id || usable[1]?.id || "");
      setError(null);
    } catch (value) {
      setError(formatInvokeError(value));
    }
  }, []);

  useEffect(() => { void refreshSessions(); }, [refreshSessions]);

  async function runCompare() {
    if (!baselineId || !candidateId || baselineId === candidateId) return;
    setBusy(true);
    try {
      const result = await invoke<SessionComparison>("compare_sessions", {
        baselineSessionId: baselineId,
        candidateSessionId: candidateId,
      });
      setComparison(result);
      const first = result.endpoints.find(endpointChanged) ?? result.endpoints[0] ?? null;
      setSelectedEndpointKey(first?.endpointKey ?? null);
      setSelectedOccurrence(0);
      setError(null);
    } catch (value) {
      setError(formatInvokeError(value));
    } finally {
      setBusy(false);
    }
  }

  const visibleEndpoints = useMemo(() => {
    if (!comparison) return [];
    const list = showChangedOnly ? comparison.endpoints.filter(endpointChanged) : comparison.endpoints;
    return [...list].sort((left, right) => endpointSeverity(right) - endpointSeverity(left) || left.endpointKey.localeCompare(right.endpointKey));
  }, [comparison, showChangedOnly]);

  useEffect(() => {
    if (!visibleEndpoints.length) {
      setSelectedEndpointKey(null);
      return;
    }
    if (!selectedEndpointKey || !visibleEndpoints.some((endpoint) => endpoint.endpointKey === selectedEndpointKey)) {
      setSelectedEndpointKey(visibleEndpoints[0].endpointKey);
      setSelectedOccurrence(0);
    }
  }, [visibleEndpoints, selectedEndpointKey]);

  const selectedEndpoint = comparison?.endpoints.find((endpoint) => endpoint.endpointKey === selectedEndpointKey) ?? null;
  const selectedCall = selectedEndpoint?.calls[selectedOccurrence] ?? selectedEndpoint?.calls[0] ?? null;
  const diagnostics = comparison ? (diagnosticSide === "baseline" ? comparison.baselineDiagnostics : comparison.candidateDiagnostics) : null;

  return <div className="compare-page-stack">
    <section className="panel compare-picker">
      <div className="panel-heading"><div><strong>Session comparison</strong><span>Deterministic evidence first · baseline → candidate</span></div><button className="secondary compact" onClick={() => void refreshSessions()}>Refresh sessions</button></div>
      {error ? <div className="error-banner">{error}</div> : null}
      <div className="compare-picker-grid">
        <label className="field-label">Baseline<select value={baselineId} onChange={(event) => setBaselineId(event.target.value)}><option value="">Choose session</option>{sessions.map((session) => <option key={session.id} value={session.id}>{session.name}{session.appId ? ` · ${session.appId}` : ""}</option>)}</select></label>
        <span className="compare-arrow">→</span>
        <label className="field-label">Candidate<select value={candidateId} onChange={(event) => setCandidateId(event.target.value)}><option value="">Choose session</option>{sessions.map((session) => <option key={session.id} value={session.id}>{session.name}{session.appId ? ` · ${session.appId}` : ""}</option>)}</select></label>
        <button className="primary" onClick={() => void runCompare()} disabled={busy || !baselineId || !candidateId || baselineId === candidateId}>{busy ? "Comparing…" : "Compare sessions"}</button>
      </div>
      {baselineId && candidateId && baselineId === candidateId ? <p className="muted-copy compare-warning">Baseline and candidate must be different sessions.</p> : null}
    </section>

    {comparison ? <>
      <SummaryCards comparison={comparison} />
      <section className="panel compare-main-layout">
        <aside className="compare-endpoint-pane">
          <div className="panel-heading"><div><strong>Endpoints</strong><span>{visibleEndpoints.length} visible · {comparison.endpoints.length} total</span></div><label className="inline-toggle"><input type="checkbox" checked={showChangedOnly} onChange={(event) => setShowChangedOnly(event.target.checked)} /> Changed only</label></div>
          <div className="compare-endpoint-list">{visibleEndpoints.map((endpoint) => <button key={endpoint.endpointKey} className={selectedEndpointKey === endpoint.endpointKey ? "compare-endpoint-row selected" : "compare-endpoint-row"} onClick={() => { setSelectedEndpointKey(endpoint.endpointKey); setSelectedOccurrence(0); }}><span className={`compare-severity severity-${endpointSeverity(endpoint)}`} /><span><strong>{endpoint.method} {endpoint.pathTemplate}</strong><small>{endpoint.host}</small><small>{endpoint.baselineCount} → {endpoint.candidateCount} calls</small></span><EndpointBadges endpoint={endpoint} /></button>)}</div>
        </aside>
        <div className="compare-detail-pane">
          {selectedEndpoint && selectedCall ? <>
            <div className="compare-detail-heading"><div><span className="eyebrow">{selectedEndpoint.method} · normalized endpoint</span><h2>{selectedEndpoint.host}{selectedEndpoint.pathTemplate}</h2></div><span className={`compare-presence presence-${selectedCall.presence}`}>{presenceLabel(selectedCall.presence)}</span></div>
            <div className="compare-occurrence-tabs">{selectedEndpoint.calls.map((call, index) => <button key={index} className={index === selectedOccurrence ? "workspace-tab active" : "workspace-tab"} onClick={() => setSelectedOccurrence(index)}>Call {index + 1}{call.changed ? " •" : ""}</button>)}</div>
            <CallDetail call={selectedCall} />
          </> : <p className="empty-state">Select an endpoint comparison.</p>}
        </div>
      </section>

      <section className="panel compare-diagnostics">
        <div className="panel-heading"><div><strong>Deterministic diagnostics</strong><span>Retries, slow calls, errors, and waterfall evidence</span></div><div className="compare-side-toggle"><button className={diagnosticSide === "baseline" ? "workspace-tab active" : "workspace-tab"} onClick={() => setDiagnosticSide("baseline")}>Baseline</button><button className={diagnosticSide === "candidate" ? "workspace-tab active" : "workspace-tab"} onClick={() => setDiagnosticSide("candidate")}>Candidate</button></div></div>
        {diagnostics ? <DiagnosticsPanel diagnostics={diagnostics} /> : null}
      </section>
    </> : <section className="panel compare-empty"><h2>Compare two completed capture sessions</h2><p>Mobile API Studio will align normalized endpoints and show missing calls, payload/status changes, JSON schema drift, timing regressions, retries, and SDK context differences.</p></section>}
  </div>;
}

function SummaryCards({ comparison }: { comparison: SessionComparison }) {
  const summary = comparison.summary;
  const cards = [
    ["Endpoints", summary.endpointCount],
    ["Changed calls", summary.changedCalls],
    ["Baseline only", summary.baselineOnlyCalls],
    ["Candidate only", summary.candidateOnlyCalls],
    ["Status changes", summary.statusChanges],
    ["JSON drift", summary.jsonShapeDrifts],
    ["Timing regressions", summary.timingRegressions],
  ];
  return <section className="compare-summary-grid">{cards.map(([label, value]) => <div className="panel compare-summary-card" key={String(label)}><span>{label}</span><strong>{value}</strong></div>)}</section>;
}

function EndpointBadges({ endpoint }: { endpoint: EndpointComparison }) {
  const missingBaseline = endpoint.calls.filter((call) => call.presence === "candidate_only").length;
  const missingCandidate = endpoint.calls.filter((call) => call.presence === "baseline_only").length;
  const changed = endpoint.calls.filter((call) => call.presence === "both" && call.changed).length;
  return <span className="compare-badges">{missingCandidate ? <b className="badge-removed">-{missingCandidate}</b> : null}{missingBaseline ? <b className="badge-added">+{missingBaseline}</b> : null}{changed ? <b className="badge-changed">Δ{changed}</b> : null}</span>;
}

function CallDetail({ call }: { call: CallComparison }) {
  if (call.presence !== "both") {
    const context = call.presence === "baseline_only" ? call.baselineContext : call.candidateContext;
    return <div className="compare-single-call"><h3>{call.presence === "baseline_only" ? "Missing from candidate" : "New in candidate"}</h3><p className="muted-copy">This normalized endpoint occurrence exists on only one side of the comparison.</p><ContextDiff baseline={call.presence === "baseline_only" ? context : null} candidate={call.presence === "candidate_only" ? context : null} /></div>;
  }
  return <div className="compare-call-sections">
    <ContextDiff baseline={call.baselineContext} candidate={call.candidateContext} />
    {call.request ? <section className="compare-section"><h3>Request</h3><ScalarDiffRow label="Method" diff={call.request.method} /><NamedDiffTable title="Query" rows={call.request.query} /><NamedDiffTable title="Headers" rows={call.request.headers} /><BodyDiff title="Request body" body={call.request.body} /></section> : null}
    {call.response ? <section className="compare-section"><h3>Response</h3><ScalarDiffRow label="Status" diff={call.response.status} /><NamedDiffTable title="Headers" rows={call.response.headers} /><BodyDiff title="Response body" body={call.response.body} /></section> : null}
    <section className="compare-section"><h3>Timing & size</h3><div className="compare-metric-grid"><Metric label="Baseline" value={formatMs(call.timing.baselineTotalMs)} /><Metric label="Candidate" value={formatMs(call.timing.candidateTotalMs)} /><Metric label="Delta" value={formatSignedMs(call.timing.deltaMs)} emphasized={Boolean(call.timing.deltaMs)} /><Metric label="Delta %" value={call.timing.deltaPercent == null ? "—" : `${call.timing.deltaPercent > 0 ? "+" : ""}${call.timing.deltaPercent}%`} emphasized={Boolean(call.timing.deltaPercent)} /><Metric label="Baseline size" value={formatBytes(call.timing.baselineSizeBytes)} /><Metric label="Candidate size" value={formatBytes(call.timing.candidateSizeBytes)} /></div></section>
  </div>;
}

function ContextDiff({ baseline, candidate }: { baseline: AppContextEvidence | null; candidate: AppContextEvidence | null }) {
  if (!baseline && !candidate) return <section className="compare-section"><h3>App context</h3><p className="muted-copy">No SDK context on either call.</p></section>;
  const rows: Array<[string, string | null, string | null]> = [
    ["App", baseline?.appName ?? baseline?.appId ?? null, candidate?.appName ?? candidate?.appId ?? null],
    ["Platform", baseline?.platform ?? null, candidate?.platform ?? null],
    ["Screen", baseline?.screen ?? null, candidate?.screen ?? null],
    ["Feature", baseline?.feature ?? null, candidate?.feature ?? null],
    ["Source", formatSource(baseline), formatSource(candidate)],
    ["Function", baseline?.sourceFunction ?? null, candidate?.sourceFunction ?? null],
  ];
  return <section className="compare-section"><h3>App context</h3><div className="compare-two-column-table"><div className="compare-table-header"><span>Field</span><span>Baseline</span><span>Candidate</span></div>{rows.map(([label, left, right]) => <div className={left === right ? "compare-table-row" : "compare-table-row changed"} key={label}><strong>{label}</strong><span>{left ?? "—"}</span><span>{right ?? "—"}</span></div>)}</div></section>;
}

function ScalarDiffRow({ label, diff }: { label: string; diff: { kind: DifferenceKind; baseline: string | null; candidate: string | null } }) {
  return <div className={`compare-scalar-row diff-${diff.kind}`}><strong>{label}</strong><span>{diff.baseline ?? "—"}</span><span>→</span><span>{diff.candidate ?? "—"}</span></div>;
}

type NamedRow = QueryDifference | HeaderDifference;
function NamedDiffTable({ title, rows }: { title: string; rows: NamedRow[] }) {
  if (!rows.length) return <div className="compare-unchanged"><strong>{title}</strong><span>No differences</span></div>;
  return <div className="compare-named-diff"><strong>{title}</strong><div className="compare-two-column-table"><div className="compare-table-header"><span>Name</span><span>Baseline</span><span>Candidate</span></div>{rows.map((row) => <div className={`compare-table-row diff-${row.kind}`} key={row.name}><strong>{row.name}</strong><span>{row.baseline.join("\n") || "—"}</span><span>{row.candidate.join("\n") || "—"}</span></div>)}</div></div>;
}

function BodyDiff({ title, body }: { title: string; body: BodyDifference }) {
  const shape = body.jsonShape;
  return <div className={`compare-body-diff diff-${body.kind}`}><div className="compare-body-heading"><strong>{title}</strong><span>{body.kind}</span></div><div className="compare-body-meta"><span>{body.baselineContentType ?? "—"} · {formatBytes(body.baselineByteSize)}</span><span>{body.candidateContentType ?? "—"} · {formatBytes(body.candidateByteSize)}</span></div>{shape ? <div className="json-shape-diff"><strong>JSON shape drift</strong>{shape.addedPaths.length ? <p><b>Added:</b> {shape.addedPaths.slice(0, 30).join(", ")}</p> : null}{shape.removedPaths.length ? <p><b>Removed:</b> {shape.removedPaths.slice(0, 30).join(", ")}</p> : null}{shape.typeChanges.length ? <p><b>Type:</b> {shape.typeChanges.slice(0, 30).map((change) => `${change.pointer} ${change.baselineType}→${change.candidateType}`).join(", ")}</p> : null}</div> : null}{body.kind !== "same" ? <div className="compare-body-previews"><pre>{body.baselinePreview ?? "No text preview"}</pre><pre>{body.candidatePreview ?? "No text preview"}</pre></div> : <p className="muted-copy">Body content is unchanged.</p>}</div>;
}

function DiagnosticsPanel({ diagnostics }: { diagnostics: SessionDiagnostics }) {
  const overlapGroups = diagnostics.waterfallGroups.filter((group) => group.kind === "overlap");
  return <div className="diagnostic-grid">
    <section><h3>Duplicate / retry candidates</h3>{diagnostics.duplicates.slice(0, 12).map((item) => <div className="diagnostic-row" key={item.endpointKey}><span><strong>{item.endpointKey}</strong><small>{item.callCount} calls</small></span><b>{item.likelyRetryCount} likely retries</b></div>)}{!diagnostics.duplicates.length ? <p className="muted-copy">No repeated endpoints.</p> : null}</section>
    <section><h3>Slowest requests</h3>{diagnostics.slowest.slice(0, 12).map((item) => <div className="diagnostic-row" key={item.flowId}><span><strong>{item.endpointKey}</strong><small>HTTP {item.statusCode ?? "—"}</small></span><b>{item.totalMs} ms</b></div>)}</section>
    <section><h3>Error clusters</h3>{diagnostics.errors.slice(0, 12).map((item) => <div className="diagnostic-row" key={item.key}><span><strong>{item.endpointKey}</strong><small>{item.errorCode ?? `HTTP ${item.statusCode ?? "error"}`}</small></span><b>{item.count}</b></div>)}{!diagnostics.errors.length ? <p className="muted-copy">No HTTP/error clusters.</p> : null}</section>
    <section><h3>Waterfall overlap</h3><div className="diagnostic-stat"><strong>{overlapGroups.length}</strong><span>overlapping groups</span></div><div className="diagnostic-stat"><strong>{diagnostics.waterfallGroups.length - overlapGroups.length}</strong><span>sequential/single groups</span></div>{overlapGroups.slice(0, 6).map((group, index) => <p className="muted-copy" key={index}>{group.flowIds.length} calls overlap across {group.endedAtMs - group.startedAtMs} ms</p>)}</section>
  </div>;
}

function Metric({ label, value, emphasized = false }: { label: string; value: string; emphasized?: boolean }) { return <span className={emphasized ? "compare-metric emphasized" : "compare-metric"}><small>{label}</small><strong>{value}</strong></span>; }
function endpointChanged(endpoint: EndpointComparison) { return endpoint.baselineCount !== endpoint.candidateCount || endpoint.calls.some((call) => call.changed); }
function endpointSeverity(endpoint: EndpointComparison) { if (endpoint.calls.some((call) => call.presence !== "both")) return 3; if (endpoint.calls.some((call) => call.response?.status.kind !== "same" || call.response?.body.jsonShape)) return 2; if (endpoint.calls.some((call) => call.changed)) return 1; return 0; }
function presenceLabel(value: CallComparison["presence"]) { return value === "both" ? "matched" : value === "baseline_only" ? "baseline only" : "candidate only"; }
function formatMs(value: number | null) { return value == null ? "—" : `${value} ms`; }
function formatSignedMs(value: number | null) { return value == null ? "—" : `${value > 0 ? "+" : ""}${value} ms`; }
function formatBytes(value: number | null) { if (value == null) return "—"; if (value < 1024) return `${value} B`; if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`; return `${(value / (1024 * 1024)).toFixed(1)} MB`; }
function formatSource(context: AppContextEvidence | null) { if (!context?.sourceFile) return null; return `${context.sourceFile}${context.sourceLine ? `:${context.sourceLine}` : ""}`; }
function formatInvokeError(value: unknown) { if (typeof value === "string") return value; if (value && typeof value === "object" && "message" in value) return String((value as { message: unknown }).message); return String(value); }
