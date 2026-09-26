import { invoke } from "../api/invoke";
import { useCallback, useEffect, useMemo, useState } from "react";
import type {
  ConnectDeviceResult,
  ConnectionDiagnostic,
  ConnectionDoctorReport,
  ConnectionSnapshot,
  Device,
  DeviceDiscoveryPayload,
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

export function ConnectView({ onOpenTraffic }: { onOpenTraffic: () => void }) {
  const [payload, setPayload] = useState<DeviceDiscoveryPayload>({ devices: [], diagnostics: [] });
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [connection, setConnection] = useState<ConnectionSnapshot>(disconnected);
  const [connectionDiagnostics, setConnectionDiagnostics] = useState<ConnectionDiagnostic[]>([]);
  const [pendingRollback, setPendingRollback] = useState<RollbackJournal | null>(null);
  const [doctor, setDoctor] = useState<ConnectionDoctorReport | null>(null);
  const [sessionName, setSessionName] = useState("");
  const [loading, setLoading] = useState(true);
  const [acting, setActing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const [devices, current, rollback, report] = await Promise.all([
        invoke<DeviceDiscoveryPayload>("list_devices"),
        invoke<ConnectionSnapshot>("current_connection"),
        invoke<RollbackJournal | null>("pending_rollback"),
        invoke<ConnectionDoctorReport>("connection_doctor"),
      ]);
      setPayload(devices);
      setConnection(current);
      setPendingRollback(rollback);
      setDoctor(report);
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
        <div><span className="eyebrow">03 / CAPTURE</span><strong>Connection</strong><small>{connection.connected ? "Capturing traffic" : "Waiting for a device"}</small></div>
        <button className="secondary" onClick={onOpenTraffic} disabled={!connection.connected}>Open Traffic →</button>
      </section>
      {doctor && doctor.checks.some((check) => check.status !== "pass") ? <section className="panel readiness-checks" aria-label="Prerequisites and actions">
        <div className="panel-heading"><div><strong>Before you connect</strong><span>Fix the items that apply to your runtime</span></div></div>
        <div className="check-grid">{doctor.checks.map((check) => <article className={`check-item ${check.status}`} key={check.id}>
          <span className="check-symbol" aria-hidden="true">{check.status === "pass" ? "✓" : "!"}</span>
          <div><strong>{check.title}</strong><p>{check.detail}</p>{check.action ? <small>{check.action}</small> : null}</div>
        </article>)}</div>
      </section> : null}
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

        {error ? <div className="error-banner">{error}</div> : null}

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

          {selected ? (
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
