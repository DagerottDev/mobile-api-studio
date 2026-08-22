import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";
import { ConnectView } from "./components/ConnectView";
import { ReplayView } from "./components/ReplayView";
import { TrafficView } from "./components/TrafficView";
import type { CaptureSession } from "./types";

type Route = "Connect" | "Traffic" | "Replay" | "Settings";
const routes: Route[] = ["Connect", "Traffic", "Replay", "Settings"];

function App() {
  const [route, setRoute] = useState<Route>("Connect");
  const [health, setHealth] = useState("checking Rust core…");
  const [sessions, setSessions] = useState<CaptureSession[]>([]);

  const refreshSessions = useCallback(async () => {
    try {
      setSessions(await invoke<CaptureSession[]>("list_sessions"));
    } catch {
      // Route-level views surface actionable command failures.
    }
  }, []);

  useEffect(() => {
    invoke<string>("health")
      .then(setHealth)
      .catch((error) => setHealth(`Rust unavailable: ${String(error)}`));
    void refreshSessions();
  }, [refreshSessions]);

  useEffect(() => {
    if (route !== "Connect" && route !== "Traffic" && route !== "Replay") return;
    const timer = window.setInterval(() => void refreshSessions(), 2000);
    return () => window.clearInterval(timer);
  }, [route, refreshSessions]);

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
            <p>
              {route === "Connect"
                ? "Discover and connect local mobile runtimes"
                : route === "Traffic"
                  ? "Inspect captured and replayed API traffic"
                  : route === "Replay"
                    ? "Edit and resend captured API requests"
                    : "Local application preferences"}
            </p>
          </div>
        </header>

        {route === "Connect" ? <ConnectView /> : null}
        {route === "Traffic" ? <TrafficView /> : null}
        {route === "Replay" ? <ReplayView /> : null}
        {route === "Settings" ? (
          <section className="placeholder panel">
            <span className="eyebrow">Settings</span>
            <h2>Capture preferences arrive with the v0.2 workflow layer.</h2>
            <p>Phase 1 keeps connection behavior local-first, explicit, and reversible by default.</p>
          </section>
        ) : null}
      </main>
    </div>
  );
}

export default App;
