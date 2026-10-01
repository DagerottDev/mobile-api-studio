import { invoke } from "../api/invoke";
import { useEffect, useState } from "react";
import type {
  FlowDetail,
  FlowSummary,
  ReplayDraft,
  ReplayHeaderDraft,
  SavedRequest,
  SavedCollection,
  PortableWorkspaceBundle,
} from "../types";

interface ReplayViewProps {
  savedRequestId?: string | null;
}

export function ReplayView({ savedRequestId = null }: ReplayViewProps) {
  const [flows, setFlows] = useState<FlowSummary[]>([]);
  const [savedRequests, setSavedRequests] = useState<SavedRequest[]>([]);
  const [sourceId, setSourceId] = useState<string>("draft:blank");
  const [draft, setDraft] = useState<ReplayDraft | null>(null);
  const [bodyEdited, setBodyEdited] = useState(false);
  const [sending, setSending] = useState(false);
  const [loadingDraft, setLoadingDraft] = useState(false);
  const [result, setResult] = useState<FlowDetail | null>(null);
  const [collections, setCollections] = useState<SavedCollection[]>([]);
  const [collectionId, setCollectionId] = useState("");
  const [saveName, setSaveName] = useState("Composed request");
  const [bodyMode, setBodyMode] = useState("raw");
  const [fields, setFields] = useState("");
  const [bodyError, setBodyError] = useState<string | null>(null);
  const [format, setFormat] = useState("curl");
  const [importText, setImportText] = useState("");
  const [preview, setPreview] = useState<{ requests: ReplayDraft[]; bundle: PortableWorkspaceBundle | null; warnings: string[] } | null>(null);
  const [exportPreview, setExportPreview] = useState("");
  const [repeatCount, setRepeatCount] = useState(1);
  const [intervalMs, setIntervalMs] = useState(0);
  const [concurrency, setConcurrency] = useState(1);
  const [outcomes, setOutcomes] = useState<{ index: number; detail: FlowDetail | null; error: { message: string } | null }[]>([]);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void invoke<SavedCollection[]>("list_collections").then((items) => { setCollections(items); setCollectionId(items[0]?.id ?? ""); }).catch((value) => setError(formatInvokeError(value)));
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
    if (sourceId === "draft:import") return;
    if (!sourceId) {
      setDraft(null);
      return;
    }

    let cancelled = false;
    setLoadingDraft(true);
    invoke<ReplayDraft>("create_replay_draft", { flowId: sourceId })
      .then((nextDraft) => {
        if (cancelled) return;
        setBodyError(null); setDraft(nextDraft); setBodyMode(nextDraft.body?.isBinary ? "binary" : "raw"); setFields("");
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
    setBodyEdited(true); setBodyError(null);
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
    setBodyEdited(true); setBodyError(null);
    setDraft((current) => current ? { ...current, body: null } : current);
  }

  function updateBody(value: string) {
    setBodyEdited(true); setBodyError(null);
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
    if (!draft || truncatedBodyBlocked || bodyError) return;
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

  async function previewImport() {
    setSending(true);
    try { const next = await invoke<{ requests: ReplayDraft[]; bundle: PortableWorkspaceBundle | null; warnings: string[] }>("preview_interchange", { format, text: importText }); setPreview(next); setError(null); }
    catch (value) { setPreview(null); setError(formatInvokeError(value)); }
    finally { setSending(false); }
  }

  function chooseImported(item: ReplayDraft) {
    setLoadingDraft(false); setBodyError(null); setFields(""); setSourceId("draft:import"); setDraft(item); setBodyMode(item.body?.isBinary ? "binary" : "raw"); setBodyEdited(true); setBodyError(null); setResult(null); setError(null);
  }

  async function importTraffic() {
    if (!preview?.bundle) return;
    setSending(true);
    try { await invoke("import_workspace", { bundle: preview.bundle, mode: "merge" }); setMessage("Imported the previewed traffic. Imported rules/profiles remain disabled."); setFlows(await invoke<FlowSummary[]>("list_flows")); setError(null); }
    catch (value) { setError(formatInvokeError(value)); }
    finally { setSending(false); }
  }

  async function saveDraft() {
    if (!draft || !collectionId || bodyError) return;
    setSending(true);
    try { await invoke("save_composed_request", { draft, collectionId, name: saveName }); setSavedRequests(await invoke<SavedRequest[]>("list_saved_requests", { collectionId: null })); setMessage("Request saved to collection."); setError(null); }
    catch (value) { setError(formatInvokeError(value)); }
    finally { setSending(false); }
  }

  async function repeat() {
    if (!draft || truncatedBodyBlocked || bodyError) return;
    setSending(true); setOutcomes([]);
    try {
      const run = await invoke<{ outcomes: { index: number; detail: FlowDetail | null; error: { message: string } | null }[]; timedOut: boolean }>("repeat_replay", { draft, count: repeatCount, intervalMs, concurrency });
      setOutcomes(run.outcomes); setMessage(run.timedOut ? "Repeat stopped at its five-minute time limit." : "Repeat finished; executed requests are recorded individually."); setError(null);
      setFlows(await invoke<FlowSummary[]>("list_flows"));
    } catch (value) { setError(formatInvokeError(value)); }
    finally { setSending(false); }
  }

  async function exportSource() {
    setSending(true); setExportPreview("");
    try {
      const value = await invoke<string>("export_interchange", { format, flowIds: sourceId && !sourceId.startsWith("saved:") && !sourceId.startsWith("draft:") ? [sourceId] : [], savedRequestIds: sourceId.startsWith("saved:") ? [sourceId.slice(6)] : [] });
      setExportPreview(value); setError(null);
    } catch (value) { setError(formatInvokeError(value)); }
    finally { setSending(false); }
  }

  function downloadPreview() {
    const url = URL.createObjectURL(new Blob([exportPreview], { type: "text/plain" }));
    const link = document.createElement("a"); link.href = url; link.download = `mobile-api-studio.${format === "har" ? "har" : format === "postman" ? "json" : format === "csv" ? "csv" : "txt"}`; link.click(); URL.revokeObjectURL(url);
  }

  function structuredBody(value: string, mode = bodyMode) {
    setFields(value); setBodyEdited(true); setBodyError(null);
    const pairs = value.split("\n").filter(Boolean).map((line) => { const index = line.indexOf("="); return [index < 0 ? line : line.slice(0, index), index < 0 ? "" : line.slice(index + 1)] as [string, string]; });
    const boundary = "mas-composed-boundary";
    if (pairs.some(([name]) => /[\r\n"]/.test(name)) || (mode === "multipart" && value.includes(`--${boundary}`))) { setBodyError("Field names cannot contain quotes or line breaks, and values cannot contain the multipart boundary."); return; }
    const text = mode === "form" ? new URLSearchParams(pairs).toString() : pairs.map(([name, content]) => `--${boundary}\r\nContent-Disposition: form-data; name="${name}"\r\n\r\n${content}\r\n`).join("") + `--${boundary}--\r\n`;
    setDraft((current) => current ? { ...current, body: { text, base64: null, isBinary: false, contentType: mode === "form" ? "application/x-www-form-urlencoded" : `multipart/form-data; boundary=${boundary}`, useOriginal: false, sourceTruncated: false }, headers: current.headers.filter((header) => header.name.toLowerCase() !== "content-type") } : current);
  }

  function changeBodyMode(mode: string) {
    setBodyMode(mode); setFields(""); setBodyEdited(true); setBodyError(null);
    if (mode === "form" || mode === "multipart") { structuredBody("", mode); return; }
    setDraft((current) => current ? { ...current, body: { text: mode === "binary" ? null : "", base64: mode === "binary" ? "" : null, isBinary: mode === "binary", contentType: mode === "json" ? "application/json" : mode === "binary" ? "application/octet-stream" : "text/plain", useOriginal: false, sourceTruncated: false }, headers: current.headers.filter((header) => header.name.toLowerCase() !== "content-type") } : current);
  }

  async function binaryFile(file: File | undefined) {
    if (!file) return;
    if (file.size > 2 * 1024 * 1024) { setError("Binary request body must be 2 MiB or smaller."); return; }
    const bytes = new Uint8Array(await file.arrayBuffer());
    let text = ""; for (let offset = 0; offset < bytes.length; offset += 8192) text += String.fromCharCode(...bytes.subarray(offset, offset + 8192));
    updateBody(btoa(text));
  }

  return (
    <section className="replay-layout">
      <div className="panel replay-source-panel">
        <div className="panel-heading">
          <div>
            <strong>Source request</strong>
            <span>Start blank, import a request, or edit captured and saved traffic</span>
          </div>
        </div>
        <div className="replay-source-content">
          <label className="field-label" htmlFor="replay-source">Request source</label>
          <select
            id="replay-source"
            className="replay-select"
            value={sourceId}
            disabled={sending}
            onChange={(event) => setSourceId(event.target.value)}
          >
            <option value="draft:blank">Blank request</option>
            {sourceId === "draft:import" ? <option value="draft:import">Imported draft</option> : null}
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
            <p className="muted-copy">Compose a blank request or preview an imported request.</p>
          ) : null}
          <p className="muted-copy">
            {isSavedSource
              ? "Saved-request templates can use {{variables}} from the active environment."
              : "You can add {{variables}} to URL, headers, or text bodies before sending."}
          </p>
          <label className="field-label">Interchange format<select className="text-input" value={format} onChange={(event) => { setFormat(event.target.value); setPreview(null); setExportPreview(""); }}><option value="curl">cURL</option><option value="har">HAR 1.2</option><option value="postman">Postman v2.1 JSON</option><option value="csv">CSV</option></select></label>
          <label className="field-label">Import file (16 MiB max)<input type="file" disabled={sending} onChange={(event) => { const file = event.currentTarget.files?.[0]; event.currentTarget.value = ""; if (file && file.size <= 16 * 1024 * 1024) void file.text().then((text) => { setImportText(text); setPreview(null); }); else if (file) setError("Import must be 16 MiB or smaller."); }} /></label>
          <label className="field-label">Import text<textarea className="replay-body-editor" value={importText} onChange={(event) => { setImportText(event.target.value); setPreview(null); }} /></label>
          <button className="secondary compact" disabled={sending || !importText} onClick={() => void previewImport()}>Preview import</button>
          {preview ? <div><p>{preview.requests.length} request drafts; review before saving or sending.</p>{preview.warnings.map((warning, index) => <p role="status" key={index}>{warning}</p>)}{preview.requests.map((item, index) => <button className="secondary compact" key={index} disabled={sending} onClick={() => chooseImported(item)}>{item.method} {item.url}</button>)}{preview.bundle ? <><p>Traffic preview shows the first 100,000 characters. All previewed entries are included when importing.</p><pre>{JSON.stringify(preview.bundle, null, 2).slice(0, 100000)}</pre><button className="secondary compact" disabled={sending} onClick={() => void importTraffic()}>Import previewed traffic</button></> : null}</div> : null}
          <button className="secondary compact" disabled={sending || sourceId.startsWith("draft:")} onClick={() => void exportSource()}>Preview selected source export</button>
          {exportPreview ? <><p>Known secret headers are redacted. Captured bodies can contain application data; review this export before downloading.</p><textarea className="replay-body-editor" readOnly value={exportPreview} /><button className="secondary compact" onClick={downloadPreview}>Download previewed export</button></> : null}
          {message ? <p role="status">{message}</p> : null}
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
            disabled={!draft || sending || truncatedBodyBlocked || Boolean(bodyError)}
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
                  <label className="field-label">Body format<select className="text-input" value={bodyMode} onChange={(event) => changeBodyMode(event.target.value)}><option value="raw">Raw text</option><option value="json">JSON</option><option value="form">Form fields</option><option value="multipart">Multipart fields</option><option value="binary">Binary/base64</option></select></label>
                  <label className="field-label">Content type<input className="text-input" value={draft.body.contentType ?? ""} onChange={(event) => setDraft({ ...draft, body: draft.body ? { ...draft.body, contentType: event.target.value, useOriginal: false } : null })} /></label>
                  {bodyMode === "form" || bodyMode === "multipart" ? <label className="field-label">Fields (one name=value per line)<textarea className="replay-body-editor" value={fields} onChange={(event) => structuredBody(event.target.value)} /></label> : null}
                  {bodyMode === "binary" ? <label className="field-label">Binary body file<input type="file" onChange={(event) => { const file = event.currentTarget.files?.[0]; event.currentTarget.value = ""; void binaryFile(file).catch((value) => setError(formatInvokeError(value))); }} /></label> : null}
                  {bodyError ? <div role="alert" className="error-banner">{bodyError}</div> : null}
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

            <section className="replay-section"><h3>Save composed request</h3><label className="field-label">Collection<select className="text-input" value={collectionId} onChange={(event) => setCollectionId(event.target.value)}>{collections.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}</select></label><label className="field-label">Saved name<input className="text-input" value={saveName} onChange={(event) => setSaveName(event.target.value)} /></label><button className="secondary compact" disabled={sending || !collectionId || truncatedBodyBlocked || Boolean(bodyError)} onClick={() => void saveDraft()}>Save draft</button>{!collections.length ? <p>Create a collection in Workspace first.</p> : null}</section>
            <section className="replay-section"><h3>Bounded repeat</h3><p>Up to 100 requests, concurrency 1–4, a four-minute start schedule and a five-minute run limit. Requests already sent can have effects on the target.</p><div className="mock-grid three-column"><label className="field-label">Count<input className="text-input" type="number" min="1" max="100" value={repeatCount} onChange={(event) => setRepeatCount(Number(event.target.value))} /></label><label className="field-label">Interval (ms)<input className="text-input" type="number" min="0" max="60000" value={intervalMs} onChange={(event) => setIntervalMs(Number(event.target.value))} /></label><label className="field-label">Concurrency<input className="text-input" type="number" min="1" max="4" value={concurrency} onChange={(event) => setConcurrency(Number(event.target.value))} /></label></div><button className="secondary compact" disabled={sending || truncatedBodyBlocked || Boolean(bodyError)} onClick={() => void repeat()}>Run repeat</button>{outcomes.map((outcome) => <p key={outcome.index}>#{outcome.index + 1} · {outcome.detail?.summary.id ?? "Not sent"} · {outcome.error?.message ?? outcome.detail?.errorMessage ?? outcome.detail?.response?.statusCode ?? "Unknown outcome"}</p>)}</section>
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
