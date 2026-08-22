import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useMemo, useState } from "react";
import type {
  BodyPayload,
  FlowDetail,
  FlowSummary,
  HeaderValue,
  ReplayDraft,
} from "../types";

const COMMON_METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
const SENSITIVE_HEADERS = new Set([
  "authorization",
  "proxy-authorization",
  "cookie",
  "set-cookie",
  "x-api-key",
  "api-key",
  "x-auth-token",
]);

export function ReplayView() {
  const [flows, setFlows] = useState<FlowSummary[]>([]);
  const [sourceFlowId, setSourceFlowId] = useState<string>("");
  const [method, setMethod] = useState("GET");
  const [url, setUrl] = useState("");
  const [headersText, setHeadersText] = useState("");
  const [bodyText, setBodyText] = useState("");
  const [sourceSessionId, setSourceSessionId] = useState<string | null>(null);
  const [sourceMessage, setSourceMessage] = useState("Start blank or load a captured request.");
  const [result, setResult] = useState<FlowDetail | null>(null);
  const [resultBody, setResultBody] = useState<BodyPayload | null>(null);
  const [sending, setSending] = useState(false);
  const [loadingSource, setLoadingSource] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refreshFlows = useCallback(async () => {
    try {
      const next = await invoke<FlowSummary[]>("list_flows");
      setFlows(next);
      setError(null);
    } catch (value) {
      setError(formatInvokeError(value));
    }
  }, []);

  useEffect(() => {
    void refreshFlows();
  }, [refreshFlows]);

  const replayableFlows = useMemo(
    () => flows.filter((flow) => flow.source !== "fixture"),
    [flows],
  );

  async function loadSource() {
    if (!sourceFlowId) {
      resetDraft();
      return;
    }

    setLoadingSource(true);
    try {
      const detail = await invoke<FlowDetail | null>("get_flow_detail", { flowId: sourceFlowId });
      if (!detail?.request) {
        throw new Error("This flow does not have captured request details.");
      }

      setMethod(detail.request.method.toUpperCase());
      setUrl(detail.request.url);
      setHeadersText(headersToText(detail.request.headers));
      setSourceSessionId(detail.summary.sessionId);
      setBodyText("");

      if (detail.request.body) {
        if (detail.request.body.isBinary) {
          setSourceMessage("Loaded request. Binary request bodies are not editable in v0.1, so the body was left blank.");
        } else {
          const payload = await invoke<BodyPayload>("read_body", {
            sha256: detail.request.body.sha256,
          });
          setBodyText(payload.text ?? "");
          setSourceMessage(`Loaded ${detail.request.method} ${detail.request.host}${detail.request.path}`);
        }
      } else {
        setSourceMessage(`Loaded ${detail.request.method} ${detail.request.host}${detail.request.path}`);
      }

      setResult(null);
      setResultBody(null);
      setError(null);
    } catch (value) {
      setError(formatInvokeError(value));
    } finally {
      setLoadingSource(false);
    }
  }

  function resetDraft() {
    setSourceFlowId("");
    setMethod("GET");
    setUrl("");
    setHeadersText("");
    setBodyText("");
    setSourceSessionId(null);
    setSourceMessage("Blank request.");
    setResult(null);
    setResultBody(null);
    setError(null);
  }

  async function sendReplay() {
    if (!url.trim()) {
      setError("Enter an absolute HTTP or HTTPS URL before sending.");
      return;
    }

    let headers: HeaderValue[];
    try {
      headers = parseHeaders(headersText);
    } catch (value) {
      setError(formatInvokeError(value));
      return;
    }

    const draft: ReplayDraft = {
      sourceFlowId: sourceFlowId || null,
      sessionId: sourceSessionId,
      method: method.trim().toUpperCase() || "GET",
      url: url.trim(),
      headers,
      bodyText: bodyText.length > 0 ? bodyText : null,
    };

    setSending(true);
    setResult(null);
    setResultBody(null);
    try {
      const replayed = await invoke<FlowDetail>("execute_replay", { draft });
      setResult(replayed);
      if (replayed.response?.body) {
        setResultBody(
          await invoke<BodyPayload>("read_body", {
            sha256: replayed.response.body.sha256,
          }),
        );
      }
      await refreshFlows();
      setError(null);
    } catch (value) {
      setError(formatInvokeError(value));
    } finally {
      setSending(false);
    }
  }

  return (
    <section className="replay-layout">
      <div className="panel replay-editor">
        <div className="panel-heading">
          <div>
            <strong>Request composer</strong>
            <span>{sourceMessage}</span>
          </div>
          <button className="secondary compact" onClick={resetDraft} disabled={sending || loadingSource}>
            New blank
          </button>
        </div>

        {error ? <div className="error-banner">{error}</div> : null}

        <div className="replay-source-row">
          <select
            className="select-input"
            value={sourceFlowId}
            onChange={(event) => setSourceFlowId(event.target.value)}
            disabled={sending || loadingSource}
          >
            <option value="">Blank request</option>
            {replayableFlows.map((flow) => (
              <option value={flow.id} key={flow.id}>
                {flow.method} · {flow.host}{flow.path} {flow.statusCode ? `· ${flow.statusCode}` : ""}
              </option>
            ))}
          </select>
          <button
            className="secondary compact"
            onClick={() => void loadSource()}
            disabled={!sourceFlowId || sending || loadingSource}
          >
            {loadingSource ? "Loading…" : "Load captured request"}
          </button>
          <button className="secondary compact" onClick={() => void refreshFlows()} disabled={sending}>
            Refresh sources
          </button>
        </div>

        <div className="replay-request-line">
          <select
            className="select-input method-select"
            value={method}
            onChange={(event) => setMethod(event.target.value)}
            disabled={sending}
          >
            {COMMON_METHODS.map((value) => <option key={value}>{value}</option>)}
          </select>
          <input
            className="text-input replay-url"
            placeholder="https://api.example.com/v1/resource"
            value={url}
            onChange={(event) => setUrl(event.target.value)}
            disabled={sending}
          />
          <button className="primary replay-send" onClick={() => void sendReplay()} disabled={sending || !url.trim()}>
            {sending ? "Sending…" : "Send"}
          </button>
        </div>

        <div className="replay-fields">
          <label>
            <span>Headers</span>
            <small>One header per line: Name: Value. Sensitive values stay local and are redacted in safe cURL exports.</small>
            <textarea
              className="replay-textarea headers-editor"
              placeholder={"Accept: application/json\nAuthorization: Bearer …"}
              value={headersText}
              onChange={(event) => setHeadersText(event.target.value)}
              spellCheck={false}
              disabled={sending}
            />
          </label>
          <label>
            <span>Body</span>
            <small>Text/JSON request body. Leave empty for requests without a body.</small>
            <textarea
              className="replay-textarea body-editor"
              placeholder={'{\n  "example": true\n}'}
              value={bodyText}
              onChange={(event) => setBodyText(event.target.value)}
              spellCheck={false}
              disabled={sending}
            />
          </label>
        </div>
      </div>

      <div className="panel replay-result">
        <div className="panel-heading">
          <div>
            <strong>Replay result</strong>
            <span>{result ? "Persisted to Traffic as a replay flow" : "Send a request to see its response"}</span>
          </div>
          {result ? <span className="pill">{result.summary.durationMs ?? "—"} ms</span> : null}
        </div>

        {result ? (
          <div className="replay-result-scroll">
            <section className="replay-result-summary">
              <span className="replay-status">{result.response?.statusCode ?? "ERR"}</span>
              <div>
                <strong>{result.request?.method} {result.request?.url}</strong>
                <small>{result.response?.reason ?? result.errorMessage ?? "Response received"}</small>
              </div>
            </section>

            <section className="replay-result-section">
              <h3>Response headers</h3>
              <div className="header-table">
                {(result.response?.headers ?? []).map((header, index) => (
                  <div className="header-row" key={`${header.name}-${index}`}>
                    <code>{header.name}</code>
                    <code>{header.sensitive ? "•••••••• (sensitive)" : header.value}</code>
                  </div>
                ))}
              </div>
            </section>

            <section className="replay-result-section">
              <h3>Response body</h3>
              {resultBody?.text != null ? (
                <pre className="code-block replay-response-body">
                  {prettyBody(resultBody.text, result.response?.body?.contentType)}
                </pre>
              ) : null}
              {resultBody?.base64 != null ? (
                <pre className="code-block replay-response-body">Binary body · base64{"\n"}{resultBody.base64}</pre>
              ) : null}
              {!result.response?.body ? <span className="muted">No response body.</span> : null}
            </section>
          </div>
        ) : (
          <div className="replay-empty-state">
            <strong>Native replay</strong>
            <p>The request is sent by the Rust core, so the desktop webview is not constrained by browser CORS.</p>
            <p>Every successful replay is saved with source <code>replay</code> and can be inspected from Traffic.</p>
          </div>
        )}
      </div>
    </section>
  );
}

function headersToText(headers: HeaderValue[]) {
  return headers.map((header) => `${header.name}: ${header.value}`).join("\n");
}

function parseHeaders(text: string): HeaderValue[] {
  return text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line, index) => {
      const separator = line.indexOf(":");
      if (separator <= 0) {
        throw new Error(`Header line ${index + 1} must use the format Name: Value.`);
      }
      const name = line.slice(0, separator).trim();
      const value = line.slice(separator + 1).trim();
      if (!name) {
        throw new Error(`Header line ${index + 1} is missing a name.`);
      }
      return {
        name,
        value,
        sensitive: SENSITIVE_HEADERS.has(name.toLowerCase()),
      };
    });
}

function prettyBody(text: string, contentType?: string | null) {
  if (!contentType?.toLowerCase().includes("json")) return text;
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return text;
  }
}

function formatInvokeError(value: unknown) {
  if (typeof value === "string") return value;
  if (value && typeof value === "object" && "message" in value) {
    return String((value as { message: unknown }).message);
  }
  return String(value);
}
