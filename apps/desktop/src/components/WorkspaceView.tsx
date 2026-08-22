import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useMemo, useState } from "react";
import type {
  CaptureSession,
  Environment,
  EnvironmentSnapshot,
  EnvironmentVariable,
  SavedCollection,
  SavedRequest,
} from "../types";

type WorkspaceTab = "sessions" | "collections" | "environments";

interface WorkspaceViewProps {
  onOpenReplay?: (requestId: string) => void;
}

export function WorkspaceView({ onOpenReplay }: WorkspaceViewProps) {
  const [tab, setTab] = useState<WorkspaceTab>("sessions");
  return (
    <section className="workspace-view panel">
      <div className="workspace-tabs">
        {(["sessions", "collections", "environments"] as WorkspaceTab[]).map((value) => (
          <button
            key={value}
            className={tab === value ? "workspace-tab active" : "workspace-tab"}
            onClick={() => setTab(value)}
          >
            {value[0].toUpperCase() + value.slice(1)}
          </button>
        ))}
      </div>
      {tab === "sessions" ? <SessionsPanel /> : null}
      {tab === "collections" ? <CollectionsPanel onOpenReplay={onOpenReplay} /> : null}
      {tab === "environments" ? <EnvironmentsPanel /> : null}
    </section>
  );
}

function SessionsPanel() {
  const [sessions, setSessions] = useState<CaptureSession[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [notes, setNotes] = useState("");
  const [showArchived, setShowArchived] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const next = await invoke<CaptureSession[]>("list_sessions");
      setSessions(next);
      setSelectedId((current) => current && next.some((item) => item.id === current) ? current : next[0]?.id ?? null);
      setError(null);
    } catch (value) {
      setError(formatInvokeError(value));
    }
  }, []);

  useEffect(() => { void refresh(); }, [refresh]);

  const selected = sessions.find((session) => session.id === selectedId) ?? null;
  useEffect(() => {
    setName(selected?.name ?? "");
    setNotes(selected?.notes ?? "");
  }, [selected?.id, selected?.name, selected?.notes]);

  const visibleSessions = useMemo(
    () => sessions.filter((session) => showArchived || session.status !== "archived"),
    [sessions, showArchived],
  );

  async function save() {
    if (!selected) return;
    setBusy(true);
    try {
      await invoke("update_session_metadata", {
        input: { id: selected.id, name, notes: notes.trim() || null },
      });
      await refresh();
    } catch (value) {
      setError(formatInvokeError(value));
    } finally {
      setBusy(false);
    }
  }

  async function archive() {
    if (!selected) return;
    setBusy(true);
    try {
      await invoke("archive_session", { id: selected.id });
      await refresh();
    } catch (value) {
      setError(formatInvokeError(value));
    } finally {
      setBusy(false);
    }
  }

  async function remove() {
    if (!selected || !window.confirm(`Delete session “${selected.name}” and its captured flows?`)) return;
    setBusy(true);
    try {
      await invoke("delete_session", { id: selected.id });
      await refresh();
    } catch (value) {
      setError(formatInvokeError(value));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="workspace-two-pane">
      <aside className="workspace-list-pane">
        <div className="workspace-pane-heading">
          <div><strong>Capture sessions</strong><span>{visibleSessions.length} visible</span></div>
          <label className="inline-toggle"><input type="checkbox" checked={showArchived} onChange={(event) => setShowArchived(event.target.checked)} /> Archived</label>
        </div>
        <div className="workspace-item-list">
          {visibleSessions.map((session) => (
            <button key={session.id} className={selectedId === session.id ? "workspace-list-item selected" : "workspace-list-item"} onClick={() => setSelectedId(session.id)}>
              <span><strong>{session.name}</strong><small>{session.appId ? `${session.appId} · ` : ""}{session.deviceId ?? "No device"}</small></span>
              <span className={`status-badge status-${session.status}`}>{session.status}</span>
            </button>
          ))}
          {visibleSessions.length === 0 ? <p className="empty-state">No sessions yet.</p> : null}
        </div>
      </aside>

      <div className="workspace-detail-pane">
        {error ? <div className="error-banner">{error}</div> : null}
        {selected ? (
          <>
            <div className="workspace-detail-heading"><div><span className="eyebrow">Session</span><h2>{selected.name}</h2></div><span className={`status-badge status-${selected.status}`}>{selected.status}</span></div>
            <label className="field-label">Name<input className="text-input" value={name} onChange={(event) => setName(event.target.value)} disabled={busy} /></label>
            <label className="field-label">Notes<textarea className="workspace-textarea" value={notes} onChange={(event) => setNotes(event.target.value)} placeholder="What were you debugging in this session?" disabled={busy} /></label>
            <dl className="workspace-metadata">
              <dt>Started</dt><dd>{formatTimestamp(selected.startedAt)}</dd>
              <dt>Ended</dt><dd>{selected.endedAt ? formatTimestamp(selected.endedAt) : "Active"}</dd>
              <dt>Device</dt><dd>{selected.deviceId ?? "—"}</dd>
              <dt>App</dt><dd>{selected.appId ?? "Proxy-only / unknown"}</dd>
              <dt>Connection</dt><dd>{selected.connectionStrategy ?? "—"}</dd>
              <dt>Engine</dt><dd>{selected.captureEngine ?? "—"}</dd>
            </dl>
            <div className="workspace-actions">
              <button className="primary" onClick={() => void save()} disabled={busy}>Save changes</button>
              {selected.status !== "archived" ? <button className="secondary" onClick={() => void archive()} disabled={busy || selected.status === "active"}>Archive</button> : null}
              <button className="secondary danger-action" onClick={() => void remove()} disabled={busy || selected.status === "active"}>Delete</button>
            </div>
          </>
        ) : <p className="empty-state">Select a session.</p>}
      </div>
    </div>
  );
}

function CollectionsPanel({ onOpenReplay }: { onOpenReplay?: (requestId: string) => void }) {
  const [collections, setCollections] = useState<SavedCollection[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [requests, setRequests] = useState<SavedRequest[]>([]);
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [newCollectionName, setNewCollectionName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refreshCollections = useCallback(async () => {
    try {
      const next = await invoke<SavedCollection[]>("list_collections");
      setCollections(next);
      setSelectedId((current) => current && next.some((item) => item.id === current) ? current : next[0]?.id ?? null);
      setError(null);
    } catch (value) {
      setError(formatInvokeError(value));
    }
  }, []);

  useEffect(() => { void refreshCollections(); }, [refreshCollections]);

  const selected = collections.find((collection) => collection.id === selectedId) ?? null;
  useEffect(() => {
    setName(selected?.name ?? "");
    setDescription(selected?.description ?? "");
  }, [selected?.id, selected?.name, selected?.description]);

  const refreshRequests = useCallback(async () => {
    if (!selectedId) {
      setRequests([]);
      return;
    }
    try {
      setRequests(await invoke<SavedRequest[]>("list_saved_requests", { collectionId: selectedId }));
    } catch (value) {
      setError(formatInvokeError(value));
    }
  }, [selectedId]);

  useEffect(() => { void refreshRequests(); }, [refreshRequests]);

  async function createCollection() {
    if (!newCollectionName.trim()) return;
    setBusy(true);
    try {
      const created = await invoke<SavedCollection>("upsert_collection", { input: { id: null, name: newCollectionName.trim(), description: null, sortOrder: collections.length } });
      setNewCollectionName("");
      await refreshCollections();
      setSelectedId(created.id);
    } catch (value) {
      setError(formatInvokeError(value));
    } finally {
      setBusy(false);
    }
  }

  async function saveCollection() {
    if (!selected) return;
    setBusy(true);
    try {
      await invoke("upsert_collection", { input: { id: selected.id, name, description: description.trim() || null, sortOrder: selected.sortOrder } });
      await refreshCollections();
    } catch (value) {
      setError(formatInvokeError(value));
    } finally {
      setBusy(false);
    }
  }

  async function deleteCollection() {
    if (!selected || !window.confirm(`Delete collection “${selected.name}” and its saved requests?`)) return;
    setBusy(true);
    try {
      await invoke("delete_collection", { id: selected.id });
      await refreshCollections();
    } catch (value) {
      setError(formatInvokeError(value));
    } finally {
      setBusy(false);
    }
  }

  async function deleteRequest(id: string) {
    setBusy(true);
    try {
      await invoke("delete_saved_request", { id });
      await refreshRequests();
    } catch (value) {
      setError(formatInvokeError(value));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="workspace-two-pane">
      <aside className="workspace-list-pane">
        <div className="workspace-pane-heading"><div><strong>Collections</strong><span>{collections.length} local groups</span></div></div>
        <div className="workspace-create-row"><input className="text-input" value={newCollectionName} onChange={(event) => setNewCollectionName(event.target.value)} placeholder="New collection" /><button className="primary compact" onClick={() => void createCollection()} disabled={busy}>Add</button></div>
        <div className="workspace-item-list">{collections.map((collection) => <button key={collection.id} className={selectedId === collection.id ? "workspace-list-item selected" : "workspace-list-item"} onClick={() => setSelectedId(collection.id)}><span><strong>{collection.name}</strong><small>{collection.description ?? "Saved requests"}</small></span></button>)}</div>
      </aside>
      <div className="workspace-detail-pane">
        {error ? <div className="error-banner">{error}</div> : null}
        {selected ? <>
          <div className="workspace-detail-heading"><div><span className="eyebrow">Collection</span><h2>{selected.name}</h2></div></div>
          <label className="field-label">Name<input className="text-input" value={name} onChange={(event) => setName(event.target.value)} /></label>
          <label className="field-label">Description<input className="text-input" value={description} onChange={(event) => setDescription(event.target.value)} placeholder="Optional purpose" /></label>
          <div className="workspace-actions"><button className="primary" onClick={() => void saveCollection()} disabled={busy}>Save collection</button><button className="secondary danger-action" onClick={() => void deleteCollection()} disabled={busy}>Delete</button></div>
          <section className="saved-request-section"><h3>Saved requests</h3>{requests.length === 0 ? <p className="muted-copy">Save a captured request from Traffic to populate this collection.</p> : <div className="saved-request-list">{requests.map((request) => <div className="saved-request-row" key={request.id}><div><strong>{request.name}</strong><small>{request.method} · {request.url}</small></div><div><button className="primary compact" onClick={() => onOpenReplay?.(request.id)}>Open in Replay</button><button className="icon-button" onClick={() => void deleteRequest(request.id)}>×</button></div></div>)}</div>}</section>
        </> : <p className="empty-state">Create a collection to save reusable requests.</p>}
      </div>
    </div>
  );
}

function EnvironmentsPanel() {
  const [environments, setEnvironments] = useState<Environment[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [snapshot, setSnapshot] = useState<EnvironmentSnapshot | null>(null);
  const [newName, setNewName] = useState("");
  const [preview, setPreview] = useState("{{base_url}}/products/{{product_id}}");
  const [previewResult, setPreviewResult] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const next = await invoke<Environment[]>("list_environments");
      setEnvironments(next);
      setSelectedId((current) => current && next.some((item) => item.id === current) ? current : next[0]?.id ?? null);
      setError(null);
    } catch (value) {
      setError(formatInvokeError(value));
    }
  }, []);

  useEffect(() => { void refresh(); }, [refresh]);

  const loadSnapshot = useCallback(async () => {
    if (!selectedId) { setSnapshot(null); return; }
    try {
      setSnapshot(await invoke<EnvironmentSnapshot>("environment_snapshot", { environmentId: selectedId }));
    } catch (value) { setError(formatInvokeError(value)); }
  }, [selectedId]);

  useEffect(() => { void loadSnapshot(); }, [loadSnapshot]);

  async function createEnvironment() {
    if (!newName.trim()) return;
    setBusy(true);
    try {
      const created = await invoke<Environment>("upsert_environment", { input: { id: null, name: newName.trim(), isActive: environments.length === 0 } });
      setNewName("");
      await refresh();
      setSelectedId(created.id);
    } catch (value) { setError(formatInvokeError(value)); } finally { setBusy(false); }
  }

  async function setActive() {
    if (!selectedId) return;
    setBusy(true);
    try { await invoke("set_active_environment", { id: selectedId }); await refresh(); await loadSnapshot(); } catch (value) { setError(formatInvokeError(value)); } finally { setBusy(false); }
  }

  async function deleteEnvironment() {
    if (!selectedId || !window.confirm("Delete this environment and its variables?")) return;
    setBusy(true);
    try { await invoke("delete_environment", { id: selectedId }); await refresh(); } catch (value) { setError(formatInvokeError(value)); } finally { setBusy(false); }
  }

  async function addVariable() {
    if (!selectedId) return;
    setBusy(true);
    try { await invoke("upsert_environment_variable", { input: { environmentId: selectedId, key: `variable_${(snapshot?.variables.length ?? 0) + 1}`, value: "", isSecret: false, enabled: true, sortOrder: snapshot?.variables.length ?? 0 } }); await loadSnapshot(); } catch (value) { setError(formatInvokeError(value)); } finally { setBusy(false); }
  }

  async function saveVariable(variable: EnvironmentVariable, patch: Partial<EnvironmentVariable>, secretValue?: string) {
    if (!selectedId) return;
    const next = { ...variable, ...patch };
    try {
      await invoke("upsert_environment_variable", { input: { environmentId: selectedId, key: next.key, value: next.isSecret ? (secretValue ?? null) : next.value, isSecret: next.isSecret, enabled: next.enabled, sortOrder: next.sortOrder } });
      await loadSnapshot();
    } catch (value) { setError(formatInvokeError(value)); }
  }

  async function deleteVariable(variable: EnvironmentVariable) {
    try { await invoke("delete_environment_variable", { id: variable.id }); await loadSnapshot(); } catch (value) { setError(formatInvokeError(value)); }
  }

  async function runPreview() {
    try {
      const result = await invoke<{ value: string; usedSecret: boolean; missingVariables: string[] }>("interpolate_with_active_environment", { value: preview });
      setPreviewResult(`${result.value}${result.usedSecret ? " · includes Keychain secret" : ""}${result.missingVariables.length ? ` · missing: ${result.missingVariables.join(", ")}` : ""}`);
    } catch (value) { setError(formatInvokeError(value)); }
  }

  return <div className="workspace-two-pane"><aside className="workspace-list-pane"><div className="workspace-pane-heading"><div><strong>Environments</strong><span>{environments.length} local environments</span></div></div><div className="workspace-create-row"><input className="text-input" value={newName} onChange={(event) => setNewName(event.target.value)} placeholder="New environment" /><button className="primary compact" onClick={() => void createEnvironment()}>Add</button></div><div className="workspace-item-list">{environments.map((environment) => <button key={environment.id} className={selectedId === environment.id ? "workspace-list-item selected" : "workspace-list-item"} onClick={() => setSelectedId(environment.id)}><span><strong>{environment.name}</strong><small>{environment.isActive ? "Active for Replay" : "Inactive"}</small></span>{environment.isActive ? <span className="status-badge status-active">active</span> : null}</button>)}</div></aside><div className="workspace-detail-pane">{error ? <div className="error-banner">{error}</div> : null}{snapshot ? <><div className="workspace-detail-heading"><div><span className="eyebrow">Environment</span><h2>{snapshot.environment.name}</h2></div></div><div className="workspace-actions"><button className="primary" onClick={() => void setActive()} disabled={snapshot.environment.isActive || busy}>Set active</button><button className="secondary" onClick={() => void addVariable()} disabled={busy}>Add variable</button><button className="secondary danger-action" onClick={() => void deleteEnvironment()} disabled={busy}>Delete</button></div><div className="variable-list">{snapshot.variables.map((variable) => <VariableRow key={variable.id} variable={variable} onSave={saveVariable} onDelete={deleteVariable} />)}</div><section className="interpolation-preview"><h3>Active environment preview</h3><div className="workspace-create-row"><input className="text-input" value={preview} onChange={(event) => setPreview(event.target.value)} /><button className="secondary" onClick={() => void runPreview()}>Resolve</button></div>{previewResult ? <p className="muted-copy">{previewResult}</p> : null}</section></> : <p className="empty-state">Create an environment to manage variables.</p>}</div></div>;
}

function VariableRow({ variable, onSave, onDelete }: { variable: EnvironmentVariable; onSave: (variable: EnvironmentVariable, patch: Partial<EnvironmentVariable>, secretValue?: string) => Promise<void>; onDelete: (variable: EnvironmentVariable) => Promise<void> }) {
  const [key, setKey] = useState(variable.key);
  const [value, setValue] = useState(variable.isSecret ? "" : variable.value ?? "");
  const [isSecret, setIsSecret] = useState(variable.isSecret);
  return <div className="variable-row"><input className="text-input" value={key} onChange={(event) => setKey(event.target.value)} /><input className="text-input" type={isSecret ? "password" : "text"} value={value} onChange={(event) => setValue(event.target.value)} placeholder={variable.isSecret ? "Stored in macOS Keychain · enter to replace" : "Value"} /><label className="inline-toggle"><input type="checkbox" checked={isSecret} onChange={(event) => setIsSecret(event.target.checked)} /> Secret</label><label className="inline-toggle"><input type="checkbox" checked={variable.enabled} onChange={(event) => void onSave(variable, { key, isSecret, enabled: event.target.checked }, value || undefined)} /> Enabled</label><button className="secondary compact" onClick={() => void onSave(variable, { key, isSecret }, value || undefined)}>Save</button><button className="icon-button" onClick={() => void onDelete(variable)}>×</button></div>;
}

function formatTimestamp(value: string) { const numeric = Number(value); return Number.isFinite(numeric) && numeric > 0 ? new Date(numeric).toLocaleString() : value; }
function formatInvokeError(value: unknown) { if (typeof value === "string") return value; if (value && typeof value === "object" && "message" in value) return String((value as { message: unknown }).message); return String(value); }
