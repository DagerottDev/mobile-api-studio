import { invoke } from "../api/invoke";
import { useCallback, useEffect, useMemo, useState } from "react";
import type { AiContextPreview, AiGenerationResult, AiResultRecord, AiSettingsSnapshot } from "../aiTypes";
import type { CaptureSession, TrafficSearchResult } from "../types";

type AiMode = "session" | "flow";

export function AiView() {
  const [mode, setMode] = useState<AiMode>("session");
  const [settings, setSettings] = useState<AiSettingsSnapshot | null>(null);
  const [sessions, setSessions] = useState<CaptureSession[]>([]);
  const [flows, setFlows] = useState<TrafficSearchResult[]>([]);
  const [baselineId, setBaselineId] = useState("");
  const [candidateId, setCandidateId] = useState("");
  const [flowId, setFlowId] = useState("");
  const [preview, setPreview] = useState<AiContextPreview | null>(null);
  const [result, setResult] = useState<AiResultRecord | null>(null);
  const [history, setHistory] = useState<AiResultRecord[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [nextSettings, nextSessions, nextFlows] = await Promise.all([
        invoke<AiSettingsSnapshot>("ai_settings"),
        invoke<CaptureSession[]>("list_sessions"),
        invoke<TrafficSearchResult[]>("search_traffic", {
          query: { text: null, sessionId: null, source: null, method: null, statusClass: null, endpointKey: null, limit: 750 },
        }),
      ]);
      const completed = nextSessions.filter((session) => session.status !== "active");
      setSettings(nextSettings);
      setSessions(completed);
      setFlows(nextFlows);
      setBaselineId((current) => current || completed[1]?.id || completed[0]?.id || "");
      setCandidateId((current) => current || completed[0]?.id || completed[1]?.id || "");
      setFlowId((current) => current || nextFlows[0]?.flow.id || "");
      setError(null);
    } catch (value) {
      setError(formatInvokeError(value));
    }
  }, []);

  useEffect(() => { void refresh(); }, [refresh]);

  const sourceRef = useMemo(() => mode === "session"
    ? (baselineId && candidateId ? `session-diff:${baselineId}:${candidateId}` : "")
    : (flowId ? `flow:${flowId}` : ""), [baselineId, candidateId, flowId, mode]);

  useEffect(() => {
    setPreview(null);
    setResult(null);
  }, [baselineId, candidateId, flowId, mode]);

  useEffect(() => {
    if (!sourceRef) { setHistory([]); return; }
    invoke<AiResultRecord[]>("list_ai_results", { sourceRef, limit: 10 })
      .then(setHistory)
      .catch(() => setHistory([]));
  }, [sourceRef, result?.id]);

  async function previewContext() {
    setBusy(true);
    try {
      const next = mode === "session"
        ? await invoke<AiContextPreview>("preview_session_ai_context", {
            baselineSessionId: baselineId,
            candidateSessionId: candidateId,
          })
        : await invoke<AiContextPreview>("preview_flow_ai_context", { flowId });
      setPreview(next);
      setResult(null);
      setError(null);
    } catch (value) {
      setError(formatInvokeError(value));
    } finally {
      setBusy(false);
    }
  }

  async function askAi() {
    if (!preview) return;
    setBusy(true);
    try {
      const generated = mode === "session"
        ? await invoke<AiGenerationResult>("explain_session_comparison", {
            baselineSessionId: baselineId,
            candidateSessionId: candidateId,
            input: { expectedContextFingerprint: preview.contextFingerprint },
          })
        : await invoke<AiGenerationResult>("diagnose_flow_with_ai", {
            flowId,
            input: { expectedContextFingerprint: preview.contextFingerprint },
          });
      setResult(generated.record);
      setPreview(generated.context);
      setError(null);
    } catch (value) {
      setError(formatInvokeError(value));
      if (value && typeof value === "object" && "code" in value && value.code === "ai_context_changed") setPreview(null);
    } finally {
      setBusy(false);
    }
  }

  const pairInvalid = mode === "session" && (!baselineId || !candidateId || baselineId === candidateId);
  const targetInvalid = mode === "flow" && !flowId;
  const canPreview = !busy && !pairInvalid && !targetInvalid;
  const canSend = !busy && Boolean(preview) && settings?.apiKeyConfigured === true;

  return <div className="ai-page-stack">
    <section className="panel ai-workspace">
      <div className="panel-heading">
        <div><strong>AI debugging</strong><span>Optional explanation over redacted deterministic evidence</span></div>
        <div className="compare-side-toggle"><button className={mode === "session" ? "workspace-tab active" : "workspace-tab"} onClick={() => setMode("session")}>Session diff</button><button className={mode === "flow" ? "workspace-tab active" : "workspace-tab"} onClick={() => setMode("flow")}>Flow diagnosis</button></div>
      </div>
      {error ? <div className="error-banner">{error}</div> : null}
      <div className="ai-target-picker">
        {mode === "session" ? <>
          <label className="field-label">Baseline<select value={baselineId} onChange={(event) => setBaselineId(event.target.value)}><option value="">Choose session</option>{sessions.map((session) => <option key={session.id} value={session.id}>{session.name}{session.appId ? ` · ${session.appId}` : ""}</option>)}</select></label>
          <span className="compare-arrow">→</span>
          <label className="field-label">Candidate<select value={candidateId} onChange={(event) => setCandidateId(event.target.value)}><option value="">Choose session</option>{sessions.map((session) => <option key={session.id} value={session.id}>{session.name}{session.appId ? ` · ${session.appId}` : ""}</option>)}</select></label>
        </> : <label className="field-label ai-flow-picker">Captured flow<select value={flowId} onChange={(event) => setFlowId(event.target.value)}><option value="">Choose flow</option>{flows.map(({ flow, sessionName }) => <option key={flow.id} value={flow.id}>{flow.method} {flow.host}{flow.path} · {flow.statusCode ?? "—"}{sessionName ? ` · ${sessionName}` : ""}</option>)}</select></label>}
        <button className="secondary" disabled={!canPreview} onClick={() => void previewContext()}>{busy ? "Preparing…" : "Preview AI context"}</button>
      </div>
      {pairInvalid && baselineId && candidateId ? <p className="muted-copy ai-inline-note">Choose two different sessions.</p> : null}
      {!settings?.apiKeyConfigured ? <p className="muted-copy ai-inline-note">No API key is configured. You can still inspect the complete redacted context; add a BYOK key in Settings to send it.</p> : null}
    </section>

    {preview ? <section className="panel ai-preview-panel">
      <div className="panel-heading">
        <div><strong>External context preview</strong><span>This exact sanitized payload will be sent if you continue</span></div>
        <div className="ai-preview-stats"><span>{formatBytes(preview.byteCount)}</span><span>{preview.redactionCount} redactions</span>{preview.truncated ? <span>context capped</span> : null}</div>
      </div>
      <div className="ai-fingerprint"><span>SHA-256</span><code>{preview.contextFingerprint}</code></div>
      <pre className="ai-context-preview">{preview.json}</pre>
      <div className="ai-send-row">
        <p className="muted-copy">Sending is always explicit. Mobile API Studio recomputes the redacted context and aborts if its fingerprint differs from this preview.</p>
        <button className="primary" disabled={!canSend} onClick={() => void askAi()}>{busy ? "Asking AI…" : mode === "session" ? "Explain this comparison" : "Diagnose this flow"}</button>
      </div>
    </section> : null}

    {result ? <section className="panel ai-result-panel">
      <div className="panel-heading"><div><strong>AI explanation</strong><span>{result.provider} · {result.model} · {formatTimestamp(result.createdAt)}</span></div><span className="ai-key-pill configured">stored locally</span></div>
      <div className="ai-result-copy">{result.outputText}</div>
    </section> : null}

    {history.length ? <section className="panel ai-history-panel">
      <div className="panel-heading"><div><strong>Local AI history</strong><span>{history.length} recent results for this target</span></div></div>
      <div className="ai-history-list">{history.map((item) => <button key={item.id} onClick={() => setResult(item)}><span><strong>{item.model}</strong><small>{formatTimestamp(item.createdAt)} · {item.contextFingerprint.slice(0, 12)}…</small></span><span>{item.taskKind}</span></button>)}</div>
    </section> : null}
  </div>;
}

function formatBytes(value: number) {
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
  return `${(value / (1024 * 1024)).toFixed(1)} MB`;
}

function formatTimestamp(value: string) {
  const millis = Number(value);
  return Number.isFinite(millis) && millis > 0 ? new Date(millis).toLocaleString() : value;
}

function formatInvokeError(value: unknown) {
  if (typeof value === "string") return value;
  if (value && typeof value === "object" && "message" in value) return String((value as { message: unknown }).message);
  return String(value);
}
