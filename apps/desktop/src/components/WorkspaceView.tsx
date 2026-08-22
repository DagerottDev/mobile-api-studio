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
              <span><strong>{session.name}</strong><small>{session.deviceId ?? "No device"}</small></span>
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
  const selectedIndex = collections.findIndex((collection) => collection.id === selectedId);
  useEffect(() => {
    setName(selected?.name ?? "");
    setDescription(selected?.description ?? "");
    if (!selected) { setRequests([]); return; }
    invoke<SavedRequest[]>("list_saved_requests", { collectionId: selected.id })
      .then(setRequests)
      .catch((value) => setError(formatInvokeError(value)));
  }, [selected?.id, selected?.name, selected?.description]);

  async function createCollection() {
    if (!newCollectionName.trim()) return;
    setBusy(true);
    try {
      const collection = await invoke<SavedCollection>("upsert_collection", {
        input: { id: null, name: newCollectionName, description: null, sortOrder: collections.length },
      });
      setNewCollectionName("");
      await refreshCollections();
      setSelectedId(collection.id);
    } catch (value) {
      setError(formatInvokeError(value));
    } finally { setBusy(false); }
  }

  async function saveCollection() {
    if (!selected) return;
    setBusy(true);
    try {
      await invoke("upsert_collection", {
        input: { id: selected.id, name, description: description.trim() || null, sortOrder: selected.sortOrder },
      });
      await refreshCollections();
    } catch (value) { setError(formatInvokeError(value)); }
    finally { setBusy(false); }
  }

  async function moveCollection(direction: -1 | 1) {
    if (!selected || selectedIndex < 0) return;
    const targetIndex = selectedIndex + direction;
    if (targetIndex < 0 || targetIndex >= collections.length) return;
    const reordered = [...collections];
    const [moved] = reordered.splice(selectedIndex, 1);
    reordered.splice(targetIndex, 0, moved);
    setBusy(true);
    try {
      await Promise.all(
        reordered.map((collection, index) => invoke("upsert_collection", {
          input: {
            id: collection.id,
            name: collection.name,
            description: collection.description,
            sortOrder: index,
          },
        })),
      );
      await refreshCollections();
      setSelectedId(selected.id);
    } catch (value) { setError(formatInvokeError(value)); }
    finally { setBusy(false); }
  }

  async function deleteCollection() {
    if (!selected || !window.confirm(`Delete collection “${selected.name}” and its saved requests?`)) return;
    setBusy(true);
    try {
      await invoke("delete_collection", { id: selected.id });
      await refreshCollections();
    } catch (value) { setError(formatInvokeError(value)); }
    finally { setBusy(false); }
  }

  async function deleteRequest(request: SavedRequest) {
    if (!window.confirm(`Delete saved request “${request.name}”?`)) return;
    try {
      await invoke("delete_saved_request", { id: request.id });
      if (selected) setRequests(await invoke<SavedRequest[]>("list_saved_requests", { collectionId: selected.id }));
    } catch (value) { setError(formatInvokeError(value)); }
  }

  return (
    <div className="workspace-two-pane">
      <aside className="workspace-list-pane">
        <div className="workspace-pane-heading"><div><strong>Collections</strong><span>{collections.length} saved</span></div></div>
        <div className="inline-create-row"><input className="text-input" placeholder="New collection" value={newCollectionName} onChange={(event) => setNewCollectionName(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") void createCollection(); }} /><button className="primary compact" onClick={() => void createCollection()} disabled={busy || !newCollectionName.trim()}>Add</button></div>
        <div className="workspace-item-list">
          {collections.map((collection) => <button key={collection.id} className={selectedId === collection.id ? "workspace-list-item selected" : "workspace-list-item"} onClick={() => setSelectedId(collection.id)}><span><strong>{collection.name}</strong><small>{collection.description || "No description"}</small></span><span>{collection.id === selectedId ? requests.length : ""}</span></button>)}
          {collections.length === 0 ? <p className="empty-state">Create a collection, then save requests from Traffic.</p> : null}
        </div>
      </aside>

      <div className="workspace-detail-pane">
        {error ? <div className="error-banner">{error}</div> : null}
        {selected ? <>
          <div className="workspace-detail-heading"><div><span className="eyebrow">Collection</span><h2>{selected.name}</h2></div><span className="pill">{requests.length} requests</span></div>
          <div className="collection-edit-grid"><label className="field-label">Name<input className="text-input" value={name} onChange={(event) => setName(event.target.value)} /></label><label className="field-label">Description<input className="text-input" value={description} onChange={(event) => setDescription(event.target.value)} /></label></div>
          <div className="workspace-actions">
            <button className="primary" onClick={() => void saveCollection()} disabled={busy}>Save collection</button>
            <button className="secondary" onClick={() => void moveCollection(-1)} disabled={busy || selectedIndex <= 0}>Move up</button>
            <button className="secondary" onClick={() => void moveCollection(1)} disabled={busy || selectedIndex < 0 || selectedIndex >= collections.length - 1}>Move down</button>
            <button className="secondary danger-action" onClick={() => void deleteCollection()} disabled={busy}>Delete collection</button>
          </div>
          <div className="saved-request-list">
            {requests.map((request) => (
              <div className="saved-request-row" key={request.id}>
                <span className={`method method-${request.method.toLowerCase()}`}>{request.method}</span>
                <div><strong>{request.name}</strong><small>{request.url}</small></div>
                <div className="saved-request-actions">
                  {onOpenReplay ? <button className="primary compact" onClick={() => onOpenReplay(request.id)}>Open in Replay</button> : null}
                  <button className="secondary compact" onClick={() => void deleteRequest(request)}>Delete</button>
                </div>
              </div>
            ))}
            {requests.length === 0 ? <p className="empty-state">No requests saved here yet. Open Traffic and save a captured request into this collection.</p> : null}
          </div>
        </> : <p className="empty-state">Select a collection.</p>}
      </div>
    </div>
  );
}

function EnvironmentsPanel() {
  const [environments, setEnvironments] = useState<Environment[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [snapshot, setSnapshot] = useState<EnvironmentSnapshot | null>(null);
  const [newEnvironment, setNewEnvironment] = useState("");
  const [variableKey, setVariableKey] = useState("");
  const [variableValue, setVariableValue] = useState("");
  const [variableSecret, setVariableSecret] = useState(false);
  const [previewTemplate, setPreviewTemplate] = useState("{{base_url}}/v1/resource");
  const [previewResult, setPreviewResult] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const next = await invoke<Environment[]>("list_environments");
      setEnvironments(next);
      setSelectedId((current) => current && next.some((item) => item.id === current) ? current : next.find((item) => item.isActive)?.id ?? next[0]?.id ?? null);
      setError(null);
    } catch (value) { setError(formatInvokeError(value)); }
  }, []);

  useEffect(() => { void refresh(); }, [refresh]);
  const selected = environments.find((environment) => environment.id === selectedId) ?? null;
  const refreshSnapshot = useCallback(async (environmentId: string) => {
    try { setSnapshot(await invoke<EnvironmentSnapshot>("environment_snapshot", { environmentId })); }
    catch (value) { setError(formatInvokeError(value)); }
  }, []);
  useEffect(() => { if (selectedId) void refreshSnapshot(selectedId); else setSnapshot(null); }, [selectedId, refreshSnapshot]);

  async function createEnvironment() {
    if (!newEnvironment.trim()) return;
    setBusy(true);
    try {
      const environment = await invoke<Environment>("upsert_environment", { input: { id: null, name: newEnvironment, isActive: environments.length === 0 } });
      setNewEnvironment(""); await refresh(); setSelectedId(environment.id);
    } catch (value) { setError(formatInvokeError(value)); }
    finally { setBusy(false); }
  }

  async function activate() {
    if (!selected) return;
    try { await invoke("set_active_environment", { id: selected.id }); await refresh(); }
    catch (value) { setError(formatInvokeError(value)); }
  }

  async function deleteEnvironment() {
    if (!selected || !window.confirm(`Delete environment “${selected.name}”?`)) return;
    try { await invoke("delete_environment", { id: selected.id }); await refresh(); }
    catch (value) { setError(formatInvokeError(value)); }
  }

  async function saveVariable() {
    if (!selected || !variableKey.trim()) return;
    setBusy(true);
    try {
      await invoke<EnvironmentVariable>("upsert_environment_variable", { input: { environmentId: selected.id, key: variableKey, value: variableValue || null, isSecret: variableSecret, enabled: true, sortOrder: null } });
      setVariableKey(""); setVariableValue(""); setVariableSecret(false); await refreshSnapshot(selected.id);
    } catch (value) { setError(formatInvokeError(value)); }
    finally { setBusy(false); }
  }

  async function deleteVariable(variable: EnvironmentVariable) {
    if (!selected) return;
    try { await invoke("delete_environment_variable", { id: variable.id, environmentId: selected.id }); await refreshSnapshot(selected.id); }
    catch (value) { setError(formatInvokeError(value)); }
  }

  async function previewInterpolation() {
    try {
      const result = await invoke<{ value: string; usedSecret: boolean; missingVariables: string[] }>("interpolate_with_active_environment", { template: previewTemplate });
      setPreviewResult(`${result.value}${result.missingVariables.length ? `\nMissing: ${result.missingVariables.join(", ")}` : ""}${result.usedSecret ? "\nUses secure secret values" : ""}`);
    } catch (value) { setError(formatInvokeError(value)); }
  }

  return (
    <div className="workspace-two-pane">
      <aside className="workspace-list-pane">
        <div className="workspace-pane-heading"><div><strong>Environments</strong><span>{environments.length} configured</span></div></div>
        <div className="inline-create-row"><input className="text-input" placeholder="e.g. Staging" value={newEnvironment} onChange={(event) => setNewEnvironment(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") void createEnvironment(); }} /><button className="primary compact" onClick={() => void createEnvironment()} disabled={busy || !newEnvironment.trim()}>Add</button></div>
        <div className="workspace-item-list">{environments.map((environment) => <button key={environment.id} className={selectedId === environment.id ? "workspace-list-item selected" : "workspace-list-item"} onClick={() => setSelectedId(environment.id)}><span><strong>{environment.name}</strong><small>{environment.isActive ? "Active environment" : "Inactive"}</small></span>{environment.isActive ? <span className="status-badge status-active">active</span> : null}</button>)}{environments.length === 0 ? <p className="empty-state">Create an environment for reusable API variables.</p> : null}</div>
      </aside>

      <div className="workspace-detail-pane">
        {error ? <div className="error-banner">{error}</div> : null}
        {selected && snapshot ? <>
          <div className="workspace-detail-heading"><div><span className="eyebrow">Environment</span><h2>{selected.name}</h2></div>{selected.isActive ? <span className="status-badge status-active">active</span> : <button className="secondary compact" onClick={() => void activate()}>Make active</button>}</div>
          <div className="variable-create-grid"><input className="text-input" placeholder="variable_name" value={variableKey} onChange={(event) => setVariableKey(event.target.value)} /><input className="text-input" type={variableSecret ? "password" : "text"} placeholder={variableSecret ? "Secret value → Keychain" : "Value"} value={variableValue} onChange={(event) => setVariableValue(event.target.value)} /><label className="inline-toggle"><input type="checkbox" checked={variableSecret} onChange={(event) => setVariableSecret(event.target.checked)} /> Secret</label><button className="primary compact" onClick={() => void saveVariable()} disabled={busy || !variableKey.trim()}>Add variable</button></div>
          <div className="environment-variable-list">{snapshot.variables.map((variable) => <div className="environment-variable-row" key={variable.id}><code>{`{{${variable.key}}}`}</code><span>{variable.isSecret ? "•••••••• · Keychain" : variable.value ?? ""}</span><span>{variable.enabled ? "enabled" : "disabled"}</span><button className="secondary compact" onClick={() => void deleteVariable(variable)}>Delete</button></div>)}{snapshot.variables.length === 0 ? <p className="empty-state">No variables yet.</p> : null}</div>
          <section className="interpolation-preview"><h3>Interpolation preview</h3><div className="inline-create-row"><input className="text-input" value={previewTemplate} onChange={(event) => setPreviewTemplate(event.target.value)} /><button className="secondary compact" onClick={() => void previewInterpolation()}>Resolve active environment</button></div>{previewResult ? <pre>{previewResult}</pre> : null}</section>
          <div className="workspace-actions"><button className="secondary danger-action" onClick={() => void deleteEnvironment()}>Delete environment</button></div>
        </> : <p className="empty-state">Select an environment.</p>}
      </div>
    </div>
  );
}

function formatTimestamp(value: string) {
  const numeric = Number(value);
  const date = Number.isFinite(numeric) ? new Date(numeric) : new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

function formatInvokeError(value: unknown) {
  if (typeof value === "string") return value;
  if (value && typeof value === "object" && "message" in value) return String((value as { message: unknown }).message);
  return String(value);
}
