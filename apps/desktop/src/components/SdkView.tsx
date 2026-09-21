import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useMemo, useState } from "react";
import type { SdkClientRecord, SdkEnvelope, SdkSetupInfo } from "../sdkTypes";
import { contextForEnvelope } from "../sdkTypes";
import type { CaptureSession } from "../types";

export function SdkView() {
  const [clients, setClients] = useState<SdkClientRecord[]>([]);
  const [sessions, setSessions] = useState<CaptureSession[]>([]);
  const [setup, setSetup] = useState<SdkSetupInfo | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [events, setEvents] = useState<SdkEnvelope[]>([]);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [nextClients, nextSessions, nextSetup] = await Promise.all([
        invoke<SdkClientRecord[]>("list_sdk_clients"),
        invoke<CaptureSession[]>("list_sessions"),
        invoke<SdkSetupInfo>("sdk_setup_info"),
      ]);
      setClients(nextClients);
      setSessions(nextSessions);
      setSetup(nextSetup);
      setSelectedId((current) => current && nextClients.some((client) => client.clientId === current)
        ? current
        : nextClients[0]?.clientId ?? null);
      setError(null);
    } catch (value) {
      setError(formatInvokeError(value));
    }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), 1500);
    return () => window.clearInterval(timer);
  }, [refresh]);

  useEffect(() => {
    if (!selectedId) {
      setEvents([]);
      return;
    }
    let cancelled = false;
    async function load() {
      try {
        const next = await invoke<SdkEnvelope[]>("list_sdk_events", { clientId: selectedId, limit: 300 });
        if (!cancelled) setEvents(next);
      } catch (value) {
        if (!cancelled) setError(formatInvokeError(value));
      }
    }
    void load();
    const timer = window.setInterval(() => void load(), 1000);
    return () => { cancelled = true; window.clearInterval(timer); };
  }, [selectedId]);

  const selected = useMemo(() => clients.find((client) => client.clientId === selectedId) ?? null, [clients, selectedId]);
  const attributedSessions = sessions.filter((session) => Boolean(session.appId)).slice(0, 8);

  return (
    <div className="sdk-page-stack">
      <section className="panel sdk-overview">
        <div className="panel-heading">
          <div><strong>App-aware SDK</strong><span>{setup?.activeClientCount ?? 0} active · {setup?.knownClientCount ?? clients.length} known clients</span></div>
          <div className="sdk-health-heading"><span className={setup?.ingestionReachable ? "sdk-status-pill active" : "sdk-status-pill"}>{setup?.ingestionReachable ? "ingestion online" : "ingestion unavailable"}</span><button className="secondary compact" onClick={() => void refresh()}>Refresh</button></div>
        </div>
        {error ? <div className="error-banner">{error}</div> : null}
        {setup ? <div className="sdk-setup-grid">
          <div><span>iOS Simulator</span><strong>{setup.iosBaseUrl}</strong></div>
          <div><span>Android Emulator</span><strong>{setup.androidBaseUrl}</strong></div>
          <div><span>Event endpoint</span><strong>{setup.eventPath}</strong></div>
          <div><span>Last SDK activity</span><strong>{setup.latestSeenAt ? formatTimestamp(setup.latestSeenAt) : "No handshake yet"}</strong></div>
        </div> : null}
        <p className="muted-copy sdk-note">SDK telemetry stays local. The correlation header <code>{setup?.correlationHeader ?? "X-Mobile-API-Studio-Request-Id"}</code> is captured locally and removed by the proxy before the real backend receives the request.</p>
        {attributedSessions.length > 0 ? <div className="sdk-session-strip">
          <strong>Attributed sessions</strong>
          <div>{attributedSessions.map((session) => <span key={session.id}><b>{session.name}</b><small>{session.appId} · {session.deviceId ?? "unknown device"}</small></span>)}</div>
        </div> : null}
      </section>

      <section className="panel sdk-client-layout">
        <aside className="sdk-client-list">
          <div className="panel-heading"><div><strong>SDK clients</strong><span>Handshake registry</span></div></div>
          {clients.map((client) => <button key={client.clientId} className={selectedId === client.clientId ? "sdk-client-row selected" : "sdk-client-row"} onClick={() => setSelectedId(client.clientId)}>
            <span className={isRecentlyActive(client) ? "sdk-presence active" : "sdk-presence"} />
            <span><strong>{client.appName}</strong><small>{client.platform} · {client.deviceName ?? "unknown device"}</small></span>
          </button>)}
          {clients.length === 0 ? <p className="empty-state">No SDK handshakes received yet. Run a debug app with the iOS or Android SDK enabled.</p> : null}
        </aside>

        <div className="sdk-client-detail">
          {selected ? <>
            <div className="sdk-client-header">
              <div><span className="eyebrow">{selected.platform} app</span><h2>{selected.appName}</h2><p>{selected.appId}</p></div>
              <span className={isRecentlyActive(selected) ? "sdk-status-pill active" : "sdk-status-pill"}>{isRecentlyActive(selected) ? "active" : "idle"}</span>
            </div>
            <dl className="detail-grid sdk-detail-grid">
              <dt>Version</dt><dd>{selected.appVersion ?? "—"}{selected.appBuild ? ` (${selected.appBuild})` : ""}</dd>
              <dt>Device</dt><dd>{selected.deviceName ?? "—"}</dd>
              <dt>OS</dt><dd>{selected.osVersion ?? "—"}</dd>
              <dt>SDK</dt><dd>{selected.sdkVersion}</dd>
              <dt>Last seen</dt><dd>{formatTimestamp(selected.lastSeenAt)}</dd>
            </dl>
            <section className="sdk-event-section">
              <h3>Recent app events</h3>
              <div className="sdk-event-list">{events.map((event) => <SdkEventRow key={event.eventId} event={event} />)}</div>
            </section>
          </> : <p className="empty-state">Select an SDK client.</p>}
        </div>
      </section>
    </div>
  );
}

function SdkEventRow({ event }: { event: SdkEnvelope }) {
  const context = contextForEnvelope(event);
  let title: string = event.event.type;
  let detail = "";
  if (event.event.type === "network") {
    title = `${event.event.payload.phase} · ${event.event.payload.method}`;
    detail = event.event.payload.url;
  } else if (event.event.type === "log") {
    title = `${event.event.payload.level} · log`;
    detail = event.event.payload.message;
  } else if (event.event.type === "context") {
    title = "context changed";
    detail = [context?.screen, context?.feature].filter(Boolean).join(" · ");
  } else {
    detail = `${event.event.payload.appName} · ${event.event.payload.platform}`;
  }
  return <div className="sdk-event-row">
    <span>{formatTimestamp(event.occurredAt)}</span>
    <div><strong>{title}</strong><small>{detail || "—"}</small>{context?.source?.file ? <small>{context.source.file}{context.source.line ? `:${context.source.line}` : ""}</small> : null}</div>
    <span>{context?.screen ?? context?.feature ?? ""}</span>
  </div>;
}

function isRecentlyActive(client: SdkClientRecord) {
  return Date.now() - Number(client.lastSeenAt) < 15_000;
}

function formatTimestamp(value: string) {
  const millis = Number(value);
  if (!Number.isFinite(millis) || millis <= 0) return value;
  return new Date(millis).toLocaleTimeString();
}

function formatInvokeError(value: unknown) {
  if (typeof value === "string") return value;
  if (value && typeof value === "object" && "message" in value) return String((value as { message: unknown }).message);
  return String(value);
}
