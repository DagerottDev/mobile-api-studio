import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useState } from "react";
import type { FlowSummary } from "./types";

type Route = "Connect" | "Traffic" | "Replay" | "Settings";

const routes: Route[] = ["Connect", "Traffic", "Replay", "Settings"];

function App() {
  const [route, setRoute] = useState<Route>("Traffic");
  const [health, setHealth] = useState("checking Rust core…");
  const [flows, setFlows] = useState<FlowSummary[]>([]);
  const [selectedFlowId, setSelectedFlowId] = useState<string | null>(null);

  useEffect(() => {
    invoke<string>("health")
      .then(setHealth)
      .catch((error) => setHealth(`Rust unavailable: ${String(error)}`));

    invoke<FlowSummary[]>("list_fake_flows")
      .then((result) => {
        setFlows(result);
        setSelectedFlowId(result[0]?.id ?? null);
      })
      .catch(() => setFlows([]));
  }, []);

  const selectedFlow = useMemo(
    () => flows.find((flow) => flow.id === selectedFlowId) ?? null,
    [flows, selectedFlowId],
  );

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

        <div className="core-status">
          <span className="status-dot" />
          <span>{health}</span>
        </div>
      </aside>

      <main className="workspace">
        <header className="toolbar">
          <div>
            <h1>{route}</h1>
            <p>Phase 0 foundation</p>
          </div>
          <button className="primary" disabled>
            Connect device
          </button>
        </header>

        {route === "Traffic" ? (
          <section className="traffic-layout">
            <div className="traffic-list panel">
              <div className="panel-heading">
                <div>
                  <strong>Fixture traffic</strong>
                  <span>{flows.length} flows</span>
                </div>
                <span className="pill">fake capture</span>
              </div>

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
                  <span>normalized Flow model</span>
                </div>
              </div>

              {selectedFlow ? (
                <dl className="detail-grid">
                  <dt>Flow ID</dt>
                  <dd>{selectedFlow.id}</dd>
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
                <p className="empty-state">
                  No flows yet. Phase 0 will next persist fixture events in SQLite.
                </p>
              )}
            </div>
          </section>
        ) : (
          <section className="placeholder panel">
            <span className="eyebrow">{route}</span>
            <h2>{route} is intentionally minimal in Phase 0.</h2>
            <p>
              We are proving the Rust/Tauri/domain-model pipeline before adding
              simulator, emulator, proxy, replay, and settings workflows.
            </p>
          </section>
        )}
      </main>
    </div>
  );
}

export default App;
