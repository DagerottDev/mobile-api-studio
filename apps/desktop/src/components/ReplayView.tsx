import { invoke } from "../api/invoke";
import { useEffect, useState } from "react";
import type {
  FlowDetail,
  FlowSummary,
  ReplayDraft,
  ReplayHeaderDraft,
  SavedRequest,
} from "../types";

interface ReplayViewProps {
  savedRequestId?: string | null;
}

export function ReplayView({ savedRequestId = null }: ReplayViewProps) {
  const [flows, setFlows] = useState<FlowSummary[]>([]);
  const [savedRequests, setSavedRequests] = useState<SavedRequest[]>([]);
  const [sourceId, setSourceId] = useState<string>("");
  const [draft, setDraft] = useState<ReplayDraft | null>(null);
  const [bodyEdited, setBodyEdited] = useState(false);
  const [sending, setSending] = useState(false);
  const [loadingDraft, setLoadingDraft] = useState(false);
  const [result, setResult] = useState<FlowDetail | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    Promise.all([
      invoke<FlowSummary[]>("list_flows"),
      invoke<SavedRequest[]>("list_saved_requests", { collectionId: null }),
    ])
      .then(([flowItems, requestItems]) => {
        setFlows(flowItems);
        setSavedRequests(requestItems);
        setSourceId((current) => {
          if (savedRequestId && requestItems.some((item) => item.id === savedRequestId)) {
            return `saved:${savedRequestId}`;
          }
          if (current) return current;
          return flowItems[0]?.id ?? (requestItems[0] ? `saved:${requestItems[0].id}` : "");
        });
      })
      .catch((value) => setError(formatInvokeError(value)));
  }, [savedRequestId]);

  useEffect(() => {
    if (savedRequestId && savedRequests.some((item) => item.id === savedRequestId)) {
      setSourceId(`saved:${savedRequestId}`);
    }
  }, [savedRequestId, savedRequests]);

  useEffect(() => {
    if (!sourceId) {
      setDraft(null);
      return;
    }

    let cancelled = false;
    setLoadingDraft(true);
    invoke<ReplayDraft>("create_replay_draft", { flowId: sourceId })
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
  }, [sourceId]);

  const truncatedBodyBlocked = Boolean(draft?.body?.sourceTruncated && !bodyEdited);
  const isSavedSource = sourceId.startsWith("saved:");

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

  function addBody() {
    setBodyEdited(true);
    setDraft((current) => current ? {
      ...current,
      body: {
        text: "",
        base64: null,
        isBinary: false,
        contentType: "application/json",
        useOriginal: false,
        sourceTruncated: false,
      },
    } : current);
  }

  function removeBody() {
    setBodyEdited(true);
    setDraft((current) => current ? { ...current, body: null } : current);
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
            <span>Start from captured traffic or a saved collection request</span>
          </div>
        </div>
        <div className="replay-source-content">
          <label className="field-label" htmlFor="replay-source">Request source</label>
          <select
            id="replay-source"
            className="replay-select"
            value={sourceId}
            onChange={(event) => setSourceId(event.target.value)}
          >
            {flows.length > 0 ? (
              <optgroup label="Captured traffic">
                {flows.map((flow) => (
                  <option key={flow.id} value={flow.id}>
                    {flow.method} {flow.host}{flow.path} · {flow.statusCode ?? "—"}
                  </option>
                ))}
              </optgroup>
            ) : null}
            {savedRequests.length > 0 ? (
              <optgroup label="Saved requests">
                {savedRequests.map((request) => (
                  <option key={request.id} value={`saved:${request.id}`}>
                    {request.name} · {request.method} {request.url}
                  </option>
                ))}
              </optgroup>
            ) : null}
          </select>
          {flows.length === 0 && savedRequests.length === 0 ? (
            <p className="muted-copy">Capture or save a request before using Replay.</p>
          ) : null}
          <p className="muted-copy">
            {isSavedSource
              ? "Saved-request templates can use {{variables}} from the active environment."
              : "You can add {{variables}} to URL, headers, or text bodies before sending."}
          </p>
          {loadingDraft ? <p className="muted-copy">Loading replay draft…</p> : null}
          {error ? <div className="error-banner replay-error">{error}</div> : null}
        </div>
      </div>

      <div className="panel replay-editor-panel">
        <div className="panel-heading">
          <div>
            <strong>Request editor</strong>
            <span>Sensitive stored values stay backend-only; active environment resolves on send</span>
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
                  <span>Disabled rows are not sent. Environment placeholders resolve immediately before execution.</span>
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
                  <span>{draft.body?.contentType ?? "No request body"}</span>
                </div>
                <div className="replay-heading-actions">
                  {draft.body?.useOriginal ? <span className="replay-badge">Using stored body</span> : null}
                  {draft.body ? (
                    <button className="secondary compact" onClick={removeBody}>Remove body</button>
                  ) : (
                    <button className="secondary compact" onClick={addBody}>Add body</button>
                  )}
                </div>
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
                  {draft.body.isBinary ? (
                    <small className="muted-copy">Binary request body is edited as base64 and is not environment-interpolated.</small>
                  ) : null}
                </>
              ) : (
                <p className="muted-copy">This request has no body. Add one if the edited request needs it.</p>
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
          <p className="empty-state">Choose a captured or saved request to create a replay draft.</p>
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
