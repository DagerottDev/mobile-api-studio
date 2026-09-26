import { invoke } from "../api/invoke";
import { useCallback, useEffect, useMemo, useState } from "react";
import type {
  BreakpointBody,
  BreakpointHeader,
  MockFixture,
  MockHeaderMutation,
  MockRule,
  PendingBreakpoint,
} from "../mockTypes";

type UtilityTab = "fixtures" | "breakpoints";

export function MockUtilitiesView() {
  const [tab, setTab] = useState<UtilityTab>("fixtures");
  return (
    <section className="mock-utilities panel">
      <div className="mock-utility-tabs">
        <button className={tab === "fixtures" ? "workspace-tab active" : "workspace-tab"} onClick={() => setTab("fixtures")}>Fixtures</button>
        <button className={tab === "breakpoints" ? "workspace-tab active" : "workspace-tab"} onClick={() => setTab("breakpoints")}>Breakpoints</button>
      </div>
      {tab === "fixtures" ? <FixturesPanel /> : <BreakpointsPanel />}
    </section>
  );
}

function FixturesPanel() {
  const [fixtures, setFixtures] = useState<MockFixture[]>([]);
  const [rules, setRules] = useState<MockRule[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [selectedRuleId, setSelectedRuleId] = useState("");
  const [draft, setDraft] = useState<MockFixture | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [nextFixtures, nextRules] = await Promise.all([
        invoke<MockFixture[]>("list_mock_fixtures"),
        invoke<MockRule[]>("list_mock_rules"),
      ]);
      setFixtures(nextFixtures);
      setRules(nextRules);
      setSelectedId((current) => current && nextFixtures.some((fixture) => fixture.id === current) ? current : nextFixtures[0]?.id ?? null);
      setSelectedRuleId((current) => current && nextRules.some((rule) => rule.id === current) ? current : nextRules[0]?.id ?? "");
      setError(null);
    } catch (value) {
      setError(formatInvokeError(value));
    }
  }, []);

  useEffect(() => { void refresh(); }, [refresh]);

  const selected = useMemo(() => fixtures.find((fixture) => fixture.id === selectedId) ?? null, [fixtures, selectedId]);
  useEffect(() => { setDraft(selected ? structuredClone(selected) : null); }, [selected]);

  function createFixture() {
    const timestamp = Date.now().toString();
    const fixture: MockFixture = {
      schemaVersion: 1,
      id: `fixture-${Date.now()}-${Math.random().toString(16).slice(2)}`,
      name: "New response fixture",
      statusCode: 200,
      responseHeaders: [],
      responseBody: { contentType: "application/json", encoding: "text", data: "{}" },
      sourceFlowId: null,
      createdAt: timestamp,
      updatedAt: timestamp,
    };
    setFixtures((current) => [...current, fixture]);
    setSelectedId(fixture.id);
    setDraft(fixture);
  }

  async function save() {
    if (!draft) return;
    setBusy(true);
    try {
      const saved = await invoke<MockFixture>("upsert_mock_fixture", {
        fixture: { ...draft, updatedAt: Date.now().toString() },
      });
      setMessage(`Saved fixture ${saved.name}.`);
      await refresh();
      setSelectedId(saved.id);
    } catch (value) {
      setError(formatInvokeError(value));
    } finally {
      setBusy(false);
    }
  }

  async function remove() {
    if (!draft || !window.confirm(`Delete fixture “${draft.name}”?`)) return;
    setBusy(true);
    try {
      await invoke("delete_mock_fixture", { id: draft.id });
      setMessage("Fixture deleted.");
      await refresh();
    } catch (value) {
      setError(formatInvokeError(value));
    } finally {
      setBusy(false);
    }
  }

  async function applyToRule() {
    if (!draft || !selectedRuleId) return;
    setBusy(true);
    try {
      const rule = await invoke<MockRule>("apply_fixture_to_mock", {
        fixtureId: draft.id,
        ruleId: selectedRuleId,
      });
      setMessage(`Applied ${draft.name} to ${rule.name}. Active capture reloads it automatically.`);
      setRules(await invoke<MockRule[]>("list_mock_rules"));
    } catch (value) {
      setError(formatInvokeError(value));
    } finally {
      setBusy(false);
    }
  }

  function updateFixtureHeader(index: number, patch: Partial<MockHeaderMutation>) {
    if (!draft) return;
    setDraft({
      ...draft,
      responseHeaders: draft.responseHeaders.map((header, current) => current === index ? { ...header, ...patch } : header),
    });
  }

  return (
    <div className="mock-utility-grid">
      <aside className="mock-utility-list">
        <div className="panel-heading"><div><strong>Response fixtures</strong><span>{fixtures.length} reusable responses</span></div><button className="primary compact" onClick={createFixture}>New fixture</button></div>
        {fixtures.map((fixture) => <button key={fixture.id} className={selectedId === fixture.id ? "mock-utility-row selected" : "mock-utility-row"} onClick={() => setSelectedId(fixture.id)}><strong>{fixture.name}</strong><small>{fixture.statusCode} · {fixture.responseBody?.contentType ?? "no body"}</small></button>)}
        {fixtures.length === 0 ? <p className="empty-state">Create a fixture here or save one from Traffic.</p> : null}
      </aside>
      <div className="mock-utility-editor">
        {error ? <div className="error-banner">{error}</div> : null}
        {message ? <div className="settings-message">{message}</div> : null}
        {draft ? <>
          <div className="mock-editor-heading"><div><span className="eyebrow">Reusable fixture</span><h2>{draft.name}</h2></div><div className="mock-heading-actions"><button className="primary compact" onClick={() => void save()} disabled={busy}>Save</button><button className="secondary compact danger-action" onClick={() => void remove()} disabled={busy}>Delete</button></div></div>
          <div className="mock-grid two-column">
            <label className="field-label">Name<input className="text-input" value={draft.name} onChange={(event) => setDraft({ ...draft, name: event.target.value })} /></label>
            <label className="field-label">Status<input className="text-input" type="number" min="100" max="599" value={draft.statusCode} onChange={(event) => setDraft({ ...draft, statusCode: Number(event.target.value) })} /></label>
          </div>
          <label className="field-label">Content type<input className="text-input" value={draft.responseBody?.contentType ?? ""} onChange={(event) => setDraft({ ...draft, responseBody: { contentType: event.target.value || null, encoding: draft.responseBody?.encoding ?? "text", data: draft.responseBody?.data ?? "" } })} /></label>
          <label className="field-label">Response body<textarea className="mock-body-editor" value={draft.responseBody?.data ?? ""} onChange={(event) => setDraft({ ...draft, responseBody: { contentType: draft.responseBody?.contentType ?? "application/json", encoding: draft.responseBody?.encoding ?? "text", data: event.target.value } })} spellCheck={false} /></label>
          <div className="mock-section-heading"><div><h3>Fixture headers</h3><span>These are applied as response header mutations when the fixture is assigned.</span></div><button className="secondary compact" onClick={() => setDraft({ ...draft, responseHeaders: [...draft.responseHeaders, { name: "", value: "", remove: false }] })}>Add header</button></div>
          <div className="mock-mutation-list">
            {draft.responseHeaders.map((header, index) => <div className="mock-header-row" key={index}><input className="text-input" value={header.name} onChange={(event) => updateFixtureHeader(index, { name: event.target.value })} placeholder="Header" /><input className="text-input" value={header.value ?? ""} disabled={header.remove} onChange={(event) => updateFixtureHeader(index, { value: event.target.value })} placeholder="Value" /><label className="inline-toggle"><input type="checkbox" checked={header.remove} onChange={(event) => updateFixtureHeader(index, { remove: event.target.checked })} /> Remove</label><button className="icon-button" onClick={() => setDraft({ ...draft, responseHeaders: draft.responseHeaders.filter((_, current) => current !== index) })}>×</button></div>)}
          </div>
          <div className="fixture-apply-row"><select value={selectedRuleId} onChange={(event) => setSelectedRuleId(event.target.value)}><option value="">Choose mock rule</option>{rules.map((rule) => <option key={rule.id} value={rule.id}>{rule.name}</option>)}</select><button className="secondary" onClick={() => void applyToRule()} disabled={busy || !selectedRuleId}>Apply fixture to rule</button></div>
          {draft.sourceFlowId ? <p className="muted-copy">Created from captured flow {draft.sourceFlowId}.</p> : null}
        </> : <p className="empty-state">Select a response fixture.</p>}
      </div>
    </div>
  );
}

function BreakpointsPanel() {
  const [pending, setPending] = useState<PendingBreakpoint[]>([]);
  const [rules, setRules] = useState<MockRule[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [draft, setDraft] = useState<PendingBreakpoint | null>(null);
  const [bodyText, setBodyText] = useState("");
  const [bodyEdited, setBodyEdited] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [nextPending, nextRules] = await Promise.all([
        invoke<PendingBreakpoint[]>("list_pending_breakpoints"),
        invoke<MockRule[]>("list_mock_rules"),
      ]);
      setPending(nextPending);
      setRules(nextRules);
      setSelectedId((current) => current && nextPending.some((item) => item.id === current) ? current : nextPending[0]?.id ?? null);
      setError(null);
    } catch (value) {
      setError(formatInvokeError(value));
    }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), 250);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const selected = useMemo(() => pending.find((item) => item.id === selectedId) ?? null, [pending, selectedId]);
  useEffect(() => {
    setDraft(selected ? structuredClone(selected) : null);
    setBodyText(selected?.body ? decodeBodyForEditor(selected.body) : "");
    setBodyEdited(false);
  }, [selected?.id]);

  async function toggleRuleBreakpoint(rule: MockRule, field: "requestBreakpoint" | "responseBreakpoint", enabled: boolean) {
    try {
      await invoke<MockRule>("upsert_mock_rule", {
        rule: { ...rule, [field]: enabled, updatedAt: Date.now().toString() },
      });
      await refresh();
    } catch (value) {
      setError(formatInvokeError(value));
    }
  }

  function updateHeader(index: number, patch: Partial<BreakpointHeader>) {
    if (!draft) return;
    setDraft({ ...draft, headers: draft.headers.map((header, current) => current === index ? { ...header, ...patch } : header) });
  }

  async function resolve(action: "continue" | "cancel") {
    if (!draft) return;
    setBusy(true);
    try {
      const body = action === "continue" && bodyEdited && draft.body
        ? encodeBodyFromEditor(draft.body, bodyText)
        : null;
      await invoke("resolve_breakpoint", {
        input: {
          id: draft.id,
          action,
          method: draft.stage === "request" ? draft.method : null,
          url: draft.stage === "request" ? draft.url : null,
          headers: action === "continue" ? draft.headers : null,
          body,
          clearBody: action === "continue" && bodyEdited && draft.body === null,
          statusCode: draft.stage === "response" ? draft.statusCode : null,
        },
      });
      setMessage(action === "continue" ? "Breakpoint continued with the edited payload." : "Breakpoint cancelled and the flow was terminated.");
      setSelectedId(null);
      await refresh();
    } catch (value) {
      setError(formatInvokeError(value));
    } finally {
      setBusy(false);
    }
  }

  async function clearStale() {
    try {
      const count = await invoke<number>("clear_stale_breakpoints");
      setMessage(`Cleared ${count} expired breakpoint file${count === 1 ? "" : "s"}.`);
      await refresh();
    } catch (value) {
      setError(formatInvokeError(value));
    }
  }

  return (
    <div className="breakpoint-workspace">
      <section className="breakpoint-rule-settings">
        <div className="panel-heading"><div><strong>Breakpoint rules</strong><span>Pause matching flows for up to 60 seconds</span></div><button className="secondary compact" onClick={() => void clearStale()}>Clear expired</button></div>
        <div className="breakpoint-rule-list">
          {rules.map((rule) => <div className="breakpoint-rule-row" key={rule.id}><div><strong>{rule.name}</strong><small>{rule.method ?? "ANY"} {rule.host ?? "*"}{rule.pathPattern}</small></div><label className="inline-toggle"><input type="checkbox" checked={rule.requestBreakpoint} onChange={(event) => void toggleRuleBreakpoint(rule, "requestBreakpoint", event.target.checked)} /> Request</label><label className="inline-toggle"><input type="checkbox" checked={rule.responseBreakpoint} onChange={(event) => void toggleRuleBreakpoint(rule, "responseBreakpoint", event.target.checked)} /> Response</label></div>)}
          {rules.length === 0 ? <p className="empty-state">Create a mock rule before enabling breakpoints.</p> : null}
        </div>
      </section>

      <div className="mock-utility-grid breakpoint-grid">
        <aside className="mock-utility-list">
          <div className="panel-heading"><div><strong>Paused flows</strong><span>{pending.length} waiting</span></div></div>
          {pending.map((item) => <button key={item.id} className={selectedId === item.id ? "mock-utility-row selected" : "mock-utility-row"} onClick={() => setSelectedId(item.id)}><strong>{item.stage.toUpperCase()} · {item.ruleName}</strong><small>{item.method} {item.url}</small></button>)}
          {pending.length === 0 ? <p className="empty-state">No paused flows. Matching traffic appears here while a breakpoint is enabled.</p> : null}
        </aside>
        <div className="mock-utility-editor">
          {error ? <div className="error-banner">{error}</div> : null}
          {message ? <div className="settings-message">{message}</div> : null}
          {draft ? <>
            <div className="mock-editor-heading"><div><span className="eyebrow">{draft.stage} breakpoint</span><h2>{draft.ruleName}</h2><small>{secondsRemaining(draft.deadlineAt)}s until auto-continue</small></div><div className="mock-heading-actions"><button className="primary compact" onClick={() => void resolve("continue")} disabled={busy}>Continue</button><button className="secondary compact danger-action" onClick={() => void resolve("cancel")} disabled={busy}>Cancel flow</button></div></div>
            <div className="mock-grid two-column">
              <label className="field-label">Method<input className="text-input" value={draft.method} disabled={draft.stage !== "request"} onChange={(event) => setDraft({ ...draft, method: event.target.value })} /></label>
              {draft.stage === "response" ? <label className="field-label">Status<input className="text-input" type="number" min="100" max="599" value={draft.statusCode ?? 200} onChange={(event) => setDraft({ ...draft, statusCode: Number(event.target.value) })} /></label> : <span />}
            </div>
            <label className="field-label">URL<input className="text-input" value={draft.url} disabled={draft.stage !== "request"} onChange={(event) => setDraft({ ...draft, url: event.target.value })} /></label>
            <div className="mock-section-heading"><div><h3>Headers</h3><span>Edited rows replace the paused side’s headers.</span></div><button className="secondary compact" onClick={() => setDraft({ ...draft, headers: [...draft.headers, { name: "", value: "" }] })}>Add header</button></div>
            <div className="breakpoint-header-list">{draft.headers.map((header, index) => <div className="breakpoint-header-row" key={index}><input className="text-input" value={header.name} onChange={(event) => updateHeader(index, { name: event.target.value })} /><input className="text-input" value={header.value} onChange={(event) => updateHeader(index, { value: event.target.value })} /><button className="icon-button" onClick={() => setDraft({ ...draft, headers: draft.headers.filter((_, current) => current !== index) })}>×</button></div>)}</div>
            <div className="mock-section-heading"><div><h3>Body</h3><span>{draft.body?.contentType ?? "No body"}{draft.body?.isTruncated ? " · preview truncated; original is preserved until edited" : bodyEdited ? " · edited" : " · original preserved"}</span></div>{draft.body ? <button className="secondary compact" onClick={() => { setDraft({ ...draft, body: null }); setBodyText(""); setBodyEdited(true); }}>Clear body</button> : <button className="secondary compact" onClick={() => { const body = emptyBody(); setDraft({ ...draft, body }); setBodyText(""); setBodyEdited(true); }}>Add body</button>}</div>
            {draft.body ? <textarea className="mock-body-editor" value={bodyText} onChange={(event) => { setBodyText(event.target.value); setBodyEdited(true); }} spellCheck={false} /> : <p className="muted-copy">{bodyEdited ? "Continuing will send an empty body." : "This paused side has no body."}</p>}
          </> : <p className="empty-state">Select a paused request or response.</p>}
        </div>
      </div>
    </div>
  );
}

function emptyBody(): BreakpointBody {
  return { dataBase64: "", contentType: "application/json", isBinary: false, isTruncated: false };
}

function decodeBodyForEditor(body: BreakpointBody) {
  try {
    const binary = atob(body.dataBase64);
    if (body.isBinary) return body.dataBase64;
    const bytes = Uint8Array.from(binary, (character) => character.charCodeAt(0));
    return new TextDecoder().decode(bytes);
  } catch {
    return body.dataBase64;
  }
}

function encodeBodyFromEditor(source: BreakpointBody, value: string): BreakpointBody {
  if (source.isBinary) return { ...source, dataBase64: value, isTruncated: false };
  const bytes = new TextEncoder().encode(value);
  let binary = "";
  bytes.forEach((byte) => { binary += String.fromCharCode(byte); });
  return { ...source, dataBase64: btoa(binary), isTruncated: false };
}

function secondsRemaining(deadlineAt: string) {
  const remaining = Number(deadlineAt) - Date.now();
  return Math.max(0, Math.ceil(remaining / 1000));
}

function formatInvokeError(value: unknown) {
  if (typeof value === "string") return value;
  if (value && typeof value === "object" && "message" in value) return String((value as { message: unknown }).message);
  return String(value);
}
