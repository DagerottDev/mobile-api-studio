import { useEffect, useRef, useState } from "react";
import { invoke } from "../api/invoke";
import type { NetworkProfile, NetworkProfileScope } from "../types";

const MAX_RATE = 1024 ** 3;
const emptyScope: NetworkProfileScope = { type: "global" };
function errorText(value: unknown) { return value instanceof Error ? value.message : String(typeof value === "object" && value !== null && "message" in value ? value.message : value); }
function profileScope(scope: NetworkProfileScope) {
  return scope.type === "endpoint" ? `${scope.method || "ANY"} ${scope.host || "*"}${scope.path || "*"}`
    : scope.type === "host" ? scope.host : scope.type === "app" ? `App ${scope.appId}` : "Global";
}

export function NetworkProfilesView({ profiles, refresh }: { profiles: NetworkProfile[]; refresh: () => Promise<void> }) {
  const [draft, setDraft] = useState<NetworkProfile | null>(null);
  const knownEnabled = useRef(new Map<string, boolean>());
  const [newId, setNewId] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => { void refresh(); }, [refresh]);
  useEffect(() => {
    const previous = knownEnabled.current;
    const changed = new Set(profiles.filter((profile) => previous.has(profile.id) && previous.get(profile.id) !== profile.enabled).map((profile) => profile.id));
    knownEnabled.current = new Map(profiles.map((profile) => [profile.id, profile.enabled]));
    setDraft((current) => {
      const saved = current && changed.has(current.id) ? profiles.find((profile) => profile.id === current.id) : null;
      return saved ? { ...current!, enabled: saved.enabled } : current;
    });
  }, [profiles]);

  function create() {
    const now = Date.now().toString();
    const profile: NetworkProfile = {
      schemaVersion: 1, id: `network-${Date.now()}-${Math.random().toString(16).slice(2)}`,
      name: "New network profile", enabled: false, priority: profiles.length, scope: emptyScope,
      latencyMs: 0, jitterMs: 0, uploadBytesPerSecond: null, downloadBytesPerSecond: null,
      offline: false, failurePercent: 0, createdAt: now, updatedAt: now,
    };
    setDraft(profile); setNewId(profile.id); setMessage(null); setError(null);
  }

  async function save() {
    if (!draft) return;
    if (!draft.name.trim()) { setError("Profile name is required."); return; }
    if (!Number.isInteger(draft.priority) || draft.priority < -2_147_483_648 || draft.priority > 2_147_483_647) { setError("Priority must be a whole number."); return; }
    if (![draft.latencyMs, draft.jitterMs].every((value) => Number.isInteger(value) && value >= 0 && value <= 10_000)) { setError("Latency and jitter must be whole numbers from 0 to 10,000 ms."); return; }
    if (![draft.uploadBytesPerSecond, draft.downloadBytesPerSecond].every((value) => value === null || (Number.isInteger(value) && value >= 1_024 && value <= MAX_RATE))) { setError("Transfer rates must be blank or 1 KiB/s to 1 GiB/s."); return; }
    if (!Number.isFinite(draft.failurePercent) || draft.failurePercent < 0 || draft.failurePercent > 100) { setError("Request failure percent must be from 0 to 100."); return; }
    setBusy(true);
    try {
      const saved = await invoke<NetworkProfile>("upsert_network_profile", { profile: { ...draft, name: draft.name.trim(), updatedAt: Date.now().toString() } });
      setDraft(saved); setNewId(null); setMessage(`Saved ${saved.name}.`); await refresh();
    } catch (value) { setError(errorText(value)); } finally { setBusy(false); }
  }

  async function remove() {
    if (!draft || !window.confirm(`Delete network profile “${draft.name}”?`)) return;
    setBusy(true);
    try {
      await invoke("delete_network_profile", { id: draft.id });
      setDraft(null); setNewId(null); setMessage("Network profile deleted."); setError(null); await refresh();
    } catch (value) { setError(errorText(value)); } finally { setBusy(false); }
  }

  function patchEndpoint(patch: Partial<Extract<NetworkProfileScope, { type: "endpoint" }>>) {
    setDraft((current) => current?.scope.type === "endpoint" ? { ...current, scope: { ...current.scope, ...patch } } : current);
  }

  function setScopeType(type: NetworkProfileScope["type"]) {
    setDraft((current) => current ? { ...current, scope: type === "app" ? { type, appId: "" }
      : type === "host" ? { type, host: "" }
      : type === "endpoint" ? { type, method: "GET", host: "", path: "/" }
      : { type } } : null);
  }

  const active = profiles.filter((profile) => profile.enabled);
  return <section className="mocks-layout panel" aria-label="Network profiles">
    <aside className="mocks-list-pane">
      <div className="panel-heading"><div><strong>Network profiles</strong><span>{active.length} active · higher-specificity scopes match first</span></div>
        <button className="primary compact" onClick={create} disabled={busy || newId !== null}>New profile</button></div>
      <div className="mock-rule-list">{profiles.map((profile) => <button key={profile.id} className={draft?.id === profile.id ? "mock-rule-row selected" : "mock-rule-row"} onClick={() => { setDraft({ ...profile }); setNewId(null); setError(null); }}>
        <span className="mock-priority">{profile.priority}</span><span className="mock-rule-copy"><strong>{profile.name}</strong><small>{profileScope(profile.scope)} · {profile.enabled ? "Active" : "Disabled"}</small></span>
      </button>)}{profiles.length === 0 ? <p className="empty-state">No network profiles. New profiles start disabled.</p> : null}</div>
    </aside>
    <div className="mock-editor-pane">
      {error ? <div className="error-banner" role="alert">{error}</div> : null}
      {message ? <div className="settings-message mock-message" role="status">{message}</div> : null}
      <div className="mock-editor-scroll">
        <div className="mock-editor-heading"><div><span className="eyebrow">Network profile</span><h2>{draft?.name ?? "Select a profile"}</h2></div>
          {draft ? <div className="mock-heading-actions"><button className="secondary compact" onClick={() => void remove()} disabled={busy}>Delete</button><button className="primary compact" onClick={() => void save()} disabled={busy}>Save</button></div> : null}</div>
        {draft ? <>
          <section className="mock-section"><h3>Profile</h3><div className="mock-grid two-column">
            <label className="field-label">Name<input className="text-input" value={draft.name} onChange={(event) => setDraft({ ...draft, name: event.target.value })} /></label>
            <label className="field-label">Priority<input className="text-input" type="number" step="1" value={draft.priority} onChange={(event) => setDraft({ ...draft, priority: Number(event.target.value) })} /></label>
            <label className="field-label">Scope<select value={draft.scope.type} onChange={(event) => setScopeType(event.target.value as NetworkProfileScope["type"])}><option value="global">Global</option><option value="app">App</option><option value="host">Host</option><option value="endpoint">Endpoint</option></select></label>
            <label className="inline-toggle"><input type="checkbox" checked={draft.enabled} onChange={(event) => setDraft({ ...draft, enabled: event.target.checked })} /> Enabled</label>
          </div>
          {draft.scope.type === "app" ? <label className="field-label">App ID<input className="text-input" value={draft.scope.appId} onChange={(event) => setDraft({ ...draft, scope: { type: "app", appId: event.target.value } })} placeholder="Available app correlation ID" /></label> : null}
          {draft.scope.type === "host" ? <label className="field-label">Host<input className="text-input" value={draft.scope.host} onChange={(event) => setDraft({ ...draft, scope: { type: "host", host: event.target.value } })} placeholder="api.example.com" /></label> : null}
          {draft.scope.type === "endpoint" ? <div className="mock-grid three-column"><label className="field-label">Method<input className="text-input" value={draft.scope.method} onChange={(event) => patchEndpoint({ method: event.target.value.toUpperCase() })} /></label><label className="field-label">Host<input className="text-input" value={draft.scope.host} onChange={(event) => patchEndpoint({ host: event.target.value })} placeholder="api.example.com" /></label><label className="field-label">Path<input className="text-input" value={draft.scope.path} onChange={(event) => patchEndpoint({ path: event.target.value })} placeholder="/v1/items" /></label></div> : null}
          <p className="muted-copy">Match order is endpoint, host, app, then global; priority breaks ties. App scope uses only correlation already available when the request arrives.</p></section>
          <section className="mock-section"><h3>Conditions</h3><div className="mock-grid three-column">
            <label className="field-label">Latency (ms)<input className="text-input" type="number" min="0" max="10000" value={draft.latencyMs} onChange={(event) => setDraft({ ...draft, latencyMs: Number(event.target.value) })} /></label>
            <label className="field-label">Jitter (ms)<input className="text-input" type="number" min="0" max="10000" value={draft.jitterMs} onChange={(event) => setDraft({ ...draft, jitterMs: Number(event.target.value) })} /></label>
            <label className="field-label">Request failures (%)<input className="text-input" type="number" min="0" max="100" value={draft.failurePercent} onChange={(event) => setDraft({ ...draft, failurePercent: Number(event.target.value) })} /></label>
          </div><div className="mock-grid two-column">
            <label className="field-label">Upload rate (bytes/s)<input className="text-input" type="number" min="1024" max={MAX_RATE} value={draft.uploadBytesPerSecond ?? ""} placeholder="Unlimited" onChange={(event) => setDraft({ ...draft, uploadBytesPerSecond: event.target.value ? Number(event.target.value) : null })} /></label>
            <label className="field-label">Download rate (bytes/s)<input className="text-input" type="number" min="1024" max={MAX_RATE} value={draft.downloadBytesPerSecond ?? ""} placeholder="Unlimited" onChange={(event) => setDraft({ ...draft, downloadBytesPerSecond: event.target.value ? Number(event.target.value) : null })} /></label>
          </div><label className="inline-toggle"><input type="checkbox" checked={draft.offline} onChange={(event) => setDraft({ ...draft, offline: event.target.checked })} /> Offline</label>
          <p className="muted-copy">Bandwidth uses application-level buffered transfer delay proportional to body bytes per rate, capped at 120 seconds per stage; it does not smooth packet pacing. Request failures are simulated separately from packet loss. Packet loss is unavailable in selected MITM proxy modes.</p></section>
        </> : <p className="empty-state">Create a profile to configure network conditions.</p>}
      </div>
    </div>
  </section>;
}
