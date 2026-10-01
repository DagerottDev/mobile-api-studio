import { invoke } from "../api/invoke";
import { useCallback, useEffect, useMemo, useState } from "react";
import type {
  CaptureTarget,
  ConnectDeviceResult,
  ConnectionDiagnostic,
  ConnectionDoctorReport,
  ConnectionSnapshot,
  Device,
  DeviceDiscoveryPayload,
  LanInterface,
  ListenerMode,
  MacProcess,
  RollbackJournal,
} from "../types";

const disconnected: ConnectionSnapshot = {
  connected: false,
  sessionId: null,
  deviceId: null,
  strategy: null,
  proxyHost: null,
  proxyPort: null,
};

export function ConnectView({ onOpenTraffic, sharedConnection }: { onOpenTraffic: () => void; sharedConnection: ConnectionSnapshot | null }) {
  const [payload, setPayload] = useState<DeviceDiscoveryPayload>({ devices: [], diagnostics: [] });
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [connection, setConnection] = useState<ConnectionSnapshot>(disconnected);
  const [connectionDiagnostics, setConnectionDiagnostics] = useState<ConnectionDiagnostic[]>([]);
  const [pairingToken, setPairingToken] = useState<string | null>(null);
  const [pendingRollback, setPendingRollback] = useState<RollbackJournal | null>(null);
  const [doctor, setDoctor] = useState<ConnectionDoctorReport | null>(null);
  const [sessionName, setSessionName] = useState("");
  const [processes, setProcesses] = useState<MacProcess[]>([]);
  const [selectedProcessPid, setSelectedProcessPid] = useState<number | null>(null);
  const [interfaces, setInterfaces] = useState<LanInterface[]>([]);
  const [selectedInterface, setSelectedInterface] = useState("");
  const [pairedAddress, setPairedAddress] = useState("");
  const [physicalPlatform, setPhysicalPlatform] = useState<"ios" | "android">("ios");
  const [listenerMode, setListenerMode] = useState<ListenerMode["type"]>("reverse_proxy");
  const [listenerUrl, setListenerUrl] = useState("");
  const [listenerPort, setListenerPort] = useState(8185);
  const [loading, setLoading] = useState(true);
  const [acting, setActing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const [devices, current, rollback, report, discoveredProcesses, discoveredInterfaces] = await Promise.all([
        invoke<DeviceDiscoveryPayload>("list_devices"),
        invoke<ConnectionSnapshot>("current_connection"),
        invoke<RollbackJournal | null>("pending_rollback"),
        invoke<ConnectionDoctorReport>("connection_doctor"),
        invoke<MacProcess[]>("list_mac_processes").catch(() => []),
        invoke<LanInterface[]>("list_lan_interfaces").catch(() => []),
      ]);
      setPayload(devices);
      setConnection(current);
      setPendingRollback(rollback);
      setDoctor(report);
      setProcesses(discoveredProcesses);
      setSelectedProcessPid((pid) => discoveredProcesses.some((process) => process.pid === pid) ? pid : discoveredProcesses[0]?.pid ?? null);
      setInterfaces(discoveredInterfaces);
      setSelectedInterface((name) => discoveredInterfaces.some((item) => item.name === name) ? name : discoveredInterfaces[0]?.name ?? "");
      setSelectedId((existing) => {
        if (current.deviceId && devices.devices.some((device) => device.id === current.deviceId)) {
          return current.deviceId;
        }
        if (existing && devices.devices.some((device) => device.id === existing)) {
          return existing;
        }
        return preferredDevice(devices.devices)?.id ?? null;
      });
      setError(null);
    } catch (value) {
      setError(formatInvokeError(value));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // Follow new server snapshots without replaying a pre-action snapshot when acting ends.
  useEffect(() => {
    if (acting || !sharedConnection) return;
    setConnection(sharedConnection);
    if (!sharedConnection.connected) { setConnectionDiagnostics([]); setPairingToken(null); }
  }, [sharedConnection]);

  const selected = useMemo(
    () => payload.devices.find((device) => device.id === selectedId) ?? null,
    [payload.devices, selectedId],
  );

  async function connect() {
    if (!selected) return;
    setActing(true);
    try {
      const result = await invoke<ConnectDeviceResult>("connect_device", {
        deviceId: selected.id,
        sessionName: sessionName.trim() || null,
      });
      setConnection(result.connection);
      setConnectionDiagnostics(result.diagnostics);
      setPairingToken(result.pairingToken ?? null);
      setPendingRollback(null);
      setError(null);
    } catch (value) {
      const message = formatInvokeError(value);
      await refresh();
      setError(message);
    } finally {
      setActing(false);
    }
  }

  async function connectTarget(target: CaptureTarget) {
    setActing(true);
    try {
      const result = await invoke<ConnectDeviceResult>("connect_capture_target", {
        target,
        sessionName: sessionName.trim() || null,
      });
      setConnection(result.connection);
      setConnectionDiagnostics(result.diagnostics);
      setPairingToken(result.pairingToken ?? null);
      setPendingRollback(null);
      setError(null);
    } catch (value) {
      const message = formatInvokeError(value);
      await refresh();
      setError(message);
    } finally {
      setActing(false);
    }
  }

  async function disconnect() {
    setActing(true);
    try {
      const result = await invoke<ConnectionSnapshot>("disconnect_device");
      setConnection(result);
      setConnectionDiagnostics([]);
      setPairingToken(null);
      setPendingRollback(null);
      setError(null);
    } catch (value) {
      const message = formatInvokeError(value);
      await refresh();
      setError(message);
    } finally {
      setActing(false);
    }
  }

  async function recover() {
    setActing(true);
    try {
      const diagnostics = await invoke<ConnectionDiagnostic[]>("recover_pending_rollback");
      setConnectionDiagnostics(diagnostics);
      setPendingRollback(null);
      setError(null);
      await refresh();
    } catch (value) {
      setError(formatInvokeError(value));
    } finally {
      setActing(false);
    }
  }

  const allDiagnostics = [...payload.diagnostics, ...connectionDiagnostics];
  const selectedReady = selected ? isReady(selected) : false;

  return (
    <div className="connect-workbench">
      <section className="readiness-strip" aria-label="Capture readiness">
        <div><span className="eyebrow">01 / CHECK</span><strong>Prerequisites</strong><small>{doctor ? `${doctor.checks.filter((check) => check.status === "pass").length} of ${doctor.checks.length} ready` : "Checking local tools"}</small></div>
        <div><span className="eyebrow">02 / CHOOSE</span><strong>Runtime</strong><small>{loading ? "Scanning…" : `${payload.devices.length} discovered`}</small></div>
        <div><span className="eyebrow">03 / CAPTURE</span><strong>Connection</strong><small>{connection.connected ? connection.captureRunning === false ? "Capture stopped" : "Capturing traffic" : "Waiting for a device"}</small></div>
        <button className="secondary" onClick={onOpenTraffic} disabled={!connection.connected}>Open Traffic →</button>
      </section>
      {doctor && doctor.checks.some((check) => check.status !== "pass") ? <section className="panel readiness-checks" aria-label="Prerequisites and actions">
        <div className="panel-heading"><div><strong>Before you connect</strong><span>Fix the items that apply to your runtime</span></div></div>
        <div className="check-grid">{doctor.checks.map((check) => <article className={`check-item ${check.status}`} key={check.id}>
          <span className="check-symbol" aria-hidden="true">{check.status === "pass" ? "✓" : "!"}</span>
          <div><strong>{check.title}</strong><p>{check.detail}</p>{check.action ? <small>{check.action}</small> : null}</div>
        </article>)}</div>
      </section> : null}
      {error ? <div className="error-banner" role="alert">{error}</div> : null}
      {connection.connected && connection.captureRunning === false ? <div className="error-banner" role="alert">Capture engine stopped unexpectedly. Disconnect to restore the capture settings, then reconnect.</div> : null}
      <section className="connect-grid" aria-label="Additional capture targets">
        <div className="panel">
          <div className="panel-heading"><div><strong>Mac capture</strong><span>All traffic or one running process</span></div></div>
          <div className="selected-device-detail">
            <p>mitmproxy local capture needs no Mac system proxy change. macOS may ask for permission.</p>
            <button className="primary wide" onClick={() => void connectTarget({ schemaVersion: 1, type: "mac_all" })} disabled={acting || loading || connection.connected || Boolean(pendingRollback)}>
              {acting ? "Starting…" : "Capture this Mac"}
            </button>
            <label className="field-label" htmlFor="mac-process">Running process</label>
            <select className="text-input" id="mac-process" value={selectedProcessPid ?? ""} onChange={(event) => setSelectedProcessPid(Number(event.target.value))} disabled={acting || connection.connected}>
              {processes.length === 0 ? <option value="">No process available</option> : processes.map((process) => <option key={process.pid} value={process.pid}>{process.name} · PID {process.pid}</option>)}
            </select>
            <button className="secondary wide" onClick={() => {
              const process = processes.find((item) => item.pid === selectedProcessPid);
              if (process) void connectTarget({ schemaVersion: 1, type: "mac_process", pid: process.pid, name: process.name });
            }} disabled={acting || connection.connected || selectedProcessPid === null || Boolean(pendingRollback)}>
              Capture selected process
            </button>
          </div>
        </div>
        <div className="panel">
          <div className="panel-heading"><div><strong>Physical device</strong><span>Explicit LAN proxy for one paired address</span></div></div>
          <div className="selected-device-detail">
            <label className="field-label" htmlFor="physical-platform">Device platform</label>
            <select className="text-input" id="physical-platform" value={physicalPlatform} onChange={(event) => setPhysicalPlatform(event.target.value as "ios" | "android")} disabled={acting || connection.connected}>
              <option value="ios">iOS</option><option value="android">Android</option>
            </select>
            <label className="field-label" htmlFor="lan-interface">Mac LAN interface</label>
            <select className="text-input" id="lan-interface" value={selectedInterface} onChange={(event) => setSelectedInterface(event.target.value)} disabled={acting || connection.connected}>
              {interfaces.length === 0 ? <option value="">No private LAN interface available</option> : interfaces.map((item) => <option key={item.name} value={item.name}>{item.name} · {item.address}</option>)}
            </select>
            <label className="field-label" htmlFor="paired-address">Device IPv4 address</label>
            <input className="text-input" id="paired-address" inputMode="decimal" placeholder="192.168.1.42" value={pairedAddress} onChange={(event) => setPairedAddress(event.target.value)} disabled={acting || connection.connected} />
            <small>Find the address in the device's Wi-Fi settings. You will set its proxy and development CA manually after starting capture.</small>
            <button className="primary wide" onClick={() => void connectTarget({ schemaVersion: 1, type: physicalPlatform === "ios" ? "physical_ios" : "physical_android", address: pairedAddress.trim(), interface: selectedInterface })} disabled={acting || connection.connected || !selectedInterface || !pairedAddress.trim() || Boolean(pendingRollback)}>
              {acting ? "Starting…" : "Enable paired LAN proxy"}
            </button>
          </div>
        </div>
      </section>
      <section className="panel" aria-label="Manual proxy listener">
        <div className="panel-heading"><div><strong>Manual listener</strong><span>Reverse, upstream, SOCKS5, or DNS on this Mac</span></div></div>
        <div className="selected-device-detail">
          <div className="mock-grid three-column">
            <label className="field-label">Mode<select className="text-input" value={listenerMode} disabled={acting || connection.connected} onChange={(event) => { const mode = event.target.value as ListenerMode["type"]; setListenerMode(mode); setListenerPort(mode === "dns_proxy" ? 8186 : 8185); }}><option value="reverse_proxy">Reverse proxy</option><option value="upstream_proxy">Upstream proxy</option><option value="socks5">SOCKS5 listener</option><option value="dns_proxy">DNS listener</option></select></label>
            <label className="field-label">Loopback port<input className="text-input" type="number" min="1024" max="65535" step="1" value={listenerPort} disabled={acting || connection.connected} onChange={(event) => setListenerPort(Number(event.target.value))} /></label>
            {listenerMode === "reverse_proxy" || listenerMode === "upstream_proxy" ? <label className="field-label">{listenerMode === "reverse_proxy" ? "Target URL" : "Upstream proxy URL"}<input className="text-input" type="url" value={listenerUrl} placeholder="https://example.com" disabled={acting || connection.connected} onChange={(event) => setListenerUrl(event.target.value)} /></label> : null}
          </div>
          <p>Configure your development client to use 127.0.0.1:{listenerPort}. DNS overrides apply only to queries sent to the DNS listener; SOCKS5 needs a SOCKS5 client setting.</p>
          <button className="primary" disabled={acting || connection.connected || Boolean(pendingRollback) || !Number.isInteger(listenerPort) || listenerPort < 1024 || listenerPort > 65535 || ((listenerMode === "reverse_proxy" || listenerMode === "upstream_proxy") && !listenerUrl.trim())} onClick={() => {
            const mode: ListenerMode = listenerMode === "reverse_proxy" || listenerMode === "upstream_proxy" ? { type: listenerMode, url: listenerUrl.trim() } : { type: listenerMode };
            void connectTarget({ schemaVersion: 1, type: "proxy_listener", mode, listenPort: listenerPort });
          }}>{acting ? "Starting…" : "Start listener"}</button>
        </div>
      </section>
    <section className="connect-grid">
      <div className="panel device-panel">
        <div className="panel-heading">
          <div>
            <strong>Local runtimes</strong>
            <span>{loading ? "Scanning local tools…" : `${payload.devices.length} discovered`}</span>
          </div>
          <button className="secondary compact" onClick={() => void refresh()} disabled={loading || acting}>
            Refresh
          </button>
        </div>

        {pendingRollback ? (
          <div className="error-banner">
            <strong>Interrupted connection detected.</strong>{" "}
            {pendingRollback.platform === "android"
              ? "The previous emulator proxy setting should be restored before another capture."
              : "The previous Simulator session installed a local capture CA."}
            <button className="secondary compact" onClick={() => void recover()} disabled={acting}>
              Recover
            </button>
          </div>
        ) : null}

        <div className="device-list">
          {payload.devices.map((device) => (
            <button
              className={device.id === selectedId ? "device-card selected" : "device-card"}
              key={device.id}
              onClick={() => setSelectedId(device.id)}
              disabled={connection.connected && connection.deviceId !== device.id}
            >
              <div className="device-icon">{device.platform === "ios" ? "iOS" : "A"}</div>
              <div className="device-copy">
                <strong>{device.name}</strong>
                <span>
                  {device.platform === "ios" ? "iOS Simulator" : "Android Emulator"}
                  {device.osVersion ? ` · ${device.osVersion}` : ""}
                </span>
              </div>
              <span className={isReady(device) ? "device-state ready" : "device-state"}>
                {device.state}
              </span>
            </button>
          ))}

          {!loading && payload.devices.length === 0 ? (
            <div className="empty-state">No iOS Simulator or Android Emulator was discovered.</div>
          ) : null}
        </div>
      </div>

      <div className="connect-side">
        <div className="panel selected-device-panel">
          <div className="panel-heading">
            <div>
              <strong>{connection.connected ? "Active capture" : "Connection target"}</strong>
              <span>{connection.connected ? connection.strategy : "Phase 1 capture runtime"}</span>
            </div>
          </div>

          {connection.connected && connection.captureTarget && !["ios_simulator", "android_emulator"].includes(connection.captureTarget.type) ? (
            <div className="selected-device-detail">
              <h2>{connection.captureTarget.type === "mac_all" ? "This Mac" : connection.captureTarget.type === "mac_process" ? connection.captureTarget.name : connection.captureTarget.type === "proxy_listener" ? `${connection.captureTarget.mode.type.replaceAll("_", " ")} listener` : "Paired physical device"}</h2>
              <p>{connection.strategy}</p>
              {connection.proxyHost ? <div className="capability-row"><span>Listener</span><strong>{connection.proxyHost}:{connection.proxyPort}</strong></div> : null}
              {pairingToken ? <div className="capability-row"><span>SDK pairing token</span><code className="pairing-token">{pairingToken}</code></div> : null}
              <div className="capability-row"><span>Session</span><strong>{connection.sessionId}</strong></div>
              <button className="secondary wide" onClick={() => void disconnect()} disabled={acting}>{acting ? "Disconnecting…" : "Disconnect capture"}</button>
              <button className="primary wide" onClick={onOpenTraffic}>Inspect traffic →</button>
            </div>
          ) : selected ? (
            <div className="selected-device-detail">
              <h2>{selected.name}</h2>
              <p>{selected.id}</p>
              <div className="capability-row">
                <span>CA setup</span>
                <strong>{selected.capabilities.canInstallCa ? "Automated + guided trust" : "Guided"}</strong>
              </div>
              <div className="capability-row">
                <span>Proxy routing</span>
                <strong>{selected.capabilities.canAutoRouteProxy ? "Automated" : "Guided"}</strong>
              </div>

              {connection.connected ? (
                <>
                  <div className="capability-row">
                    <span>Proxy</span>
                    <strong>{connection.proxyHost}:{connection.proxyPort}</strong>
                  </div>
                  <div className="capability-row">
                    <span>Session</span>
                    <strong>{connection.sessionId}</strong>
                  </div>
                  <button className="secondary wide" onClick={() => void disconnect()} disabled={acting}>
                    {acting ? "Disconnecting…" : "Disconnect capture"}
                  </button>
                  <button className="primary wide" onClick={onOpenTraffic}>Inspect traffic →</button>
                </>
              ) : (
                <>
                  <label className="field-label" htmlFor="session-name">Session name</label>
                  <input
                    id="session-name"
                    className="text-input"
                    placeholder="Optional — e.g. Checkout regression"
                    value={sessionName}
                    onChange={(event) => setSessionName(event.target.value)}
                    disabled={acting}
                  />
                  <button
                    className="primary wide"
                    onClick={() => void connect()}
                    disabled={!selectedReady || acting || Boolean(pendingRollback)}
                  >
                    {acting ? "Connecting…" : "Start capture"}
                  </button>
                  {!selectedReady ? <small>Boot or start this runtime before connecting.</small> : null}
                </>
              )}
            </div>
          ) : (
            <div className="empty-state">Select a runtime to inspect its capabilities.</div>
          )}
        </div>

        {allDiagnostics.length > 0 ? (
          <div className="panel diagnostic-panel">
            <div className="panel-heading">
              <div>
                <strong>Diagnostics</strong>
                <span>Connection prerequisites and guidance</span>
              </div>
            </div>
            <div className="diagnostic-list">
              {allDiagnostics.map((diagnostic, index) => (
                <article key={`${diagnostic.code}-${index}`} className="diagnostic-item">
                  <strong>{diagnostic.title}</strong>
                  <p>{diagnostic.message}</p>
                  {diagnostic.suggestedAction ? <small>{diagnostic.suggestedAction}</small> : null}
                </article>
              ))}
            </div>
          </div>
        ) : null}
      </div>
    </section>
    </div>
  );
}

function preferredDevice(devices: Device[]) {
  return devices.find(isReady) ?? devices[0];
}

function isReady(device: Device) {
  return device.state.toLowerCase() === "booted" || device.state.toLowerCase() === "device";
}

function formatInvokeError(value: unknown) {
  if (typeof value === "string") return value;
  if (value && typeof value === "object" && "message" in value) {
    return String((value as { message: unknown }).message);
  }
  return String(value);
}
