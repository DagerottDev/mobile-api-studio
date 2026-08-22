import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";
import { AiSettingsPanel } from "./components/AiSettingsPanel";
import { AiView } from "./components/AiView";
import { CompareView } from "./components/CompareView";
import { ConnectView } from "./components/ConnectView";
import { MocksView } from "./components/MocksView";
import { MockUtilitiesView } from "./components/MockUtilitiesView";
import { ReplayView } from "./components/ReplayView";
import { SdkView } from "./components/SdkView";
import { SettingsView } from "./components/SettingsView";
import { SidecarSettingsPanel } from "./components/SidecarSettingsPanel";
import { TrafficView } from "./components/TrafficView";
import { WorkspaceView } from "./components/WorkspaceView";
import type { CaptureSession } from "./types";

type Route = "Connect" | "Traffic" | "Replay" | "Mocks" | "Compare" | "AI" | "SDK" | "Workspace" | "Settings";

const routes: Route[] = ["Connect", "Traffic", "Replay", "Mocks", "Compare", "AI", "SDK", "Workspace", "Settings"];

function App() {
  const [route, setRoute] = useState<Route>("Connect");
  const [health, setHealth] = useState("checking Rust core…");
  const [sessions, setSessions] = useState<CaptureSession[]>([]);
  const [replaySavedRequestId, setReplaySavedRequestId] = useState<string | null>(null);

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

  function openSavedRequest(requestId: string) {
    setReplaySavedRequestId(requestId);
    setRoute("Replay");
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
                    ? "Edit and resend captured or saved requests"
                    : route === "Mocks"
                      ? "Override responses, reuse fixtures, and pause live mobile API calls"
                      : route === "Compare"
                        ? "Compare sessions deterministically across calls, payloads, timing, and app context"
                        : route === "AI"
                          ? "Preview redacted evidence, then optionally ask AI to explain a session diff or captured flow"
                          : route === "SDK"
                            ? "Connect app context, logs, screens, features, and source locations to network flows"
                            : route === "Workspace"
                              ? "Manage sessions, saved requests, and environments"
                              : "Connection Doctor, onboarding, backup, restore, capture setup, and optional AI provider settings"}
            </p>
          </div>
        </header>

        {route === "Connect" ? <ConnectView /> : null}
        {route === "Traffic" ? <TrafficView /> : null}
        {route === "Replay" ? <ReplayView savedRequestId={replaySavedRequestId} /> : null}
        {route === "Mocks" ? <div className="mocks-page-stack"><MocksView /><MockUtilitiesView /></div> : null}
        {route === "Compare" ? <CompareView /> : null}
        {route === "AI" ? <AiView /> : null}
        {route === "SDK" ? <SdkView /> : null}
        {route === "Workspace" ? <WorkspaceView onOpenReplay={openSavedRequest} /> : null}
        {route === "Settings" ? (
          <>
            <SettingsView />
            <SidecarSettingsPanel />
            <AiSettingsPanel />
          </>
        ) : null}
      </main>
    </div>
  );
}

export default App;
