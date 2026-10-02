import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "../api/invoke";
import type { MockRule } from "../mockTypes";
import type { PatternKind, ProxyRule, ProxyRuleAction, RuleHeaderMutation, RulePattern } from "../proxyTypes";

export function ProxyRulesView() {
  const [rules, setRules] = useState<ProxyRule[]>([]);
  const [mocks, setMocks] = useState<MockRule[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [newRuleId, setNewRuleId] = useState<string | null>(null);
  const [draft, setDraft] = useState<ProxyRule | null>(null);
  const [preview, setPreview] = useState({ method: "GET", host: "api.example.com", path: "/v1/users" });
  const [previewMatched, setPreviewMatched] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [scriptExport, setScriptExport] = useState("");
  const [diagnostics, setDiagnostics] = useState<{ code: string; message: string }[]>([]);

  const refresh = useCallback(async () => {
    try {
      const [nextRules, nextMocks] = await Promise.all([
        invoke<ProxyRule[]>("list_proxy_rules"), invoke<MockRule[]>("list_mock_rules"),
      ]);
      setRules((current) => JSON.stringify(current) === JSON.stringify(nextRules) ? current : nextRules);
      setMocks((current) => JSON.stringify(current) === JSON.stringify(nextMocks) ? current : nextMocks);
      setSelectedId((current) => current && nextRules.some((rule) => rule.id === current) ? current : nextRules[0]?.id ?? null);
      setError(null);
    } catch (value) { setError(formatError(value)); }
  }, []);

  useEffect(() => { void refresh(); }, [refresh]);
  useEffect(() => {
    const onFocus = () => { if (!newRuleId) void refresh(); };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refresh, newRuleId]);
  useEffect(() => {
    const loadDiagnostics = async () => {
      try {
        const recent = (await invoke<{ code: string; message: string }[]>("list_proxy_rule_diagnostics")).slice(-3);
        setDiagnostics((current) => JSON.stringify(current) === JSON.stringify(recent) ? current : recent);
      }
      catch { /* The rule editor remains usable if diagnostics are unavailable. */ }
    };
    void loadDiagnostics();
    const timer = window.setInterval(() => void loadDiagnostics(), 5000);
    return () => window.clearInterval(timer);
  }, []);
  const selected = useMemo(() => rules.find((rule) => rule.id === selectedId) ?? null, [rules, selectedId]);
  useEffect(() => { setDraft(selected ? structuredClone(selected) : null); setPreviewMatched(null); setScriptExport(""); }, [selected]);

  const combined = [
    ...rules.map((rule) => ({ id: rule.id, name: rule.name, priority: rule.priority, createdAt: rule.createdAt, kind: "proxy" as const, enabled: rule.enabled })),
    ...mocks.map((rule) => ({ id: rule.id, name: rule.name, priority: rule.priority, createdAt: rule.createdAt, kind: "mock" as const, enabled: rule.enabled })),
  ].sort((a, b) => a.priority - b.priority || a.createdAt.localeCompare(b.createdAt) || a.id.localeCompare(b.id));

  function createRule() {
    const now = Date.now().toString();
    const rule: ProxyRule = {
      schemaVersion: 1, id: `proxy-${now}-${Math.random().toString(16).slice(2)}`, name: "New block rule",
      enabled: true, priority: combined.length,
      matcher: { method: null, host: { kind: "wildcard", value: "*" }, path: { kind: "wildcard", value: "*" } },
      action: { type: "block", statusCode: 403 }, createdAt: now, updatedAt: now,
    };
    setRules((current) => [...current, rule]); setSelectedId(rule.id); setDraft(rule); setNewRuleId(rule.id); setPreviewMatched(null);
  }

  async function save(rule = draft) {
    if (!rule) return;
    setBusy(true);
    try {
      const saved = await invoke<ProxyRule>("upsert_proxy_rule", { rule: { ...rule, updatedAt: Date.now().toString() } });
      if (saved.id === newRuleId) setNewRuleId(null);
      setMessage(`Saved ${saved.name}. New requests and TLS connections use the new order.`);
      await refresh();
      if (rule === draft) setSelectedId(saved.id);
    } catch (value) { setError(formatError(value)); } finally { setBusy(false); }
  }

  async function remove() {
    if (!draft || !window.confirm(`Delete proxy rule “${draft.name}”?`)) return;
    if (draft.id === newRuleId) {
      setRules((current) => current.filter((rule) => rule.id !== draft.id));
      setSelectedId(null); setNewRuleId(null); setMessage("Unsaved rule discarded.");
      return;
    }
    setBusy(true);
    try { await invoke("delete_proxy_rule", { id: draft.id }); setMessage("Proxy rule deleted."); await refresh(); }
    catch (value) { setError(formatError(value)); } finally { setBusy(false); }
  }

  async function disableAll() {
    setBusy(true);
    try { const count = await invoke<number>("disable_all_proxy_rules"); setMessage(`Disabled ${count} proxy rules.`); await refresh(); setNewRuleId(null); }
    catch (value) { setError(formatError(value)); } finally { setBusy(false); }
  }

  async function testMatch() {
    if (!draft) return;
    try {
      const result = await invoke<{ matched: boolean }>("preview_proxy_rule", { rule: draft, request: preview });
      setPreviewMatched(result.matched); setError(null);
    } catch (value) { setError(formatError(value)); setPreviewMatched(null); }
  }

  async function previewScriptExport() {
    if (!draft) return;
    setBusy(true); setScriptExport("");
    try { const bundle = await invoke("export_selected_script_rules", { ids: [draft.id] }); setScriptExport(JSON.stringify(bundle, null, 2)); setError(null); }
    catch (value) { setError(formatError(value)); }
    finally { setBusy(false); }
  }
  function downloadScriptExport() {
    const url = URL.createObjectURL(new Blob([scriptExport], { type: "application/json" }));
    const link = document.createElement("a"); link.href = url; link.download = "mobile-api-studio-scripts.mas.json"; link.click(); URL.revokeObjectURL(url);
  }

  function patchPattern(field: "host" | "path", patch: Partial<RulePattern>) {
    setDraft((current) => current ? { ...current, matcher: { ...current.matcher, [field]: { ...current.matcher[field], ...patch } } } : null);
    setPreviewMatched(null);
  }

  function setAction(type: ProxyRuleAction["type"]) {
    const action: ProxyRuleAction = type === "block" ? { type, statusCode: 403 }
      : type === "map_local" ? { type, path: "" }
      : type === "map_remote" ? { type, url: "" }
      : type === "rewrite_request" || type === "rewrite_response" ? { type, headers: [], body: null }
      : type === "script_hook" ? { type, stage: "request", script: "function transform(event) { return event; }" }
      : type === "breakpoint" ? { type, stage: "request" }
      : type === "dns_override" ? { type, address: "" }
      : type === "inspect_https" ? { type, enabled: false }
      : type === "allow" ? { type: "allow" }
      : type === "no_cache" ? { type: "no_cache" }
      : { type: "block_cookies" };
    const connectionMethod = type === "dns_override" ? "DNS" : type === "inspect_https" ? "TLS" : null;
    setDraft((current) => current ? { ...current, action, matcher: connectionMethod ? { ...current.matcher, method: connectionMethod, path: { kind: "wildcard", value: "*" } } : current.action.type === "dns_override" || current.action.type === "inspect_https" ? { ...current.matcher, method: null } : current.matcher } : null);
    if (connectionMethod) setPreview((current) => ({ ...current, method: connectionMethod, path: "/" }));
    setPreviewMatched(null);
  }

  function updateHeader(index: number, patch: Partial<RuleHeaderMutation>) {
    setDraft((current) => current && (current.action.type === "rewrite_request" || current.action.type === "rewrite_response")
      ? { ...current, action: { ...current.action, headers: current.action.headers.map((header, position) => position === index ? { ...header, ...patch } : header) } }
      : current);
  }

  function patchRewrite(patch: Partial<{ headers: RuleHeaderMutation[]; body: string | null }>) {
    setDraft((current) => current && (current.action.type === "rewrite_request" || current.action.type === "rewrite_response")
      ? { ...current, action: { ...current.action, ...patch } }
      : current);
  }

  function addHeader() {
    if (draft?.action.type === "rewrite_request" || draft?.action.type === "rewrite_response")
      patchRewrite({ headers: [...draft.action.headers, { name: "", value: "", remove: false }] });
  }

  function removeHeader(index: number) {
    if (draft?.action.type === "rewrite_request" || draft?.action.type === "rewrite_response")
      patchRewrite({ headers: draft.action.headers.filter((_, position) => position !== index) });
  }

  async function importLocalFile(file: File) {
    if (file.size > 2 * 1024 * 1024) { setError("Choose a file no larger than 2 MiB."); return; }
    const importingRuleId = draft?.id;
    setBusy(true); setError(null); setMessage(null);
    try {
      const dataBase64 = await new Promise<string>((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () => typeof reader.result === "string" ? resolve(reader.result.split(",", 2)[1] ?? "") : reject(new Error("Could not read the file."));
        reader.onerror = () => reject(reader.error ?? new Error("Could not read the file."));
        reader.readAsDataURL(file);
      });
      const { filename } = await invoke<{ filename: string }>("import_proxy_map", { name: file.name, dataBase64 });
      setDraft((current) => current && current.id === importingRuleId && current.action.type === "map_local" ? { ...current, action: { type: "map_local", path: filename } } : current);
      setMessage(`Imported ${filename}. Save the rule to apply it.`);
    } catch (value) { setError(formatError(value)); } finally { setBusy(false); }
  }

  return <section className="mocks-layout panel" aria-label="Ordered proxy rules">
    <aside className="mocks-list-pane">
      <div className="panel-heading"><div><strong>Ordered rules</strong><span>{rules.filter((rule) => rule.enabled).length} proxy · {mocks.filter((rule) => rule.enabled).length} mock active</span></div><div className="mock-heading-actions"><button className="secondary compact" onClick={() => void refresh()} disabled={busy || newRuleId !== null}>Refresh order</button><button className="primary compact" onClick={createRule} disabled={busy || newRuleId !== null}>New rule</button></div></div>
      <div className="mocks-safety-bar"><span>Changes apply to new requests and TLS connections</span><button className="secondary compact danger-action" onClick={() => void disableAll()} disabled={busy || !rules.some((rule) => rule.enabled)}>Disable proxy rules</button></div>
      <div className="mock-rule-list">{combined.map((rule, index) => rule.kind === "proxy" ? <div className={selectedId === rule.id ? "mock-rule-row selected" : "mock-rule-row"} key={`proxy-${rule.id}`}>
        <input type="checkbox" checked={rule.enabled} disabled={busy || (newRuleId !== null && rule.id !== newRuleId)} aria-label={`Enable ${rule.name}`} onChange={(event) => { const item = rules.find((candidate) => candidate.id === rule.id); if (item) void save({ ...item, enabled: event.target.checked }); }} />
        <span className="mock-priority">{index + 1}</span><button className="proxy-rule-select" aria-pressed={selectedId === rule.id} onClick={() => setSelectedId(rule.id)}><strong>{rule.name}</strong><small>Proxy rule{rule.id === newRuleId ? " · unsaved" : ""}</small></button>
      </div> : <div className="mock-rule-row" key={`mock-${rule.id}`}><span aria-label={rule.enabled ? "Enabled mock" : "Disabled mock"} role="img">{rule.enabled ? "●" : "○"}</span><span className="mock-priority">{index + 1}</span><span className="mock-rule-copy"><strong>{rule.name}</strong><small>Existing mock · edit below</small></span></div>)}</div>
    </aside>
    <div className="mock-editor-pane">
      {error ? <div className="error-banner" role="alert">{error}</div> : null}
      {message ? <div className="settings-message mock-message" role="status">{message}</div> : null}
      {diagnostics.length > 0 ? <div className="proxy-diagnostics" role="status"><strong>Recent rule errors</strong>{diagnostics.map((item, index) => <p key={`${item.code}-${index}`}><code>{item.code}</code> {item.message}</p>)}</div> : null}
      {draft ? <div className="mock-editor-scroll">
        <div className="mock-editor-heading"><div><span className="eyebrow">Proxy rule</span><h2>{draft.name}</h2></div><button className="primary compact" onClick={() => void save()} disabled={busy}>Save</button></div>
        <section className="mock-section"><h3>Match</h3><div className="mock-grid two-column">
          <label className="field-label">Name<input className="text-input" value={draft.name} onChange={(event) => setDraft({ ...draft, name: event.target.value })} /></label>
          <label className="field-label">Priority<input className="text-input" type="number" step="1" value={draft.priority} onChange={(event) => setDraft({ ...draft, priority: Number(event.target.value) })} /></label>
          <label className="field-label">Method<input className="text-input" value={draft.matcher.method ?? ""} disabled={draft.action.type === "dns_override" || draft.action.type === "inspect_https"} placeholder="Any method" onChange={(event) => { setDraft({ ...draft, matcher: { ...draft.matcher, method: event.target.value.trim() || null } }); setPreviewMatched(null); }} /></label>
        </div>{(["host", "path"] as const).map((field) => <div className="mock-grid two-column" key={field}>
          <label className="field-label">{field === "host" ? "Host pattern" : "Path pattern"}<input className="text-input" value={draft.matcher[field].value} disabled={field === "path" && (draft.action.type === "dns_override" || draft.action.type === "inspect_https")} onChange={(event) => patchPattern(field, { value: event.target.value })} /></label>
          <label className="field-label">Match type<select value={draft.matcher[field].kind} disabled={field === "path" && (draft.action.type === "dns_override" || draft.action.type === "inspect_https")} onChange={(event) => patchPattern(field, { kind: event.target.value as PatternKind })}><option value="exact">Exact</option><option value="wildcard">Wildcard</option><option value="regex">Regex</option></select></label>
        </div>)}</section>
        <section className="mock-section"><h3>Action</h3><div className="mock-grid two-column">
          <label className="field-label">Behavior<select value={draft.action.type} onChange={(event) => setAction(event.target.value as ProxyRuleAction["type"])}><option value="block">Block</option><option value="allow">Allow</option><option value="map_local">Map local file</option><option value="map_remote">Map remote URL</option><option value="rewrite_request">Rewrite request</option><option value="rewrite_response">Rewrite response</option><option value="breakpoint">Breakpoint</option><option value="script_hook">JavaScript hook</option><option value="no_cache">No cache</option><option value="block_cookies">Block cookies</option><option value="dns_override">DNS override</option><option value="inspect_https">HTTPS inspection</option></select></label>
          {draft.action.type === "block" ? <label className="field-label">Status<input className="text-input" type="number" min="400" max="599" step="1" value={draft.action.statusCode} onChange={(event) => setDraft({ ...draft, action: { type: "block", statusCode: Number(event.target.value) } })} /></label> : null}
          {draft.action.type === "map_local" ? <label className="field-label">Filename in proxy-maps<input className="text-input" value={draft.action.path} onChange={(event) => setDraft({ ...draft, action: { type: "map_local", path: event.target.value } })} /></label> : null}
          {draft.action.type === "map_remote" ? <label className="field-label">Remote URL<input className="text-input" type="url" value={draft.action.url} onChange={(event) => setDraft({ ...draft, action: { type: "map_remote", url: event.target.value } })} /></label> : null}
          {draft.action.type === "breakpoint" ? <label className="field-label">Stage<select value={draft.action.stage} onChange={(event) => setDraft({ ...draft, action: { type: "breakpoint", stage: event.target.value as "request" | "response" } })}><option value="request">Request</option><option value="response">Response</option></select></label> : null}
          {draft.action.type === "dns_override" ? <label className="field-label">IP address<input className="text-input" value={draft.action.address} onChange={(event) => setDraft({ ...draft, action: { type: "dns_override", address: event.target.value } })} placeholder="127.0.0.1 or ::1" /></label> : null}
          {draft.action.type === "inspect_https" ? <label className="field-label">Connection policy<select value={String(draft.action.enabled)} onChange={(event) => setDraft({ ...draft, action: { type: "inspect_https", enabled: event.target.value === "true" } })}><option value="false">Pass encrypted traffic through</option><option value="true">Inspect HTTPS</option></select></label> : null}
        </div>
        {draft.action.type === "script_hook" ? <><label className="field-label">Hook stage<select value={draft.action.stage} onChange={(event) => { if (draft.action.type === "script_hook") setDraft({ ...draft, action: { ...draft.action, stage: event.target.value as "request" | "response" | "websocket" } }); }}><option value="request">Request</option><option value="response">Response</option><option value="websocket">WebSocket message</option></select></label><label className="field-label">JavaScript (64 KiB max)<textarea className="replay-body-editor" value={draft.action.script} onChange={(event) => { if (draft.action.type === "script_hook") setDraft({ ...draft, action: { ...draft.action, script: event.target.value } }); }} /></label><p>Define synchronous transform(event), return the edited event. Runs in a disposable QuickJS worker: 32 MiB heap and 100 ms engine limit, no host APIs. Failure stops the matching flow. Imported rules stay disabled. Exported source can contain secrets you typed; review it before downloading. Import script bundles through Settings.</p><button className="secondary compact" disabled={busy || draft.id === newRuleId} onClick={() => void previewScriptExport()}>Preview stored script export</button>{scriptExport ? <><textarea className="replay-body-editor" readOnly value={scriptExport} aria-label="Selected script export preview" /><button className="secondary compact" disabled={busy} onClick={downloadScriptExport}>Download previewed script bundle</button></> : null}</> : null}
        {draft.action.type === "inspect_https" ? <p className="muted-copy">First matching host rule decides before TLS. Applies to new connections; reconnect clients after changes. Encrypted TCP passthrough produces no HTTP details. UDP/QUIC passthrough is unavailable and stops with a diagnostic.</p> : null}
        {draft.action.type === "dns_override" ? <p className="muted-copy">Applies only to A/AAAA queries sent to the DNS listener. Start that listener from Connect and configure your development client to use it.</p> : null}
        {draft.action.type === "map_local" ? <><label className="field-label">Import local file (2 MiB max)<input type="file" disabled={busy} onChange={(event) => { const file = event.currentTarget.files?.[0]; event.currentTarget.value = ""; if (file) void importLocalFile(file); }} /></label><p className="muted-copy">Choose a file to copy it into the app configuration directory’s proxy-maps folder, or enter an existing filename above.</p></> : null}
        {(draft.action.type === "rewrite_request" || draft.action.type === "rewrite_response") ? <>
          <label className="inline-toggle"><input type="checkbox" checked={draft.action.body !== null} onChange={(event) => patchRewrite({ body: event.target.checked ? "" : null })} /> Replace body</label>
          <label className="field-label">Body override<textarea className="mock-body-editor" disabled={draft.action.body === null} value={draft.action.body ?? ""} onChange={(event) => patchRewrite({ body: event.target.value })} placeholder="Replacement UTF-8 body; empty clears the body" /></label>
          <div className="mock-section-heading"><h3>Header changes</h3><button className="secondary compact" onClick={addHeader}>Add header</button></div>
          <div className="mock-mutation-list">{draft.action.headers.map((header, index) => <div className="mock-header-row" key={index}>
            <input className="text-input" aria-label={`Header ${index + 1} name`} value={header.name} onChange={(event) => updateHeader(index, { name: event.target.value })} placeholder="Header name" />
            <input className="text-input" aria-label={`Header ${index + 1} value`} value={header.value ?? ""} disabled={header.remove} onChange={(event) => updateHeader(index, { value: event.target.value })} placeholder="Value" />
            <label className="inline-toggle"><input type="checkbox" checked={header.remove} onChange={(event) => updateHeader(index, { remove: event.target.checked, value: event.target.checked ? null : "" })} /> Remove</label>
            <button className="icon-button" aria-label={`Delete header ${index + 1}`} onClick={() => removeHeader(index)}>×</button>
          </div>)}</div>
        </> : null}</section>
        <section className="mock-section"><h3>Test this rule</h3><div className="mock-grid three-column">
          <label className="field-label">Method<input className="text-input" value={preview.method} onChange={(event) => { setPreview({ ...preview, method: event.target.value }); setPreviewMatched(null); }} /></label>
          <label className="field-label">Host<input className="text-input" value={preview.host} onChange={(event) => { setPreview({ ...preview, host: event.target.value }); setPreviewMatched(null); }} /></label>
          <label className="field-label">Path<input className="text-input" value={preview.path} onChange={(event) => { setPreview({ ...preview, path: event.target.value }); setPreviewMatched(null); }} /></label>
        </div><button className="secondary compact" onClick={() => void testMatch()}>Preview match</button>{previewMatched !== null ? <span className="proxy-preview-result" role="status">{previewMatched ? "Matches this request" : "Does not match"}</span> : null}</section>
        <div className="workspace-actions"><button className="secondary danger-action" onClick={() => void remove()} disabled={busy}>Delete rule</button></div>
      </div> : <p className="empty-state">Select or create a proxy rule. Existing mocks remain editable below.</p>}
    </div>
  </section>;
}

function formatError(value: unknown) {
  if (typeof value === "string") return value;
  if (value && typeof value === "object" && "message" in value) return String((value as { message: unknown }).message);
  return String(value);
}
