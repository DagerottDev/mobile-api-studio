import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useMemo, useState } from "react";
import type { MockHeaderMutation, MockJsonMutation, MockRule } from "../mockTypes";

export function MocksView() {
  const [rules, setRules] = useState<MockRule[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [draft, setDraft] = useState<MockRule | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const next = await invoke<MockRule[]>("list_mock_rules");
      setRules(next);
      setSelectedId((current) => current && next.some((rule) => rule.id === current) ? current : next[0]?.id ?? null);
      setError(null);
    } catch (value) {
      setError(formatInvokeError(value));
    }
  }, []);

  useEffect(() => { void refresh(); }, [refresh]);

  const selected = useMemo(
    () => rules.find((rule) => rule.id === selectedId) ?? null,
    [rules, selectedId],
  );

  useEffect(() => {
    setDraft(selected ? structuredClone(selected) : null);
  }, [selected]);

  const activeCount = rules.filter((rule) => rule.enabled).length;
  const selectedIndex = rules.findIndex((rule) => rule.id === selectedId);

  function createRule() {
    const timestamp = Date.now().toString();
    const rule: MockRule = {
      schemaVersion: 1,
      id: `mock-${Date.now()}-${Math.random().toString(16).slice(2)}`,
      name: "New mock rule",
      enabled: true,
      priority: rules.length,
      method: "GET",
      host: null,
      pathPattern: "/",
      pathMatch: "exact",
      statusCode: 200,
      responseHeaders: [],
      responseBody: null,
      jsonMutations: [],
      latencyMs: null,
      failureMode: "none",
      requestBreakpoint: false,
      responseBreakpoint: false,
      sourceFlowId: null,
      createdAt: timestamp,
      updatedAt: timestamp,
    };
    setRules((current) => [...current, rule]);
    setSelectedId(rule.id);
    setDraft(rule);
  }

  async function save() {
    if (!draft) return;
    setBusy(true);
    try {
      const saved = await invoke<MockRule>("upsert_mock_rule", {
        rule: { ...draft, updatedAt: Date.now().toString() },
      });
      setMessage(`Saved ${saved.name}. Active captures reload mock rules automatically.`);
      await refresh();
      setSelectedId(saved.id);
    } catch (value) {
      setError(formatInvokeError(value));
    } finally {
      setBusy(false);
    }
  }

  async function toggleEnabled(rule: MockRule, enabled: boolean) {
    try {
      await invoke("set_mock_rule_enabled", { id: rule.id, enabled });
      await refresh();
    } catch (value) {
      setError(formatInvokeError(value));
    }
  }

  async function move(direction: -1 | 1) {
    if (!selected || selectedIndex < 0) return;
    const target = selectedIndex + direction;
    if (target < 0 || target >= rules.length) return;
    const reordered = [...rules];
    const [moved] = reordered.splice(selectedIndex, 1);
    reordered.splice(target, 0, moved);
    setBusy(true);
    try {
      for (const [index, rule] of reordered.entries()) {
        await invoke("set_mock_rule_priority", { id: rule.id, priority: index });
      }
      await refresh();
      setSelectedId(selected.id);
    } catch (value) {
      setError(formatInvokeError(value));
    } finally {
      setBusy(false);
    }
  }

  async function remove() {
    if (!selected || !window.confirm(`Delete mock rule “${selected.name}”?`)) return;
    setBusy(true);
    try {
      await invoke("delete_mock_rule", { id: selected.id });
      await refresh();
    } catch (value) {
      setError(formatInvokeError(value));
    } finally {
      setBusy(false);
    }
  }

  async function disableAll() {
    if (activeCount === 0) return;
    setBusy(true);
    try {
      const changed = await invoke<number>("disable_all_mocks");
      setMessage(`Disabled ${changed} active mock rule${changed === 1 ? "" : "s"}.`);
      await refresh();
    } catch (value) {
      setError(formatInvokeError(value));
    } finally {
      setBusy(false);
    }
  }

  function patch(patchValue: Partial<MockRule>) {
    setDraft((current) => current ? { ...current, ...patchValue } : current);
  }

  function updateHeader(index: number, patchValue: Partial<MockHeaderMutation>) {
    if (!draft) return;
    patch({ responseHeaders: draft.responseHeaders.map((header, i) => i === index ? { ...header, ...patchValue } : header) });
  }

  function updateMutation(index: number, patchValue: Partial<MockJsonMutation>) {
    if (!draft) return;
    patch({ jsonMutations: draft.jsonMutations.map((mutation, i) => i === index ? { ...mutation, ...patchValue } : mutation) });
  }

  return (
    <section className="mocks-layout panel">
      <aside className="mocks-list-pane">
        <div className="panel-heading">
          <div><strong>Mock rules</strong><span>{activeCount} active · first match wins</span></div>
          <button className="primary compact" onClick={createRule}>New rule</button>
        </div>
        <div className="mocks-safety-bar">
          <span>Safety</span>
          <button className="secondary compact danger-action" onClick={() => void disableAll()} disabled={busy || activeCount === 0}>Disable all mocks</button>
        </div>
        <div className="mock-rule-list">
          {rules.map((rule, index) => (
            <button key={rule.id} className={selectedId === rule.id ? "mock-rule-row selected" : "mock-rule-row"} onClick={() => setSelectedId(rule.id)}>
              <input type="checkbox" checked={rule.enabled} onChange={(event) => { event.stopPropagation(); void toggleEnabled(rule, event.target.checked); }} onClick={(event) => event.stopPropagation()} aria-label={`Enable ${rule.name}`} />
              <span className="mock-priority">{index + 1}</span>
              <span className="mock-rule-copy"><strong>{rule.name}</strong><small>{rule.method ?? "ANY"} {rule.host ?? "*"}{rule.pathPattern}</small></span>
              <span className={`mock-mode mock-${rule.failureMode}`}>{rule.failureMode === "none" ? (rule.statusCode ?? "mutate") : rule.failureMode}</span>
            </button>
          ))}
          {rules.length === 0 ? <p className="empty-state">Create a rule here or create one from a captured response in Traffic.</p> : null}
        </div>
      </aside>

      <div className="mock-editor-pane">
        {error ? <div className="error-banner">{error}</div> : null}
        {message ? <div className="settings-message mock-message">{message}</div> : null}
        {draft ? (
          <div className="mock-editor-scroll">
            <div className="mock-editor-heading">
              <div><span className="eyebrow">Mock rule</span><h2>{draft.name}</h2></div>
              <div className="mock-heading-actions">
                <button className="secondary compact" onClick={() => void move(-1)} disabled={busy || selectedIndex <= 0}>Move up</button>
                <button className="secondary compact" onClick={() => void move(1)} disabled={busy || selectedIndex < 0 || selectedIndex >= rules.length - 1}>Move down</button>
                <button className="primary compact" onClick={() => void save()} disabled={busy}>Save</button>
              </div>
            </div>

            <section className="mock-section">
              <h3>Match</h3>
              <div className="mock-grid two-column">
                <label className="field-label">Name<input className="text-input" value={draft.name} onChange={(event) => patch({ name: event.target.value })} /></label>
                <label className="field-label">Method<input className="text-input" value={draft.method ?? ""} onChange={(event) => patch({ method: event.target.value.trim() || null })} placeholder="Any method" /></label>
                <label className="field-label">Host<input className="text-input" value={draft.host ?? ""} onChange={(event) => patch({ host: event.target.value.trim() || null })} placeholder="Any host" /></label>
                <label className="field-label">Path matching<select value={draft.pathMatch} onChange={(event) => patch({ pathMatch: event.target.value as MockRule["pathMatch"] })}><option value="exact">Exact path</option><option value="normalized">Normalized endpoint</option></select></label>
              </div>
              <label className="field-label">Path pattern<input className="text-input" value={draft.pathPattern} onChange={(event) => patch({ pathPattern: event.target.value })} placeholder="/products/:id" /></label>
            </section>

            <section className="mock-section">
              <h3>Behavior</h3>
              <div className="mock-grid three-column">
                <label className="field-label">Failure mode<select value={draft.failureMode} onChange={(event) => patch({ failureMode: event.target.value as MockRule["failureMode"] })}><option value="none">Normal response</option><option value="drop">Drop connection</option><option value="timeout">Timeout then drop</option></select></label>
                <label className="field-label">Status override<input className="text-input" type="number" min="100" max="599" value={draft.statusCode ?? ""} onChange={(event) => patch({ statusCode: event.target.value ? Number(event.target.value) : null })} /></label>
                <label className="field-label">Latency / timeout ms<input className="text-input" type="number" min="0" value={draft.latencyMs ?? ""} onChange={(event) => patch({ latencyMs: event.target.value ? Math.max(0, Number(event.target.value)) : null })} /></label>
              </div>
            </section>

            <section className="mock-section">
              <div className="mock-section-heading"><div><h3>Response body override</h3><span>Captured binary bodies remain base64; text bodies are directly editable.</span></div>{draft.responseBody ? <button className="secondary compact" onClick={() => patch({ responseBody: null })}>Remove override</button> : <button className="secondary compact" onClick={() => patch({ responseBody: { contentType: "application/json", encoding: "text", data: "{}" } })}>Add override</button>}</div>
              {draft.responseBody ? <><div className="mock-grid two-column"><label className="field-label">Content type<input className="text-input" value={draft.responseBody.contentType ?? ""} onChange={(event) => patch({ responseBody: { ...draft.responseBody!, contentType: event.target.value || null } })} /></label><label className="field-label">Encoding<select value={draft.responseBody.encoding} onChange={(event) => patch({ responseBody: { ...draft.responseBody!, encoding: event.target.value as "text" | "base64" } })}><option value="text">Text</option><option value="base64">Base64</option></select></label></div><textarea className="mock-body-editor" value={draft.responseBody.data} onChange={(event) => patch({ responseBody: { ...draft.responseBody!, data: event.target.value } })} spellCheck={false} /></> : <p className="muted-copy">Leave empty to preserve the backend response body.</p>}
            </section>

            <section className="mock-section">
              <div className="mock-section-heading"><div><h3>Response headers</h3><span>Set, replace, or remove headers after the backend response arrives.</span></div><button className="secondary compact" onClick={() => patch({ responseHeaders: [...draft.responseHeaders, { name: "", value: "", remove: false }] })}>Add header mutation</button></div>
              <div className="mock-mutation-list">
                {draft.responseHeaders.map((header, index) => <div className="mock-header-row" key={index}><input className="text-input" value={header.name} onChange={(event) => updateHeader(index, { name: event.target.value })} placeholder="Header" /><input className="text-input" value={header.value ?? ""} disabled={header.remove} onChange={(event) => updateHeader(index, { value: event.target.value })} placeholder="Value" /><label className="inline-toggle"><input type="checkbox" checked={header.remove} onChange={(event) => updateHeader(index, { remove: event.target.checked })} /> Remove</label><button className="icon-button" onClick={() => patch({ responseHeaders: draft.responseHeaders.filter((_, i) => i !== index) })}>×</button></div>)}
              </div>
            </section>

            <section className="mock-section">
              <div className="mock-section-heading"><div><h3>JSON mutations</h3><span>RFC 6901 pointer paths mutate JSON after an optional body override.</span></div><button className="secondary compact" onClick={() => patch({ jsonMutations: [...draft.jsonMutations, { pointer: "/", value: null, remove: false }] })}>Add JSON mutation</button></div>
              <div className="mock-mutation-list">
                {draft.jsonMutations.map((mutation, index) => <div className="mock-json-row" key={index}><input className="text-input" value={mutation.pointer} onChange={(event) => updateMutation(index, { pointer: event.target.value })} placeholder="/data/status" /><input className="text-input" value={mutation.remove ? "" : stringifyValue(mutation.value)} disabled={mutation.remove} onChange={(event) => updateMutation(index, { value: parseValue(event.target.value) })} placeholder='"unavailable" or 42' /><label className="inline-toggle"><input type="checkbox" checked={mutation.remove} onChange={(event) => updateMutation(index, { remove: event.target.checked })} /> Remove</label><button className="icon-button" onClick={() => patch({ jsonMutations: draft.jsonMutations.filter((_, i) => i !== index) })}>×</button></div>)}
              </div>
            </section>

            <section className="mock-section breakpoint-preview">
              <h3>Breakpoints</h3>
              <p className="muted-copy">The rule model already reserves request and response breakpoint flags. Interactive pause/edit/resume transport is the next Phase 3 slice; these controls stay off until that runtime is connected.</p>
            </section>

            <div className="workspace-actions"><button className="secondary danger-action" onClick={() => void remove()} disabled={busy}>Delete rule</button></div>
          </div>
        ) : <p className="empty-state">Select or create a mock rule.</p>}
      </div>
    </section>
  );
}

function stringifyValue(value: unknown) {
  if (typeof value === "string") return JSON.stringify(value);
  try { return JSON.stringify(value); } catch { return String(value ?? ""); }
}

function parseValue(value: string): unknown {
  try { return JSON.parse(value); } catch { return value; }
}

function formatInvokeError(value: unknown) {
  if (typeof value === "string") return value;
  if (value && typeof value === "object" && "message" in value) return String((value as { message: unknown }).message);
  return String(value);
}
