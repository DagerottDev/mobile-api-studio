import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useState } from "react";
import type {
  FlowDetail,
  FlowSummary,
  ReplayDraft,
  ReplayHeaderDraft,
} from "../types";

export function ReplayView() {
  const [flows, setFlows] = useState<FlowSummary[]>([]);
  const [sourceFlowId, setSourceFlowId] = useState<string>("");
  const [draft, setDraft] = useState<ReplayDraft | null>(null);
  const [bodyEdited, setBodyEdited] = useState(false);
  const [sending, setSending] = useState(false);
  const [loadingDraft, setLoadingDraft] = useState(false);
  const [result, setResult] = useState<FlowDetail | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<FlowSummary[]>("list_flows")
      .then((items) => {
        setFlows(items);
        setSourceFlowId((current) => current || items[0]?.id || "");
      })
      .catch((value) => setError(formatInvokeError(value)));
  }, []);

  useEffect(() => {
    if (!sourceFlowId) {
      setDraft(null);
      return;
    }

    let cancelled = false;
    setLoadingDraft(true);
    invoke<ReplayDraft>("create_replay_draft", { flowId: sourceFlowId })
      .then((nextDraft) => {
        if (cancelled) return;
        setDraft(nextDraft);
        setBodyEdited(!nextDraft.body?.sourceTruncated);
        setResult(null);
        setError(null);
      })
      .catch((value) => {
        if (!cancelled) {
          setDraft(null);
          setError(formatInvokeError(value));
        }
      })
      .finally(() => {
        if (!cancelled) setLoadingDraft(false);
      });

    return () => {
      cancelled = true;
    };
  }, [sourceFlowId]);

  const sourceOptions = useMemo(
    () => flows.map((flow) => ({
      id: flow.id,
      label: `${flow.method} ${flow.host}${flow.path} · ${flow.statusCode ?? "—"}`,
    })),
    [flows],
  );

  const truncatedBodyBlocked = Boolean(draft?.body?.sourceTruncated && !bodyEdited);

  function updateHeader(index: number, patch: Partial<ReplayHeaderDraft>) {
    setDraft((current) => {
      if (!current) return current;
      return {
        ...current,
        headers: current.headers.map((header, headerIndex) =>
          headerIndex === index ? { ...header, ...patch } : header,
        ),
      };
    });
  }

  function replaceSensitiveHeader(index: number) {
    updateHeader(index, { useOriginal: false, value: "", sensitive: true });
  }

  function addHeader() {
    setDraft((current) => {
      if (!current) return current;
      return {
        ...current,
        headers: [
          ...current.headers,
          {
            name: "",
            value: "",
            sensitive: false,
            useOriginal: false,
            enabled: true,
            sourceIndex: null,
          },
        ],
      };
    });
  }

  function removeHeader(index: number) {
    setDraft((current) => {
      if (!current) return current;
      return {
        ...current,
        headers: current.headers.filter((_, headerIndex) => headerIndex !== index),
      };
    });
  }

  function updateBody(value: string) {
    setBodyEdited(true);
    setDraft((current) => {
      if (!current?.body) return current;
      return {
        ...current,
        body: current.body.isBinary
          ? { ...current.body, base64: value, useOriginal: false }
          : { ...current.body, text: value, useOriginal: false },
      };
    });
  }

  async function sendReplay() {
    if (!draft || truncatedBodyBlocked) return;
    setSending(true);
    try {
      const replayed = await invoke<FlowDetail>("send_replay", { draft });
      setResult(replayed);
      setError(null);
      setFlows(await invoke<FlowSummary[]>("list_flows"));
    } catch (value) {
      setError(formatInvokeError(value));
    } finally {
      setSending(false);
    }
  }

  return (
    <section className="replay-layout">
      <div className="panel replay-source-panel">
        <div className="panel-heading">
          <div>
            <strong>Source request</strong>
            <span>Start from any flow with captured request details</span>
          </div>
        </div>
        <div className="replay-source-content">
          <label className="field-label" htmlFor="replay-source">Captured flow</label>
          <select
            id="replay-source"
            className="replay-select"
            value={sourceFlowId}
            onChange={(event) => setSourceFlowId(event.target.value)}
          >
            {sourceOptions.map((option) => (
              <option key={option.id} value={option.id}>{option.label}</option>
            ))}
          </select>
          {flows.length === 0 ? <p className="muted-copy">Capture a request before using Replay.</p> : null}
          {loadingDraft ? <p className="muted-copy">Loading replay draft…</p> : null}
          {error ? <div className="error-banner replay-error">{error}</div> : null}
        </div>
      </div>

      <div className="panel replay-editor-panel">
        <div className="panel-heading">
          <div>
            <strong>Request editor</strong>
            <span>Sensitive stored values remain backend-only</span>
          </div>
          <button
            className="primary"
            onClick={() => void sendReplay()}
            disabled={!draft || sending || truncatedBodyBlocked}
          >
            {sending ? "Sending…" : "Send request"}
          </button>
        </div>

        {draft ? (
          <div className="replay-editor-scroll">
            <section className="replay-section replay-request-line">
              <input
                className="replay-method-input"
                value={draft.method}
                onChange={(event) => setDraft({ ...draft, method: event.target.value })}
                aria-label="HTTP method"
              />
              <input
                className="text-input replay-url-input"
                value={draft.url}
                onChange={(event) => setDraft({ ...draft, url: event.target.value })}
                aria-label="Request URL"
              />
            </section>

            <section className="replay-section">
              <div className="replay-section-heading">
                <div>
                  <h3>Headers</h3>
                  <span>Disabled rows are not sent.</span>
                </div>
                <button className="secondary compact" onClick={addHeader}>Add header</button>
              </div>

              <div className="replay-header-list">
                {draft.headers.map((header, index) => (
                  <div className="replay-header-row" key={`${header.sourceIndex ?? "new"}-${index}`}>
                    <input
                      type="checkbox"
                      checked={header.enabled}
                      onChange={(event) => updateHeader(index, { enabled: event.target.checked })}
                      aria-label={`Enable ${header.name || "header"}`}
                    />
                    <input
                      className="replay-header-input"
                      value={header.name}
                      disabled={header.useOriginal}
                      onChange={(event) => updateHeader(index, { name: event.target.value })}
                      placeholder="Header name"
                    />
                    {header.useOriginal ? (
                      <div className="stored-secret">
                        <span>Stored value</span>
                        <button className="secondary compact" onClick={() => replaceSensitiveHeader(index)}>
                          Replace
                        </button>
                      </div>
                    ) : (
                      <input
                        className="replay-header-input"
                        value={header.value ?? ""}
                        onChange={(event) => updateHeader(index, { value: event.target.value })}
                        placeholder="Header value"
                      />
                    )}
                    <button className="icon-button" onClick={() => removeHeader(index)} aria-label="Remove header">
                      ×
                    </button>
                  </div>
                ))}
              </div>
            </section>

            <section className="replay-section">
              <div className="replay-section-heading">
                <div>
                  <h3>Body</h3>
                  <span>{draft.body?.contentType ?? "No content type captured"}</span>
                </div>
                {draft.body?.useOriginal ? <span className="replay-badge">Using captured body</span> : null}
              </div>

              {draft.body ? (
                <>
                  {draft.body.sourceTruncated && !bodyEdited ? (
                    <div className="warning-banner">
                      The captured request body was truncated. Edit or replace the body before replaying.
                    </div>
                  ) : null}
                  <textarea
                    className="replay-body-editor"
                    value={draft.body.isBinary ? draft.body.base64 ?? "" : draft.body.text ?? ""}
                    onChange={(event) => updateBody(event.target.value)}
                    spellCheck={false}
                  />
                  {draft.body.isBinary ? <small className="muted-copy">Binary request body is edited as base64.</small> : null}
                </>
              ) : (
                <p className="muted-copy">This request has no body.</p>
              )}
            </section>

            {result ? (
              <section className="replay-section replay-result">
                <div className="replay-section-heading">
                  <div>
                    <h3>Replay result</h3>
                    <span>Persisted as {result.summary.id}</span>
                  </div>
                  <span className="replay-status">{result.response?.statusCode ?? "—"}</span>
                </div>
                <div className="replay-result-grid">
                  <span>Duration <strong>{result.timing.totalMs ?? "—"} ms</strong></span>
                  <span>Host <strong>{result.summary.host}</strong></span>
                  <span>Path <strong>{result.summary.path}</strong></span>
                  <span>Source <strong>{result.summary.source}</strong></span>
                </div>
              </section>
            ) : null}
          </div>
        ) : (
          <p className="empty-state">Choose a captured flow with full request details to create a replay draft.</p>
        )}
      </div>
    </section>
  );
}

function formatInvokeError(value: unknown) {
  if (typeof value === "string") return value;
  if (value && typeof value === "object" && "message" in value) {
    return String((value as { message: unknown }).message);
  }
  return String(value);
}
