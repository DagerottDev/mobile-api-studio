import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";
import { ConnectView } from "./components/ConnectView";
import { ReplayView } from "./components/ReplayView";
import { SettingsView } from "./components/SettingsView";
import { TrafficView } from "./components/TrafficView";
import { WorkspaceView } from "./components/WorkspaceView";
import type { CaptureSession } from "./types";

type Route = "Connect" | "Traffic" | "Replay" | "Workspace" | "Settings";

const routes: Route[] = ["Connect", "Traffic", "Replay", "Workspace", "Settings"];

function App() {
  const [route, setRoute] = useState<Route>("Connect");
  const [health, setHealth] = useState("checking Rust core…");
  const [sessions, setSessions] = useState<CaptureSession[]>([]);

  const refreshSessions = useCallback(async () => {
    try {
      setSessions(await invoke<CaptureSession[]>("list_sessions"));
    } catch {
      // Session count is informational; route-level surfaces handle actionable errors.
    }
  }, []);

  useEffect(() => {
    invoke<string>("health")
      .then(setHealth)
      .catch((error) => setHealth(`Rust unavailable: ${String(error)}`));
    void refreshSessions();

    const timer = window.setInterval(() => void refreshSessions(), 2000);
    return () => window.clearInterval(timer);
  }, [refreshSessions]);

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
          <strong>{sessions.filter((session) => session.status !== "archived").length}</strong>
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
                  ? "Search and inspect traffic across capture sessions"
                  : route === "Replay"
                    ? "Edit and resend captured requests"
                    : route === "Workspace"
                      ? "Manage sessions, saved requests, and environments"
                      : "Connection Doctor, onboarding, backup, and restore"}
            </p>
          </div>
        </header>

        {route === "Connect" ? <ConnectView /> : null}
        {route === "Traffic" ? <TrafficView /> : null}
        {route === "Replay" ? <ReplayView /> : null}
        {route === "Workspace" ? <WorkspaceView /> : null}
        {route === "Settings" ? <SettingsView /> : null}
      </main>
    </div>
  );
}

export default App;
