import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useMemo, useState } from "react";
import type { Device, DeviceDiscoveryPayload } from "../types";

export function ConnectView() {
  const [payload, setPayload] = useState<DeviceDiscoveryPayload>({
    devices: [],
    diagnostics: [],
  });
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const result = await invoke<DeviceDiscoveryPayload>("list_devices");
      setPayload(result);
      setSelectedId((current) => {
        if (current && result.devices.some((device) => device.id === current)) {
          return current;
        }
        return preferredDevice(result.devices)?.id ?? null;
      });
      setError(null);
    } catch (value) {
      setError(String(value));
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

  return (
    <section className="connect-grid">
      <div className="panel device-panel">
        <div className="panel-heading">
          <div>
            <strong>Local runtimes</strong>
            <span>{loading ? "Scanning local tools…" : `${payload.devices.length} discovered`}</span>
          </div>
          <button className="secondary compact" onClick={() => void refresh()} disabled={loading}>
            Refresh
          </button>
        </div>

        {error ? <div className="error-banner">{error}</div> : null}

        <div className="device-list">
          {payload.devices.map((device) => (
            <button
              className={device.id === selectedId ? "device-card selected" : "device-card"}
              key={device.id}
              onClick={() => setSelectedId(device.id)}
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
              <strong>Connection target</strong>
              <span>Phase 1 device adapter</span>
            </div>
          </div>

          {selected ? (
            <div className="selected-device-detail">
              <h2>{selected.name}</h2>
              <p>{selected.id}</p>
              <div className="capability-row">
                <span>CA automation</span>
                <strong>{selected.capabilities.canInstallCa ? "Supported" : "Guided"}</strong>
              </div>
              <div className="capability-row">
                <span>Proxy routing</span>
                <strong>{selected.capabilities.canAutoRouteProxy ? "Supported" : "Guided"}</strong>
              </div>
              <button className="primary wide" disabled>
                Capture connection coming next
              </button>
            </div>
          ) : (
            <div className="empty-state">Select a runtime to inspect its capabilities.</div>
          )}
        </div>

        {payload.diagnostics.length > 0 ? (
          <div className="panel diagnostic-panel">
            <div className="panel-heading">
              <div>
                <strong>Diagnostics</strong>
                <span>Local prerequisites</span>
              </div>
            </div>
            <div className="diagnostic-list">
              {payload.diagnostics.map((diagnostic) => (
                <article key={diagnostic.code} className="diagnostic-item">
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
  );
}

function preferredDevice(devices: Device[]) {
  return devices.find(isReady) ?? devices[0];
}

function isReady(device: Device) {
  return device.state.toLowerCase() === "booted" || device.state.toLowerCase() === "device";
}
