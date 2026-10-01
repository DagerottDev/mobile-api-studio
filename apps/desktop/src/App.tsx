import { invoke } from "./api/invoke";
import { useCallback, useEffect, useState } from "react";
import { AiSettingsPanel } from "./components/AiSettingsPanel";
import { AiView } from "./components/AiView";
import { CompareView } from "./components/CompareView";
import { ConnectView } from "./components/ConnectView";
import { MocksView } from "./components/MocksView";
import { NetworkProfilesView } from "./components/NetworkProfilesView";
import { ProxyRulesView } from "./components/ProxyRulesView";
import { MockUtilitiesView } from "./components/MockUtilitiesView";
import { ReplayView } from "./components/ReplayView";
import { SdkView } from "./components/SdkView";
import { SettingsView } from "./components/SettingsView";
import { SidecarSettingsPanel } from "./components/SidecarSettingsPanel";
import { TrafficView } from "./components/TrafficView";
import { WorkspaceView } from "./components/WorkspaceView";
import type { CaptureSession, ConnectionSnapshot, NetworkProfile } from "./types";

type Route = "Connect" | "Traffic" | "Network" | "Replay" | "Mocks" | "Compare" | "AI" | "SDK" | "Workspace" | "Settings";

const routes: Route[] = ["Connect", "Traffic", "Network", "Replay", "Mocks", "Compare", "AI", "SDK", "Workspace", "Settings"];
const paths = Object.fromEntries(routes.map((name) => [name, `/${name.toLowerCase()}`])) as Record<Route, string>;
const groups: { title: string; items: Route[] }[] = [
  { title: "Capture", items: ["Connect", "Traffic", "Network"] },
  { title: "Investigate", items: ["Replay", "Mocks", "Compare", "AI", "SDK"] },
  { title: "Organize", items: ["Workspace", "Settings"] },
];
function errorText(value: unknown) { return value instanceof Error ? value.message : String(typeof value === "object" && value !== null && "message" in value ? value.message : value); }
function routeFromPath(): Route {
  return routes.find((name) => paths[name] === window.location.pathname) ?? "Connect";
}

function App() {
  const [route, setRoute] = useState<Route>(routeFromPath);
  const [health, setHealth] = useState("checking Rust core…");
  const [sessions, setSessions] = useState<CaptureSession[]>([]);
  const [replaySavedRequestId, setReplaySavedRequestId] = useState<string | null>(null);
  const [connection, setConnection] = useState<ConnectionSnapshot | null>(null);
  const [networkProfiles, setNetworkProfiles] = useState<NetworkProfile[]>([]);
  const [networkProfileError, setNetworkProfileError] = useState<string | null>(null);
  const [networkProfileMessage, setNetworkProfileMessage] = useState<string | null>(null);

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

  const refreshNetworkProfiles = useCallback(async () => {
    try {
      setNetworkProfiles(await invoke<NetworkProfile[]>("list_network_profiles"));
      setNetworkProfileError(null);
    } catch (error) {
      setNetworkProfileError(errorText(error));
    }
  }, []);

  async function disableAllNetworkProfiles() {
    try {
      const count = await invoke<number>("disable_all_network_profiles");
      setNetworkProfileError(null);
      setNetworkProfileMessage(`Disabled ${count} network profile${count === 1 ? "" : "s"}.`);
      await refreshNetworkProfiles();
    } catch (error) {
      setNetworkProfileMessage(null);
      setNetworkProfileError(errorText(error));
    }
  }

  useEffect(() => {
    invoke<string>("health")
      .then(setHealth)
      .catch((error) => setHealth(`Local service unavailable: ${errorText(error)}`));
    void refreshSessions();
    void refreshNetworkProfiles();
    const refreshConnection = () => invoke<ConnectionSnapshot>("current_connection").then(setConnection).catch(() => setConnection(null));
    void refreshConnection();
    const onLocation = () => setRoute(routeFromPath());
    window.addEventListener("popstate", onLocation);

    const timer = window.setInterval(() => { void refreshSessions(); void refreshConnection(); void refreshNetworkProfiles(); }, 2000);
    return () => { window.clearInterval(timer); window.removeEventListener("popstate", onLocation); };
  }, [refreshSessions, refreshNetworkProfiles]);

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

        <div className={connection?.connected && connection.captureRunning !== false ? "capture-indicator active" : "capture-indicator"} role="status">
          <span className="status-dot" />
          <div><strong>{connection?.connected ? connection.captureRunning === false ? "Capture stopped" : "Capture active" : "No active capture"}</strong>
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
        {networkProfileError ? <div className="error-banner" role="alert">Network profile status: {networkProfileError}</div> : networkProfileMessage ? <div className="settings-message" role="status">{networkProfileMessage}</div> : null}
        {networkProfiles.some((profile) => profile.enabled) ? <div className="panel" role="status" style={{ padding: 14, marginBottom: 14, borderColor: "var(--danger-line)" }}>
          <strong>Network conditions are active</strong>
          <span style={{ marginLeft: 8 }}>{networkProfiles.filter((profile) => profile.enabled).map((profile) => profile.name).join(" · ")}</span>
          <button className="secondary compact" style={{ float: "right" }} onClick={() => void disableAllNetworkProfiles()}>Disable all</button>
        </div> : null}
        <header className="toolbar">
          <div>
            <span className="eyebrow">MOBILE API STUDIO / {route.toUpperCase()}</span>
            <h1>{route === "Connect" ? "Get ready to capture" : route}</h1>
            <p>
              {route === "Connect"
                ? "Discover and connect local mobile runtimes"
                : route === "Traffic"
                  ? "Search and inspect traffic across capture sessions"
                  : route === "Network"
                  ? "Configure latency, bandwidth, offline, and request-failure profiles"
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

        {route === "Connect" ? <ConnectView onOpenTraffic={() => navigate("Traffic")} sharedConnection={connection} /> : null}
        {route === "Traffic" ? <TrafficView onOpenConnect={() => navigate("Connect")} /> : null}
        {route === "Network" ? <NetworkProfilesView profiles={networkProfiles} refresh={refreshNetworkProfiles} /> : null}
        {route === "Replay" ? <ReplayView savedRequestId={replaySavedRequestId} /> : null}
        {route === "Mocks" ? <div className="mocks-page-stack"><ProxyRulesView /><MocksView /><MockUtilitiesView /></div> : null}
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
