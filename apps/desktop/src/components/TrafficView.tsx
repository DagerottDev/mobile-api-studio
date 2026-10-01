import { invoke } from "../api/invoke";
import { BodyViewer } from "./BodyViewer";
import { Fragment, useCallback, useEffect, useMemo, useState } from "react";
import type { MockFixture, MockRule } from "../mockTypes";
import type { FlowSdkEnrichment, SdkContextSnapshot, SdkEnvelope } from "../sdkTypes";
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
  WebSocketMessage,
} from "../types";

type StatusFilter = "all" | "2xx" | "3xx" | "4xx" | "5xx";
const METHODS = ["all", "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
const SOURCES: Array<"all" | FlowSource> = ["all", "proxy", "replay", "mock", "sdk", "fixture"];

export function TrafficView({ onOpenConnect }: { onOpenConnect: () => void }) {
  const [results, setResults] = useState<TrafficSearchResult[]>([]);
  const [sessions, setSessions] = useState<CaptureSession[]>([]);
  const [collections, setCollections] = useState<SavedCollection[]>([]);
  const [selectedFlowId, setSelectedFlowId] = useState<string | null>(null);
  const [detail, setDetail] = useState<FlowDetail | null>(null);
  const [sdkEnrichment, setSdkEnrichment] = useState<FlowSdkEnrichment | null>(null);
  const [sdkError, setSdkError] = useState<string | null>(null);
  const [requestBody, setRequestBody] = useState<BodyPayload | null>(null);
  const [responseBody, setResponseBody] = useState<BodyPayload | null>(null);
  const [textFilter, setTextFilter] = useState("");
  const [sdkMetadataFilter, setSdkMetadataFilter] = useState("");
  const [methodFilter, setMethodFilter] = useState("all");
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("all");
  const [sourceFilter, setSourceFilter] = useState<"all" | FlowSource>("all");
  const [sessionFilter, setSessionFilter] = useState("all");
  const [collectionId, setCollectionId] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [detailLoading, setDetailLoading] = useState(false);
  const [detailError, setDetailError] = useState<string | null>(null);
  const [detailRetry, setDetailRetry] = useState(0);
  const [curlPreview, setCurlPreview] = useState<string | null>(null);
  const [curlCopied, setCurlCopied] = useState(false);
  const [saveState, setSaveState] = useState("Save to collection");
  const [mockState, setMockState] = useState("Create mock");
  const [fixtureState, setFixtureState] = useState("Save fixture");
  const [wsQuery, setWsQuery] = useState("");
  const [wsPage, setWsPage] = useState(0);
  const [wsMessages, setWsMessages] = useState<WebSocketMessage[]>([]);
  const [wsSelectedId, setWsSelectedId] = useState<string | null>(null);
  const [wsBody, setWsBody] = useState<BodyPayload | null>(null);
  const [wsBodyError, setWsBodyError] = useState<string | null>(null);
  const [wsLoading, setWsLoading] = useState(false);
  const [wsError, setWsError] = useState<string | null>(null);

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

  const refresh = useCallback(async (silent = false) => {
    if (!silent) setLoading(true);
    try {
      const sdkNeedle = sdkMetadataFilter.trim();
      const [baseResults, sdkFlowIds] = await Promise.all([
        invoke<TrafficSearchResult[]>("search_traffic", {
          query: {
            text: textFilter.trim() || null,
            sessionId: sessionFilter === "all" ? null : sessionFilter,
            source: sourceFilter === "all" ? null : sourceFilter,
            method: methodFilter === "all" ? null : methodFilter,
            statusClass: statusFilter === "all" ? null : Number(statusFilter[0]),
            endpointKey: null,
            limit: 1500,
          },
        }),
        sdkNeedle
          ? invoke<string[]>("sdk_flow_ids_matching", { text: sdkNeedle, limit: 1500 })
          : Promise.resolve<string[] | null>(null),
      ]);
      const sdkSet = sdkFlowIds ? new Set(sdkFlowIds) : null;
      const nextResults = sdkSet ? baseResults.filter((item) => sdkSet.has(item.flow.id)) : baseResults;
      setResults(nextResults);
      setSelectedFlowId((current) => {
        if (current && nextResults.some((item) => item.flow.id === current)) return current;
        return nextResults[0]?.flow.id ?? null;
      });
      setError(null);
    } catch (value) {
      setError(formatInvokeError(value));
    } finally {
      if (!silent) setLoading(false);
    }
  }, [methodFilter, sdkMetadataFilter, sessionFilter, sourceFilter, statusFilter, textFilter]);

  useEffect(() => { void refreshMetadata(); }, [refreshMetadata]);

  useEffect(() => {
    const debounce = window.setTimeout(() => void refresh(), 140);
    return () => window.clearTimeout(debounce);
  }, [refresh]);

  useEffect(() => {
    const timer = window.setInterval(() => void refresh(true), 1200);
    return () => window.clearInterval(timer);
  }, [refresh]);

  useEffect(() => {
    if (!selectedFlowId) {
      setDetail(null);
      setDetailLoading(false);
      setDetailError(null);
      setSdkEnrichment(null);
      setSdkError(null);
      setRequestBody(null);
      setResponseBody(null);
      return;
    }

    let cancelled = false;
    async function loadDetail() {
      setDetail(null);
      setRequestBody(null);
      setResponseBody(null);
      setDetailLoading(true);
      setDetailError(null);
      setSdkError(null);
      try {
        const nextDetail = await invoke<FlowDetail | null>("get_flow_detail", { flowId: selectedFlowId });
        if (cancelled) return;
        setDetail(nextDetail);
        if (nextDetail) {
          try {
            const enrichment = await invoke<FlowSdkEnrichment>("sdk_enrichment_for_flow", { flowId: selectedFlowId });
            if (!cancelled) setSdkEnrichment(enrichment);
          } catch (value) {
            if (!cancelled) {
              setSdkEnrichment(null);
              setSdkError(formatInvokeError(value));
            }
          }
        } else {
          setSdkEnrichment(null);
        }
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
        if (!cancelled) setDetailError(formatInvokeError(value));
      } finally {
        if (!cancelled) setDetailLoading(false);
      }
    }

    void loadDetail();
    return () => { cancelled = true; };
  }, [selectedFlowId, detailRetry]);

  useEffect(() => { setWsQuery(""); setWsPage(0); setWsSelectedId(null); }, [selectedFlowId]);

  const isWebSocket = Boolean(selectedFlowId && detail?.summary.id === selectedFlowId && detail.protocol?.websocket);
  useEffect(() => {
    if (!selectedFlowId || !isWebSocket) { setWsMessages([]); setWsSelectedId(null); setWsError(null); setWsLoading(false); return; }
    const flowId = selectedFlowId;
    let cancelled = false;
    let timer: number;
    let first = true;
    const loadMessages = async () => {
      if (first) setWsLoading(true);
      try {
        const [messages, currentDetail] = await Promise.all([
          invoke<WebSocketMessage[]>("list_websocket_messages", { flowId, sessionId: null, text: wsQuery.trim() || null, limit: 200, offset: wsPage * 200 }),
          invoke<FlowDetail | null>("get_flow_detail", { flowId }),
        ]);
        if (cancelled) return;
        setWsMessages(messages);
        setWsSelectedId((current) => current && messages.some((message) => message.id === current) ? current : messages[0]?.id ?? null);
        if (currentDetail) setDetail((current) => current?.summary.id === flowId ? currentDetail : current);
        setWsError(null);
      } catch (value) { if (!cancelled) setWsError(formatInvokeError(value)); }
      finally {
        if (!cancelled) { setWsLoading(false); first = false; timer = window.setTimeout(() => void loadMessages(), 1200); }
      }
    };
    timer = window.setTimeout(() => void loadMessages(), 200);
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, [selectedFlowId, isWebSocket, wsQuery, wsPage]);

  const selectedWsMessage = wsMessages.find((message) => message.id === wsSelectedId) ?? null;
  useEffect(() => {
    let cancelled = false;
    setWsBody(null);
    setWsBodyError(null);
    const expectedSha = selectedWsMessage?.body?.sha256;
    if (selectedWsMessage?.body) void loadBody(selectedWsMessage.body).then((body) => { if (!cancelled && body?.sha256 === expectedSha) setWsBody(body); }).catch((value) => { if (!cancelled) setWsBodyError(formatInvokeError(value)); });
    return () => { cancelled = true; };
  }, [selectedWsMessage?.id, selectedWsMessage?.body?.sha256]);

  const hasActiveFilters = Boolean(textFilter.trim() || sdkMetadataFilter.trim())
    || methodFilter !== "all" || statusFilter !== "all"
    || sourceFilter !== "all" || sessionFilter !== "all";

  function clearFilters() {
    setTextFilter("");
    setSdkMetadataFilter("");
    setMethodFilter("all");
    setStatusFilter("all");
    setSourceFilter("all");
    setSessionFilter("all");
  }

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
            <span>{loading ? "Searching traffic…" : `${results.length.toLocaleString()} matching flows`} · live refresh</span>
          </div>
          <button className="secondary compact" onClick={() => void Promise.all([refresh(), refreshMetadata()])}>Refresh</button>
        </div>

        <div className="traffic-search"><input className="text-input" aria-label="Search network traffic" value={textFilter} onChange={(event) => setTextFilter(event.target.value)} placeholder="Search host, path, or endpoint" /></div>
        <details className="traffic-filter-details"><summary>More filters</summary><div className="traffic-filters traffic-filters-advanced sdk-aware-filters">
          <input className="text-input" aria-label="Filter app context" value={sdkMetadataFilter} onChange={(event) => setSdkMetadataFilter(event.target.value)} placeholder="App context: screen, feature, source" />
          <select aria-label="Filter session" value={sessionFilter} onChange={(event) => setSessionFilter(event.target.value)}>
            <option value="all">All sessions</option>
            {sessions.map((session) => <option key={session.id} value={session.id}>{session.name}</option>)}
          </select>
          <select aria-label="Filter method" value={methodFilter} onChange={(event) => setMethodFilter(event.target.value)}>
            {METHODS.map((method) => <option key={method} value={method}>{method === "all" ? "All methods" : method}</option>)}
          </select>
          <select aria-label="Filter source" value={sourceFilter} onChange={(event) => setSourceFilter(event.target.value as "all" | FlowSource)}>
            {SOURCES.map((source) => <option key={source} value={source}>{source === "all" ? "All sources" : source}</option>)}
          </select>
          <select aria-label="Filter status" value={statusFilter} onChange={(event) => setStatusFilter(event.target.value as StatusFilter)}>
            <option value="all">All statuses</option><option value="2xx">2xx</option><option value="3xx">3xx</option><option value="4xx">4xx</option><option value="5xx">5xx</option>
          </select>
        </div></details>

        {error ? <div className="error-banner">{error}</div> : null}

        <div className="flow-header flow-grid"><span>Method</span><span>Host / Path</span><span>Status</span><span>Time</span></div>
        <div className="flow-rows">
          {results.map(({ flow, endpoint, sessionName }) => (
            <button key={flow.id} className={flow.id === selectedFlowId ? "flow-row flow-grid selected" : "flow-row flow-grid"} onClick={() => setSelectedFlowId(flow.id)}>
              <span className={`method method-${flow.method.toLowerCase()}`}>{flow.method}</span>
              <span className="endpoint"><strong>{flow.host}</strong><small>{flow.path}</small><small className="normalized-endpoint">{endpoint.pathTemplate}{sessionName ? ` · ${sessionName}` : ""} · {flow.source}{sdkMetadataFilter.trim() ? " · SDK match" : ""}</small></span>
              <span>{flow.statusCode ?? "—"}</span>
              <span>{flow.durationMs != null ? `${flow.durationMs} ms` : "—"}</span>
            </button>
          ))}
          {!loading && results.length === 0 ? <div className="empty-state flow-empty">
            <p>{hasActiveFilters ? "No flows match these filters." : "No traffic yet. Connect a runtime and send a request to see it here."}</p>
            <button className="secondary compact" onClick={hasActiveFilters ? clearFilters : onOpenConnect}>{hasActiveFilters ? "Clear filters" : "Open Connect"}</button>
          </div> : null}
          {loading && results.length === 0 ? <p className="empty-state" role="status">Loading captured traffic…</p> : null}
        </div>
      </div>

      <div className="inspector panel">
        <div className="panel-heading inspector-heading">
          <div><strong>Inspector</strong><span>{detail?.request?.url ?? (selectedFlowId ? "Selected flow" : "Select a captured flow")}</span></div>
          {detail?.request ? <div className="inspector-actions"><button className="secondary compact" onClick={() => void copyCurl()}>{curlCopied ? "Copied cURL" : "Copy safe cURL"}</button>{detail.response ? <><button className="secondary compact" onClick={() => void createFixture()}>{fixtureState}</button><button className="primary compact" onClick={() => void createMock()}>{mockState}</button></> : null}</div> : null}
        </div>

        {detail ? (
          <div className="inspector-scroll">
            {detail.errorMessage ? <section className="inspector-section" role="alert"><h3>Request failed</h3><p>{detail.errorCode ? `${detail.errorCode}: ` : ""}{detail.errorMessage}</p></section> : null}
            <InspectorSummary detail={detail} sessionName={selectedSearchResult?.sessionName ?? null} endpointKey={selectedSearchResult?.endpoint.key ?? null} />
            <InspectorProtocol detail={detail} />
            <InspectorProxyRules detail={detail} />
            {sdkError ? <section className="inspector-section" role="alert"><h3>App context</h3><p className="muted-copy">App context could not load: {sdkError}</p></section>
              : <SdkEnrichmentSection enrichment={sdkEnrichment} />}
            {collections.length > 0 ? <section className="inspector-section collection-save-panel"><h3>Save request</h3><div className="collection-save-row"><select value={collectionId} onChange={(event) => setCollectionId(event.target.value)}>{collections.map((collection) => <option key={collection.id} value={collection.id}>{collection.name}</option>)}</select><button className="primary compact" onClick={() => void saveToCollection()}>{saveState}</button></div></section> : <section className="inspector-section"><h3>Save request</h3><p className="muted-copy">Create a collection in Workspace to save this request.</p></section>}
            <InspectorHeaders title="Request headers" headers={detail.request?.headers ?? []} />
            {detail.protocol?.requestTrailers.length ? <InspectorHeaders title="Request trailers" headers={detail.protocol.requestTrailers} /> : null}
            <BodyViewer key={`request-${detail.request?.body?.sha256 ?? "none"}`} title="Request body" bodyRef={detail.request?.body ?? null} payload={requestBody} />
            <InspectorHeaders title="Response headers" headers={detail.response?.headers ?? []} />
            {detail.protocol?.responseTrailers.length ? <InspectorHeaders title="Response trailers" headers={detail.protocol.responseTrailers} /> : null}
            <BodyViewer key={`response-${detail.response?.body?.sha256 ?? "none"}`} title="Response body" bodyRef={detail.response?.body ?? null} payload={responseBody} />
            {isWebSocket ? <section className="inspector-section"><h3>WebSocket messages</h3>
              <label className="field-label">Search indexed JSON/form text<input className="text-input" value={wsQuery} maxLength={256} onChange={(event) => { setWsQuery(event.target.value); setWsPage(0); setWsMessages([]); setWsSelectedId(null); }} placeholder="Search indexed JSON/form fields" /></label>
              {wsError ? <p role="alert">{wsError}</p> : null}
              <div className="ws-message-list">{wsMessages.map((message) => <button key={message.id} className="secondary compact ws-message-row" aria-pressed={wsSelectedId === message.id} onClick={() => setWsSelectedId(message.id)}>
                <strong>#{message.sequence} {message.fromClient ? "Client → server" : "Server → client"}</strong>
                <span>{message.opcode === 1 ? "Text" : message.opcode === 2 ? "Binary" : `Opcode ${message.opcode}`} · {formatClock(message.timestamp)}{message.dropped ? " · dropped" : ""}{message.injected ? " · injected" : ""}</span>
              </button>)}</div>
              {!wsLoading && wsMessages.length === 0 && !wsError ? <p className="muted-copy">No messages match this flow and search.</p> : null}
              {wsLoading ? <p className="muted-copy" role="status">Loading messages…</p> : null}
              <div className="inspector-actions"><button className="secondary compact" onClick={() => { setWsMessages([]); setWsPage((page) => Math.max(0, page - 1)); }} disabled={wsLoading || wsPage === 0}>Previous</button><span>Page {wsPage + 1}</span><button className="secondary compact" onClick={() => { setWsMessages([]); setWsPage((page) => page + 1); }} disabled={wsLoading || wsMessages.length < 200}>Next</button></div>
            </section> : null}
            {isWebSocket && selectedWsMessage ? <>{wsBodyError ? <p role="alert">Message body could not load: {wsBodyError}</p> : null}<BodyViewer key={`ws-${selectedWsMessage.id}-${selectedWsMessage.body?.sha256 ?? "none"}`} title={`WebSocket message #${selectedWsMessage.sequence}`} bodyRef={selectedWsMessage.body} payload={wsBody} /></> : null}
            <InspectorTiming detail={detail} />
            {curlPreview ? <section className="inspector-section"><h3>Safe cURL</h3><pre>{curlPreview}</pre></section> : null}
          </div>
        ) : detailError ? <div className="empty-state flow-empty" role="alert"><p>Could not load this flow: {detailError}</p><button className="secondary compact" onClick={() => setDetailRetry((current) => current + 1)}>Retry details</button></div>
          : detailLoading ? <p className="empty-state" role="status">Loading flow details…</p>
            : selectedFlowId ? <p className="empty-state">This flow has a summary, but no request or response detail was saved.</p>
              : <p className="empty-state">Nothing to inspect yet.</p>}
      </div>
    </section>
  );
}

function SdkEnrichmentSection({ enrichment }: { enrichment: FlowSdkEnrichment | null }) {
  if (!enrichment?.requestId) {
    return <section className="inspector-section"><h3>App context</h3><p className="muted-copy">No Mobile API Studio SDK correlation metadata was attached to this request.</p></section>;
  }

  const networkEvents = enrichment.requestEvents.filter((event) => event.event.type === "network");
  const network = networkEvents.at(-1);
  const context: SdkContextSnapshot | null = network?.event.type === "network" ? network.event.payload.context : null;
  const source = context?.source;
  const nearby = enrichment.nearbyEvents.filter((event) => event.event.type === "log" || event.event.type === "context").slice(-8);

  return <section className="inspector-section">
    <h3>App context</h3>
    <div className="sdk-enrichment-card">
      <div className="section-title-row"><strong>{enrichment.client?.appName ?? "SDK-enriched request"}</strong><span>{enrichment.client?.platform ?? "sdk"} · {enrichment.client?.deviceName ?? "unknown device"}</span></div>
      <div className="sdk-enrichment-badges">
        {context?.screen ? <span>Screen · {context.screen}</span> : null}
        {context?.feature ? <span>Feature · {context.feature}</span> : null}
        {source?.file ? <span>Source · {source.file}{source.line ? `:${source.line}` : ""}</span> : null}
      </div>
      {source?.function ? <p className="muted-copy">Function: {source.function}</p> : null}
      {context && Object.keys(context.attributes).length > 0 ? <dl className="detail-grid compact-detail-grid">{Object.entries(context.attributes).flatMap(([key, value]) => [<dt key={`${key}-k`}>{key}</dt>, <dd key={`${key}-v`}>{value}</dd>])}</dl> : null}
      {nearby.length > 0 ? <div className="sdk-nearby-list"><strong>Nearby app events</strong>{nearby.map((event) => <NearbySdkEvent key={event.eventId} event={event} />)}</div> : null}
    </div>
  </section>;
}

function NearbySdkEvent({ event }: { event: SdkEnvelope }) {
  if (event.event.type === "log") {
    return <div className="sdk-nearby-row"><span>{formatClock(event.occurredAt)}</span><span>{event.event.payload.level}: {event.event.payload.message}</span></div>;
  }
  if (event.event.type === "context") {
    const context = event.event.payload.context;
    return <div className="sdk-nearby-row"><span>{formatClock(event.occurredAt)}</span><span>context: {[context.screen, context.feature].filter(Boolean).join(" · ") || "updated"}</span></div>;
  }
  return null;
}

function InspectorSummary({ detail, sessionName, endpointKey }: { detail: FlowDetail; sessionName: string | null; endpointKey: string | null }) {
  return <section className="inspector-section"><h3>Overview</h3><dl className="detail-grid compact-detail-grid"><dt>Method</dt><dd>{detail.request?.method ?? detail.summary.method}</dd><dt>Status</dt><dd>{detail.response?.statusCode ?? detail.summary.statusCode ?? "pending"}</dd><dt>URL</dt><dd>{detail.request?.url ?? `${detail.summary.host}${detail.summary.path}`}</dd><dt>Session</dt><dd>{sessionName ?? detail.summary.sessionId ?? "unassigned"}</dd><dt>Source</dt><dd>{detail.summary.source}</dd><dt>Endpoint</dt><dd>{endpointKey ?? "—"}</dd><dt>Total</dt><dd>{formatMs(detail.timing.totalMs)}</dd></dl></section>;
}

function InspectorProtocol({ detail }: { detail: FlowDetail }) {
  const protocol = detail.protocol;
  if (!protocol) return null;
  const grpcHeader = (name: string) => {
    const header = protocol.responseTrailers.find((item) => item.name.toLowerCase() === name)
      ?? detail.response?.headers.find((item) => item.name.toLowerCase() === name);
    return header ? header.sensitive ? "<redacted>" : header.value : null;
  };
  const grpcStatus = grpcHeader("grpc-status");
  const grpcMessage = grpcHeader("grpc-message");
  return <section className="inspector-section"><h3>Protocol and connections</h3>
    <dl className="detail-grid compact-detail-grid">
      <dt>Request</dt><dd>{protocol.requestHttpVersion ?? "unknown HTTP version"}</dd>
      <dt>Response</dt><dd>{protocol.responseHttpVersion ?? "unknown HTTP version"}</dd>
      {grpcStatus !== null ? <><dt>gRPC status</dt><dd>{grpcStatus}</dd></> : null}
      {grpcMessage !== null ? <><dt>gRPC message</dt><dd>{grpcMessage}</dd></> : null}
      {protocol.websocket ? <><dt>WebSocket</dt><dd>{protocol.websocketCloseCode != null ? `Closed with code ${protocol.websocketCloseCode}${protocol.websocketCloseReason ? ` · ${protocol.websocketCloseReason}` : ""}${protocol.websocketClosedByClient == null ? "" : protocol.websocketClosedByClient ? " · client closed" : " · server closed"}` : "Upgraded"}</dd></> : null}
    </dl>
    {(["clientConnection", "serverConnection"] as const).map((side) => {
      const connection = protocol[side];
      if (!connection) return null;
      return <details key={side}><summary>{side === "clientConnection" ? "Client connection" : "Server connection"} · {connection.transport}{connection.alpn ? ` · ALPN ${connection.alpn}` : ""}</summary>
        <dl className="detail-grid compact-detail-grid">
          <dt>ID</dt><dd>{connection.id}</dd>
          <dt>Peer</dt><dd>{connection.peerAddress ?? "—"}</dd>
          <dt>Local</dt><dd>{connection.localAddress ?? "—"}</dd>
          {connection.serverAddress ? <><dt>Server</dt><dd>{connection.serverAddress}</dd></> : null}
          <dt>TLS</dt><dd>{connection.tlsEstablished ? `${connection.tlsVersion ?? "established"}${connection.cipher ? ` · ${connection.cipher}` : ""}` : "Not established"}</dd>
          {connection.sni ? <><dt>SNI</dt><dd>{connection.sni}</dd></> : null}
          {connection.alpn ? <><dt>ALPN</dt><dd>{connection.alpn}</dd></> : null}
          {connection.peerCertificates[0] ? <><dt>Certificate</dt><dd>{connection.peerCertificates[0].subject}</dd></> : null}
          {connection.peerCertificates[0] ? <><dt>Issuer</dt><dd>{connection.peerCertificates[0].issuer}</dd><dt>SHA-256</dt><dd>{connection.peerCertificates[0].sha256}</dd></> : null}
          {connection.startedAt ? <><dt>Started</dt><dd>{formatClock(connection.startedAt)}</dd></> : null}
          {connection.tlsEstablishedAt ? <><dt>TLS at</dt><dd>{formatClock(connection.tlsEstablishedAt)}</dd></> : null}
          {connection.endedAt ? <><dt>Ended</dt><dd>{formatClock(connection.endedAt)}</dd></> : null}
        </dl>
      </details>;
    })}
  </section>;
}

function InspectorProxyRules({ detail }: { detail: FlowDetail }) {
  const ids = detail.proxyRuleIds ?? [];
  const changes = detail.proxyRuleChanges ?? [];
  if (!ids.length && !changes.length) return null;
  return <section className="inspector-section"><h3>Applied proxy rules</h3><dl className="detail-grid compact-detail-grid">
    {ids.map((id, index) => <Fragment key={`${id}-${index}`}><dt>Rule {index + 1}</dt><dd>{id}</dd></Fragment>)}
    {changes.map((change, index) => <Fragment key={`${change.ruleId}-${index}`}><dt>{change.field}</dt><dd>{change.before} → {change.after} <span className="muted-copy">({change.ruleId})</span></dd></Fragment>)}
  </dl></section>;
}

function InspectorHeaders({ title, headers }: { title: string; headers: HeaderValue[] }) {
  return <section className="inspector-section"><h3>{title}</h3>{headers.length === 0 ? <p className="muted-copy">No headers captured.</p> : <div className="header-table">{headers.map((header, index) => <div className="header-row" key={`${header.name}-${index}`}><strong>{header.name}</strong><span className={header.sensitive ? "redacted-value" : ""}>{header.sensitive ? "<redacted>" : header.value}</span></div>)}</div>}</section>;
}

function InspectorTiming({ detail }: { detail: FlowDetail }) {
  const timing = detail.timing;
  return <section className="inspector-section"><h3>Timing</h3><div className="timing-grid"><span>Request <strong>{formatMs(timing.requestMs)}</strong></span><span>Server <strong>{formatMs(timing.serverMs)}</strong></span><span>Download <strong>{formatMs(timing.downloadMs)}</strong></span><span>Total <strong>{formatMs(timing.totalMs)}</strong></span></div></section>;
}

async function loadBody(body: BodyRef | null) { if (!body) return null; return invoke<BodyPayload>("read_body", { sha256: body.sha256 }); }
function formatMs(value: number | null) { return value == null ? "—" : `${value} ms`; }
function formatClock(value: string) { const millis = Number(value); return Number.isFinite(millis) && millis > 0 ? new Date(millis).toLocaleTimeString() : value; }
function formatInvokeError(value: unknown) { if (typeof value === "string") return value; if (value && typeof value === "object" && "message" in value) return String((value as { message: unknown }).message); return String(value); }
