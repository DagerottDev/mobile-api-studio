import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useMemo, useState } from "react";
import type { MockFixture, MockRule } from "../mockTypes";
import type {
  BodyPayload,
  BodyRef,
  CaptureSession,
  FlowDetail,
  FlowSource,
  HeaderValue,
  SavedCollection,
  SavedRequest,
  TrafficSearchResult,
} from "../types";

type StatusFilter = "all" | "2xx" | "3xx" | "4xx" | "5xx";
const METHODS = ["all", "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
const SOURCES: Array<"all" | FlowSource> = ["all", "proxy", "replay", "mock", "sdk", "fixture"];

export function TrafficView() {
  const [results, setResults] = useState<TrafficSearchResult[]>([]);
  const [sessions, setSessions] = useState<CaptureSession[]>([]);
  const [collections, setCollections] = useState<SavedCollection[]>([]);
  const [selectedFlowId, setSelectedFlowId] = useState<string | null>(null);
  const [detail, setDetail] = useState<FlowDetail | null>(null);
  const [requestBody, setRequestBody] = useState<BodyPayload | null>(null);
  const [responseBody, setResponseBody] = useState<BodyPayload | null>(null);
  const [textFilter, setTextFilter] = useState("");
  const [methodFilter, setMethodFilter] = useState("all");
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("all");
  const [sourceFilter, setSourceFilter] = useState<"all" | FlowSource>("all");
  const [sessionFilter, setSessionFilter] = useState("all");
  const [collectionId, setCollectionId] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [curlPreview, setCurlPreview] = useState<string | null>(null);
  const [curlCopied, setCurlCopied] = useState(false);
  const [saveState, setSaveState] = useState("Save to collection");
  const [mockState, setMockState] = useState("Create mock");
  const [fixtureState, setFixtureState] = useState("Save fixture");

  const refreshMetadata = useCallback(async () => {
    try {
      const [nextSessions, nextCollections] = await Promise.all([
        invoke<CaptureSession[]>("list_sessions"),
        invoke<SavedCollection[]>("list_collections"),
      ]);
      setSessions(nextSessions);
      setCollections(nextCollections);
      setCollectionId((current) => current && nextCollections.some((item) => item.id === current) ? current : nextCollections[0]?.id ?? "");
    } catch (value) {
      setError(formatInvokeError(value));
    }
  }, []);

  const refresh = useCallback(async () => {
    try {
      const result = await invoke<TrafficSearchResult[]>("search_traffic", {
        query: {
          text: textFilter.trim() || null,
          sessionId: sessionFilter === "all" ? null : sessionFilter,
          source: sourceFilter === "all" ? null : sourceFilter,
          method: methodFilter === "all" ? null : methodFilter,
          statusClass: statusFilter === "all" ? null : Number(statusFilter[0]),
          endpointKey: null,
          limit: 1500,
        },
      });
      setResults(result);
      setSelectedFlowId((current) => {
        if (current && result.some((item) => item.flow.id === current)) return current;
        return result[0]?.flow.id ?? null;
      });
      setError(null);
    } catch (value) {
      setError(formatInvokeError(value));
    }
  }, [methodFilter, sessionFilter, sourceFilter, statusFilter, textFilter]);

  useEffect(() => { void refreshMetadata(); }, [refreshMetadata]);

  useEffect(() => {
    const debounce = window.setTimeout(() => void refresh(), 140);
    return () => window.clearTimeout(debounce);
  }, [refresh]);

  useEffect(() => {
    const timer = window.setInterval(() => void refresh(), 1200);
    return () => window.clearInterval(timer);
  }, [refresh]);

  useEffect(() => {
    if (!selectedFlowId) {
      setDetail(null);
      setRequestBody(null);
      setResponseBody(null);
      return;
    }

    let cancelled = false;
    async function loadDetail() {
      try {
        const nextDetail = await invoke<FlowDetail | null>("get_flow_detail", { flowId: selectedFlowId });
        if (cancelled) return;
        setDetail(nextDetail);
        setCurlPreview(null);
        setCurlCopied(false);
        setSaveState("Save to collection");
        setMockState("Create mock");
        setFixtureState("Save fixture");

        const [request, response] = await Promise.all([
          loadBody(nextDetail?.request?.body ?? null),
          loadBody(nextDetail?.response?.body ?? null),
        ]);
        if (!cancelled) {
          setRequestBody(request);
          setResponseBody(response);
        }
      } catch (value) {
        if (!cancelled) setError(formatInvokeError(value));
      }
    }

    void loadDetail();
    return () => { cancelled = true; };
  }, [selectedFlowId]);

  const selectedSearchResult = useMemo(
    () => results.find((item) => item.flow.id === selectedFlowId) ?? null,
    [results, selectedFlowId],
  );

  async function copyCurl() {
    if (!selectedFlowId) return;
    try {
      const curl = await invoke<string>("export_curl", { flowId: selectedFlowId });
      setCurlPreview(curl);
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(curl);
        setCurlCopied(true);
      }
      setError(null);
    } catch (value) {
      setError(formatInvokeError(value));
    }
  }

  async function createMock() {
    if (!selectedFlowId || !detail?.response) return;
    try {
      const rule = await invoke<MockRule>("create_mock_from_flow", { flowId: selectedFlowId });
      setMockState(`Mock active: ${rule.name}`);
      setError(null);
    } catch (value) {
      setMockState("Create mock failed");
      setError(formatInvokeError(value));
    }
  }

  async function createFixture() {
    if (!selectedFlowId || !detail?.response) return;
    try {
      const fixture = await invoke<MockFixture>("create_fixture_from_flow", {
        flowId: selectedFlowId,
        name: null,
      });
      setFixtureState(`Fixture saved: ${fixture.name}`);
      setError(null);
    } catch (value) {
      setFixtureState("Save fixture failed");
      setError(formatInvokeError(value));
    }
  }

  async function saveToCollection() {
    if (!selectedFlowId || !collectionId || !detail?.request) return;
    try {
      const saved = await invoke<SavedRequest>("save_flow_to_collection", {
        input: { collectionId, flowId: selectedFlowId, name: null },
      });
      setSaveState(`Saved: ${saved.name}`);
      setError(null);
    } catch (value) {
      setSaveState("Save failed");
      setError(formatInvokeError(value));
    }
  }

  return (
    <section className="traffic-layout">
      <div className="traffic-list panel">
        <div className="panel-heading">
          <div>
            <strong>Traffic search</strong>
            <span>{results.length} matches · backend search · live refresh</span>
          </div>
          <button className="secondary compact" onClick={() => void Promise.all([refresh(), refreshMetadata()])}>Refresh</button>
        </div>

        <div className="traffic-filters traffic-filters-advanced">
          <input className="text-input" value={textFilter} onChange={(event) => setTextFilter(event.target.value)} placeholder="Search host, path, endpoint, or session" />
          <select value={sessionFilter} onChange={(event) => setSessionFilter(event.target.value)}>
            <option value="all">All sessions</option>
            {sessions.map((session) => <option key={session.id} value={session.id}>{session.name}</option>)}
          </select>
          <select value={methodFilter} onChange={(event) => setMethodFilter(event.target.value)}>
            {METHODS.map((method) => <option key={method} value={method}>{method === "all" ? "All methods" : method}</option>)}
          </select>
          <select value={sourceFilter} onChange={(event) => setSourceFilter(event.target.value as "all" | FlowSource)}>
            {SOURCES.map((source) => <option key={source} value={source}>{source === "all" ? "All sources" : source}</option>)}
          </select>
          <select value={statusFilter} onChange={(event) => setStatusFilter(event.target.value as StatusFilter)}>
            <option value="all">All statuses</option><option value="2xx">2xx</option><option value="3xx">3xx</option><option value="4xx">4xx</option><option value="5xx">5xx</option>
          </select>
        </div>

        {error ? <div className="error-banner">{error}</div> : null}

        <div className="flow-header flow-grid"><span>Method</span><span>Host / Path</span><span>Status</span><span>Time</span></div>
        <div className="flow-rows">
          {results.map(({ flow, endpoint, sessionName }) => (
            <button key={flow.id} className={flow.id === selectedFlowId ? "flow-row flow-grid selected" : "flow-row flow-grid"} onClick={() => setSelectedFlowId(flow.id)}>
              <span className={`method method-${flow.method.toLowerCase()}`}>{flow.method}</span>
              <span className="endpoint"><strong>{flow.host}</strong><small>{flow.path}</small><small className="normalized-endpoint">{endpoint.pathTemplate}{sessionName ? ` · ${sessionName}` : ""} · {flow.source}</small></span>
              <span>{flow.statusCode ?? "—"}</span>
              <span>{flow.durationMs != null ? `${flow.durationMs} ms` : "—"}</span>
            </button>
          ))}
          {results.length === 0 ? <p className="empty-state">No stored flows match this search.</p> : null}
        </div>
      </div>

      <div className="inspector panel">
        <div className="panel-heading inspector-heading">
          <div><strong>Inspector</strong><span>{detail?.request?.url ?? "Select a captured flow"}</span></div>
          {detail?.request ? <div className="inspector-actions"><button className="secondary compact" onClick={() => void copyCurl()}>{curlCopied ? "Copied cURL" : "Copy safe cURL"}</button>{detail.response ? <><button className="secondary compact" onClick={() => void createFixture()}>{fixtureState}</button><button className="primary compact" onClick={() => void createMock()}>{mockState}</button></> : null}</div> : null}
        </div>

        {detail ? (
          <div className="inspector-scroll">
            <InspectorSummary detail={detail} sessionName={selectedSearchResult?.sessionName ?? null} endpointKey={selectedSearchResult?.endpoint.key ?? null} />
            {collections.length > 0 ? <section className="inspector-section collection-save-panel"><h3>Save request</h3><div className="collection-save-row"><select value={collectionId} onChange={(event) => setCollectionId(event.target.value)}>{collections.map((collection) => <option key={collection.id} value={collection.id}>{collection.name}</option>)}</select><button className="primary compact" onClick={() => void saveToCollection()}>{saveState}</button></div></section> : <section className="inspector-section"><h3>Save request</h3><p className="muted-copy">Create a collection in Workspace to save this request.</p></section>}
            <InspectorHeaders title="Request headers" headers={detail.request?.headers ?? []} />
            <InspectorBody title="Request body" bodyRef={detail.request?.body ?? null} payload={requestBody} />
            <InspectorHeaders title="Response headers" headers={detail.response?.headers ?? []} />
            <InspectorBody title="Response body" bodyRef={detail.response?.body ?? null} payload={responseBody} />
            <InspectorTiming detail={detail} />
            {curlPreview ? <section className="inspector-section"><h3>Safe cURL</h3><pre>{curlPreview}</pre></section> : null}
          </div>
        ) : <p className="empty-state">No full detail is available for this flow yet.</p>}
      </div>
    </section>
  );
}

function InspectorSummary({ detail, sessionName, endpointKey }: { detail: FlowDetail; sessionName: string | null; endpointKey: string | null }) {
  return <section className="inspector-section"><h3>Overview</h3><dl className="detail-grid compact-detail-grid"><dt>Method</dt><dd>{detail.request?.method ?? detail.summary.method}</dd><dt>Status</dt><dd>{detail.response?.statusCode ?? detail.summary.statusCode ?? "pending"}</dd><dt>URL</dt><dd>{detail.request?.url ?? `${detail.summary.host}${detail.summary.path}`}</dd><dt>Session</dt><dd>{sessionName ?? detail.summary.sessionId ?? "unassigned"}</dd><dt>Source</dt><dd>{detail.summary.source}</dd><dt>Endpoint</dt><dd>{endpointKey ?? "—"}</dd><dt>Total</dt><dd>{formatMs(detail.timing.totalMs)}</dd></dl></section>;
}

function InspectorHeaders({ title, headers }: { title: string; headers: HeaderValue[] }) {
  return <section className="inspector-section"><h3>{title}</h3>{headers.length === 0 ? <p className="muted-copy">No headers captured.</p> : <div className="header-table">{headers.map((header, index) => <div className="header-row" key={`${header.name}-${index}`}><strong>{header.name}</strong><span className={header.sensitive ? "redacted-value" : ""}>{header.sensitive ? "<redacted>" : header.value}</span></div>)}</div>}</section>;
}

function InspectorBody({ title, bodyRef, payload }: { title: string; bodyRef: BodyRef | null; payload: BodyPayload | null }) {
  if (!bodyRef) return <section className="inspector-section"><h3>{title}</h3><p className="muted-copy">No body captured.</p></section>;
  const raw = payload?.text ?? payload?.base64 ?? "Loading body…";
  const maxDisplay = 100_000;
  const rendered = raw.length > maxDisplay ? `${raw.slice(0, maxDisplay)}\n… UI preview truncated …` : raw;
  return <section className="inspector-section"><div className="section-title-row"><h3>{title}</h3><span>{bodyRef.contentType ?? "unknown"} · {bodyRef.byteSize} bytes{bodyRef.isTruncated ? " · capture truncated" : ""}</span></div><pre>{bodyRef.isBinary && payload?.base64 ? `Base64\n${rendered}` : rendered}</pre></section>;
}

function InspectorTiming({ detail }: { detail: FlowDetail }) {
  const timing = detail.timing;
  return <section className="inspector-section"><h3>Timing</h3><div className="timing-grid"><span>Request <strong>{formatMs(timing.requestMs)}</strong></span><span>Server <strong>{formatMs(timing.serverMs)}</strong></span><span>Download <strong>{formatMs(timing.downloadMs)}</strong></span><span>Total <strong>{formatMs(timing.totalMs)}</strong></span></div></section>;
}

async function loadBody(body: BodyRef | null) { if (!body) return null; return invoke<BodyPayload>("read_body", { sha256: body.sha256 }); }
function formatMs(value: number | null) { return value == null ? "—" : `${value} ms`; }
function formatInvokeError(value: unknown) { if (typeof value === "string") return value; if (value && typeof value === "object" && "message" in value) return String((value as { message: unknown }).message); return String(value); }
