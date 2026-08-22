import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useMemo, useState } from "react";
import type {
  BodyPayload,
  BodyRef,
  FlowDetail,
  FlowSummary,
  HeaderValue,
} from "../types";

type StatusFilter = "all" | "2xx" | "3xx" | "4xx" | "5xx";

export function TrafficView() {
  const [flows, setFlows] = useState<FlowSummary[]>([]);
  const [selectedFlowId, setSelectedFlowId] = useState<string | null>(null);
  const [detail, setDetail] = useState<FlowDetail | null>(null);
  const [requestBody, setRequestBody] = useState<BodyPayload | null>(null);
  const [responseBody, setResponseBody] = useState<BodyPayload | null>(null);
  const [hostFilter, setHostFilter] = useState("");
  const [methodFilter, setMethodFilter] = useState("all");
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("all");
  const [error, setError] = useState<string | null>(null);
  const [curlPreview, setCurlPreview] = useState<string | null>(null);
  const [curlCopied, setCurlCopied] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const result = await invoke<FlowSummary[]>("list_flows");
      setFlows(result);
      setSelectedFlowId((current) => {
        if (current && result.some((flow) => flow.id === current)) return current;
        return result[0]?.id ?? null;
      });
      setError(null);
    } catch (value) {
      setError(formatInvokeError(value));
    }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), 1000);
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
        const nextDetail = await invoke<FlowDetail | null>("get_flow_detail", {
          flowId: selectedFlowId,
        });
        if (cancelled) return;
        setDetail(nextDetail);
        setCurlPreview(null);
        setCurlCopied(false);

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
    return () => {
      cancelled = true;
    };
  }, [selectedFlowId]);

  const methods = useMemo(
    () => Array.from(new Set(flows.map((flow) => flow.method))).sort(),
    [flows],
  );

  const filteredFlows = useMemo(() => {
    const hostNeedle = hostFilter.trim().toLowerCase();
    return flows.filter((flow) => {
      if (hostNeedle && !`${flow.host}${flow.path}`.toLowerCase().includes(hostNeedle)) return false;
      if (methodFilter !== "all" && flow.method !== methodFilter) return false;
      if (statusFilter !== "all") {
        const status = flow.statusCode ?? 0;
        if (Math.floor(status / 100) !== Number(statusFilter[0])) return false;
      }
      return true;
    });
  }, [flows, hostFilter, methodFilter, statusFilter]);

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

  return (
    <section className="traffic-layout">
      <div className="traffic-list panel">
        <div className="panel-heading">
          <div>
            <strong>Captured traffic</strong>
            <span>{filteredFlows.length} visible · {flows.length} stored · live refresh</span>
          </div>
          <button className="secondary compact" onClick={() => void refresh()}>
            Refresh
          </button>
        </div>

        <div className="traffic-filters">
          <input
            className="text-input"
            value={hostFilter}
            onChange={(event) => setHostFilter(event.target.value)}
            placeholder="Filter host or path"
          />
          <select value={methodFilter} onChange={(event) => setMethodFilter(event.target.value)}>
            <option value="all">All methods</option>
            {methods.map((method) => <option key={method} value={method}>{method}</option>)}
          </select>
          <select value={statusFilter} onChange={(event) => setStatusFilter(event.target.value as StatusFilter)}>
            <option value="all">All statuses</option>
            <option value="2xx">2xx</option>
            <option value="3xx">3xx</option>
            <option value="4xx">4xx</option>
            <option value="5xx">5xx</option>
          </select>
        </div>

        {error ? <div className="error-banner">{error}</div> : null}

        <div className="flow-header flow-grid">
          <span>Method</span>
          <span>Host / Path</span>
          <span>Status</span>
          <span>Time</span>
        </div>

        <div className="flow-rows">
          {filteredFlows.map((flow) => (
            <button
              key={flow.id}
              className={flow.id === selectedFlowId ? "flow-row flow-grid selected" : "flow-row flow-grid"}
              onClick={() => setSelectedFlowId(flow.id)}
            >
              <span className={`method method-${flow.method.toLowerCase()}`}>{flow.method}</span>
              <span className="endpoint">
                <strong>{flow.host}</strong>
                <small>{flow.path}</small>
              </span>
              <span>{flow.statusCode ?? "—"}</span>
              <span>{flow.durationMs != null ? `${flow.durationMs} ms` : "—"}</span>
            </button>
          ))}
          {filteredFlows.length === 0 ? <p className="empty-state">No flows match these filters.</p> : null}
        </div>
      </div>

      <div className="inspector panel">
        <div className="panel-heading inspector-heading">
          <div>
            <strong>Inspector</strong>
            <span>{detail?.request?.url ?? "Select a captured flow"}</span>
          </div>
          {detail?.request ? (
            <button className="secondary compact" onClick={() => void copyCurl()}>
              {curlCopied ? "Copied cURL" : "Copy safe cURL"}
            </button>
          ) : null}
        </div>

        {detail ? (
          <div className="inspector-scroll">
            <InspectorSummary detail={detail} />
            <InspectorHeaders title="Request headers" headers={detail.request?.headers ?? []} />
            <InspectorBody title="Request body" bodyRef={detail.request?.body ?? null} payload={requestBody} />
            <InspectorHeaders title="Response headers" headers={detail.response?.headers ?? []} />
            <InspectorBody title="Response body" bodyRef={detail.response?.body ?? null} payload={responseBody} />
            <InspectorTiming detail={detail} />
            {curlPreview ? (
              <section className="inspector-section">
                <h3>Safe cURL</h3>
                <pre>{curlPreview}</pre>
              </section>
            ) : null}
          </div>
        ) : (
          <p className="empty-state">No full detail is available for this flow yet.</p>
        )}
      </div>
    </section>
  );
}

function InspectorSummary({ detail }: { detail: FlowDetail }) {
  return (
    <section className="inspector-section">
      <h3>Overview</h3>
      <dl className="detail-grid compact-detail-grid">
        <dt>Method</dt><dd>{detail.request?.method ?? detail.summary.method}</dd>
        <dt>Status</dt><dd>{detail.response?.statusCode ?? detail.summary.statusCode ?? "pending"}</dd>
        <dt>URL</dt><dd>{detail.request?.url ?? `${detail.summary.host}${detail.summary.path}`}</dd>
        <dt>Session</dt><dd>{detail.summary.sessionId ?? "unassigned"}</dd>
        <dt>Source</dt><dd>{detail.summary.source}</dd>
        <dt>Total</dt><dd>{formatMs(detail.timing.totalMs)}</dd>
      </dl>
    </section>
  );
}

function InspectorHeaders({ title, headers }: { title: string; headers: HeaderValue[] }) {
  return (
    <section className="inspector-section">
      <h3>{title}</h3>
      {headers.length === 0 ? <p className="muted-copy">No headers captured.</p> : (
        <div className="header-table">
          {headers.map((header, index) => (
            <div className="header-row" key={`${header.name}-${index}`}>
              <strong>{header.name}</strong>
              <span className={header.sensitive ? "redacted-value" : ""}>
                {header.sensitive ? "<redacted>" : header.value}
              </span>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

function InspectorBody({
  title,
  bodyRef,
  payload,
}: {
  title: string;
  bodyRef: BodyRef | null;
  payload: BodyPayload | null;
}) {
  if (!bodyRef) {
    return (
      <section className="inspector-section">
        <h3>{title}</h3>
        <p className="muted-copy">No body captured.</p>
      </section>
    );
  }

  const raw = payload?.text ?? payload?.base64 ?? "Loading body…";
  const maxDisplay = 100_000;
  const rendered = raw.length > maxDisplay ? `${raw.slice(0, maxDisplay)}\n… UI preview truncated …` : raw;

  return (
    <section className="inspector-section">
      <div className="section-title-row">
        <h3>{title}</h3>
        <span>{bodyRef.contentType ?? "unknown"} · {bodyRef.byteSize} bytes{bodyRef.isTruncated ? " · capture truncated" : ""}</span>
      </div>
      <pre>{bodyRef.isBinary && payload?.base64 ? `Base64\n${rendered}` : rendered}</pre>
    </section>
  );
}

function InspectorTiming({ detail }: { detail: FlowDetail }) {
  const timing = detail.timing;
  return (
    <section className="inspector-section">
      <h3>Timing</h3>
      <div className="timing-grid">
        <span>Request <strong>{formatMs(timing.requestMs)}</strong></span>
        <span>Server <strong>{formatMs(timing.serverMs)}</strong></span>
        <span>Download <strong>{formatMs(timing.downloadMs)}</strong></span>
        <span>Total <strong>{formatMs(timing.totalMs)}</strong></span>
      </div>
    </section>
  );
}

async function loadBody(body: BodyRef | null) {
  if (!body) return null;
  return invoke<BodyPayload>("read_body", { sha256: body.sha256 });
}

function formatMs(value: number | null) {
  return value == null ? "—" : `${value} ms`;
}

function formatInvokeError(value: unknown) {
  if (typeof value === "string") return value;
  if (value && typeof value === "object" && "message" in value) {
    return String((value as { message: unknown }).message);
  }
  return String(value);
}
