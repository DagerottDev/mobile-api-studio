import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useMemo, useState } from "react";
import { ConnectView } from "./components/ConnectView";
import type { CaptureSession, FlowSummary } from "./types";

type Route = "Connect" | "Traffic" | "Replay" | "Settings";

const routes: Route[] = ["Connect", "Traffic", "Replay", "Settings"];

function App() {
  const [route, setRoute] = useState<Route>("Connect");
  const [health, setHealth] = useState("checking Rust core…");
  const [flows, setFlows] = useState<FlowSummary[]>([]);
  const [sessions, setSessions] = useState<CaptureSession[]>([]);
  const [selectedFlowId, setSelectedFlowId] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  const refreshFlows = useCallback(async () => {
    const result = await invoke<FlowSummary[]>("list_flows");
    setFlows(result);
    setSelectedFlowId((current) => current ?? result[0]?.id ?? null);
  }, []);

  const refreshSessions = useCallback(async () => {
    setSessions(await invoke<CaptureSession[]>("list_sessions"));
  }, []);

  useEffect(() => {
    invoke<string>("health")
      .then(setHealth)
      .catch((error) => setHealth(`Rust unavailable: ${String(error)}`));

    Promise.all([refreshFlows(), refreshSessions()])
      .then(() => setLoadError(null))
      .catch((error) => setLoadError(String(error)));
  }, [refreshFlows, refreshSessions]);

  useEffect(() => {
    if (route !== "Traffic") return;

    const refresh = () => {
      void Promise.all([refreshFlows(), refreshSessions()]).catch((error) =>
        setLoadError(String(error)),
      );
    };

    refresh();
    const timer = window.setInterval(refresh, 1000);
    return () => window.clearInterval(timer);
  }, [route, refreshFlows, refreshSessions]);

  const selectedFlow = useMemo(
    () => flows.find((flow) => flow.id === selectedFlowId) ?? null,
    [flows, selectedFlowId],
  );

  async function manualRefresh() {
    try {
      await Promise.all([refreshFlows(), refreshSessions()]);
      setLoadError(null);
    } catch (error) {
      setLoadError(String(error));
    }
  }

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-mark">M</span>
          <div>
            <strong>Mobile API Studio</strong>
            <small>pre-alpha</small>
          </div>
        </div>

        <nav>
          {routes.map((item) => (
            <button
              className={route === item ? "nav-item active" : "nav-item"}
              key={item}
              onClick={() => setRoute(item)}
            >
              {item}
            </button>
          ))}
        </nav>

        <div className="sidebar-metric">
          <span>Sessions</span>
          <strong>{sessions.length}</strong>
        </div>

        <div className="core-status">
          <span className="status-dot" />
          <span>{health}</span>
        </div>
      </aside>

      <main className="workspace">
        <header className="toolbar">
          <div>
            <h1>{route}</h1>
            <p>{route === "Connect" ? "Discover and connect local mobile runtimes" : "Mobile API Studio"}</p>
          </div>
          {route === "Traffic" ? (
            <button className="secondary" onClick={() => void manualRefresh()}>
              Refresh traffic
            </button>
          ) : null}
        </header>

        {route === "Connect" ? <ConnectView /> : null}

        {route === "Traffic" ? (
          <section className="traffic-layout">
            <div className="traffic-list panel">
              <div className="panel-heading">
                <div>
                  <strong>Captured traffic</strong>
                  <span>{flows.length} persisted flows · live refresh every second</span>
                </div>
                <span className="pill">mitmdump → SQLite</span>
              </div>

              {loadError ? <div className="error-banner">{loadError}</div> : null}

              <div className="flow-header flow-grid">
                <span>Method</span>
                <span>Host / Path</span>
                <span>Status</span>
                <span>Time</span>
              </div>

              <div className="flow-rows">
                {flows.map((flow) => (
                  <button
                    key={flow.id}
                    className={
                      flow.id === selectedFlowId
                        ? "flow-row flow-grid selected"
                        : "flow-row flow-grid"
                    }
                    onClick={() => setSelectedFlowId(flow.id)}
                  >
                    <span className={`method method-${flow.method.toLowerCase()}`}>
                      {flow.method}
                    </span>
                    <span className="endpoint">
                      <strong>{flow.host}</strong>
                      <small>{flow.path}</small>
                    </span>
                    <span>{flow.statusCode ?? "—"}</span>
                    <span>{flow.durationMs ? `${flow.durationMs} ms` : "—"}</span>
                  </button>
                ))}
              </div>
            </div>

            <div className="inspector panel">
              <div className="panel-heading">
                <div>
                  <strong>Inspector</strong>
                  <span>normalized captured flow</span>
                </div>
              </div>

              {selectedFlow ? (
                <dl className="detail-grid">
                  <dt>Flow ID</dt>
                  <dd>{selectedFlow.id}</dd>
                  <dt>Session</dt>
                  <dd>{selectedFlow.sessionId ?? "unassigned"}</dd>
                  <dt>Source</dt>
                  <dd>{selectedFlow.source}</dd>
                  <dt>Method</dt>
                  <dd>{selectedFlow.method}</dd>
                  <dt>Host</dt>
                  <dd>{selectedFlow.host}</dd>
                  <dt>Path</dt>
                  <dd>{selectedFlow.path}</dd>
                  <dt>Status</dt>
                  <dd>{selectedFlow.statusCode ?? "pending"}</dd>
                  <dt>Started</dt>
                  <dd>{selectedFlow.startedAt}</dd>
                </dl>
              ) : (
                <p className="empty-state">No captured flows yet. Start a capture from Connect.</p>
              )}
            </div>
          </section>
        ) : null}

        {route === "Replay" || route === "Settings" ? (
          <section className="placeholder panel">
            <span className="eyebrow">{route}</span>
            <h2>{route} implementation follows request/response detail capture.</h2>
            <p>
              The current Phase 1 slice now handles runtime discovery, connection lifecycle,
              proxy capture and persisted flow summaries. Detailed request/response inspection
              and replay are next.
            </p>
          </section>
        ) : null}
      </main>
    </div>
  );
}

export default App;
