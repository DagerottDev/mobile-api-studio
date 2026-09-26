import { invoke } from "./api/invoke";
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
import type { CaptureSession, ConnectionSnapshot } from "./types";

type Route = "Connect" | "Traffic" | "Replay" | "Mocks" | "Compare" | "AI" | "SDK" | "Workspace" | "Settings";

const routes: Route[] = ["Connect", "Traffic", "Replay", "Mocks", "Compare", "AI", "SDK", "Workspace", "Settings"];
const paths = Object.fromEntries(routes.map((name) => [name, `/${name.toLowerCase()}`])) as Record<Route, string>;
const groups: { title: string; items: Route[] }[] = [
  { title: "Capture", items: ["Connect", "Traffic"] },
  { title: "Investigate", items: ["Replay", "Mocks", "Compare", "AI", "SDK"] },
  { title: "Organize", items: ["Workspace", "Settings"] },
];
function routeFromPath(): Route {
  return routes.find((name) => paths[name] === window.location.pathname) ?? "Connect";
}

function App() {
  const [route, setRoute] = useState<Route>(routeFromPath);
  const [health, setHealth] = useState("checking Rust core…");
  const [sessions, setSessions] = useState<CaptureSession[]>([]);
  const [replaySavedRequestId, setReplaySavedRequestId] = useState<string | null>(null);
  const [connection, setConnection] = useState<ConnectionSnapshot | null>(null);

  function navigate(next: Route) {
    window.history.pushState({}, "", paths[next]);
    setRoute(next);
  }

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
      .catch((error) => setHealth(`Local service unavailable: ${error instanceof Error ? error.message : String(error?.message ?? error)}`));
    void refreshSessions();
    const refreshConnection = () => invoke<ConnectionSnapshot>("current_connection").then(setConnection).catch(() => setConnection(null));
    void refreshConnection();
    const onLocation = () => setRoute(routeFromPath());
    window.addEventListener("popstate", onLocation);

    const timer = window.setInterval(() => { void refreshSessions(); void refreshConnection(); }, 2000);
    return () => { window.clearInterval(timer); window.removeEventListener("popstate", onLocation); };
  }, [refreshSessions]);

  function openSavedRequest(requestId: string) {
    setReplaySavedRequestId(requestId);
    navigate("Replay");
  }

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-mark" aria-hidden="true">M</span>
          <div>
            <strong>Mobile API Studio</strong>
            <small>LOCAL WORKSPACE</small>
          </div>
        </div>

        <nav aria-label="Main navigation">
          {groups.map((group) => <div className="nav-group" key={group.title}>
            <span className="nav-label">{group.title}</span>
            {group.items.map((item) => (
              <a className={route === item ? "nav-item active" : "nav-item"} href={paths[item]}
                aria-current={route === item ? "page" : undefined} key={item}
                onClick={(event) => { event.preventDefault(); navigate(item); }}>
                {item}
              </a>
            ))}
          </div>)}
        </nav>

        <div className={connection?.connected ? "capture-indicator active" : "capture-indicator"} role="status">
          <span className="status-dot" />
          <div><strong>{connection?.connected ? "Capture active" : "No active capture"}</strong>
          <small>{connection?.connected ? connection.deviceId : "Connect a runtime to begin"}</small></div>
        </div>
        <div className="sidebar-metric">
          <span>Sessions</span>
          <strong>{sessions.filter((session) => session.status !== "archived").length}</strong>
        </div>

        <div className="core-status">
          <span className="status-dot" />
          <span>{health.startsWith("Rust core ready") ? "Local service ready" : health}</span>
        </div>
      </aside>

      <main className="workspace">
        <header className="toolbar">
          <div>
            <span className="eyebrow">MOBILE API STUDIO / {route.toUpperCase()}</span>
            <h1>{route === "Connect" ? "Get ready to capture" : route}</h1>
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

        {route === "Connect" ? <ConnectView onOpenTraffic={() => navigate("Traffic")} /> : null}
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
