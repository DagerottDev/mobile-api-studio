import { useState } from "react";
import { invoke } from "../api/invoke";
import type { FlowSummary } from "../types";
import type { ProxyRule } from "../proxyTypes";

type Member = { userId: string; userName: string; role: "owner" | "editor" | "viewer" };
type Account = Member & { teamId: string; teamName: string };
type Share = { id: string; urlPath?: string; expiresAt: number; sha256: string; revoked?: boolean };
type Workspace = { schemaVersion: number; revision: number; rules: ProxyRule[]; fixtures: { id: string; name: string }[] };
const message = (error: unknown) => error instanceof Error ? error.message : "The operation failed. Review the service connection and permissions.";

export function SharingView() {
  const [endpoint, setEndpoint] = useState("http://127.0.0.1:8190");
  const [accessToken, setAccessToken] = useState("");
  const [account, setAccount] = useState<Account | null>(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  const [flows, setFlows] = useState<FlowSummary[]>([]);
  const [flowIds, setFlowIds] = useState<string[]>([]);
  const [includeBodies, setIncludeBodies] = useState(false);
  const [includeQuery, setIncludeQuery] = useState(false);
  const [includeFixtureBodies, setIncludeFixtureBodies] = useState(false);
  const [artifact, setArtifact] = useState("");
  const [previewDigest, setPreviewDigest] = useState("");
  const [expires, setExpires] = useState(3600);
  const [shares, setShares] = useState<Share[]>([]);
  const [rules, setRules] = useState<ProxyRule[]>([]);
  const [fixtures, setFixtures] = useState<{ id: string; name: string }[]>([]);
  const [ruleIds, setRuleIds] = useState<string[]>([]);
  const [fixtureIds, setFixtureIds] = useState<string[]>([]);
  const [remote, setRemote] = useState<Workspace | null>(null);
  const [pushPreview, setPushPreview] = useState<Workspace | null>(null);
  const [members, setMembers] = useState<Member[]>([]);
  const [memberName, setMemberName] = useState("");
  const [memberRole, setMemberRole] = useState<Member["role"]>("viewer");
  const [newToken, setNewToken] = useState("");
  const canEdit = account?.role === "owner" || account?.role === "editor";

  function serviceUrl() {
    const url = new URL(endpoint);
    if (url.username || url.password || url.search || url.hash || url.pathname !== "/" ||
      !(url.protocol === "https:" || url.protocol === "http:" && ["127.0.0.1", "[::1]"].includes(url.hostname))) {
      throw new Error("Use an HTTPS service origin, or HTTP on literal loopback for development.");
    }
    return url.origin;
  }

  async function request<T>(path: string, method = "GET", body?: unknown): Promise<T> {
    return invoke<T>("sharing_request", { origin: serviceUrl(), accessToken, path, method, body: body ?? null });
  }

  async function run(action: () => Promise<void>) {
    setBusy(true); setNotice("");
    try { await action(); } catch (error) { setNotice(message(error)); } finally { setBusy(false); }
  }
  function signOut() { setAccount(null); setAccessToken(""); setShares([]); setMembers([]); setRemote(null); setPushPreview(null); setArtifact(""); setNewToken(""); }
  function toggle(ids: string[], id: string) { return ids.includes(id) ? ids.filter((value) => value !== id) : [...ids, id]; }

  return <div className="settings-stack">
    <section className="panel settings-panel">
      <h2>Sharing service</h2>
      <p>Connect to your team's service with an issued access token. Sign-in only checks your membership. Capture and sync stay under your control.</p>
      <label>Service origin<input value={endpoint} disabled={!!account || busy} onChange={(event) => setEndpoint(event.target.value)} /></label>
      <label>Access token<input type="password" autoComplete="off" value={accessToken} disabled={!!account || busy} onChange={(event) => setAccessToken(event.target.value)} /></label>
      {account ? <><p>{account.userName} · {account.teamName} · {account.role}</p><button className="secondary" disabled={busy} onClick={signOut}>Sign out</button></> :
        <button disabled={busy || !accessToken.trim()} onClick={() => void run(async () => { setAccount(await request<Account>("/v1/me")); setNotice("Signed in. No traffic was uploaded or capture started."); })}>Sign in</button>}
      <p className="muted">The token stays in memory until sign-out or page reload.</p>
    </section>
    {notice && <div className="settings-message" role="status">{notice}</div>}
    {account && <>
      <section className="panel settings-panel">
        <h2>Share selected requests</h2>
        <p>Known credential headers are redacted. Query values and bodies are omitted by default. Review the exact artifact before uploading; other application data may still be sensitive.</p>
        <button className="secondary" disabled={busy} onClick={() => void run(async () => { setFlows(await invoke<FlowSummary[]>("list_flows")); setArtifact(""); })}>Load local requests</button>
        <div style={{ maxHeight: 240, overflow: "auto" }}>{flows.map((flow) => <label key={flow.id} style={{ display: "block" }}><input type="checkbox" checked={flowIds.includes(flow.id)} disabled={busy} onChange={() => { setFlowIds(toggle(flowIds, flow.id)); setArtifact(""); }} /> {flow.method} {flow.host}{flow.path}</label>)}</div>
        <label><input type="checkbox" checked={includeBodies} disabled={busy} onChange={(event) => { setIncludeBodies(event.target.checked); setArtifact(""); }} /> Include bodies after review</label>
        <label><input type="checkbox" checked={includeQuery} disabled={busy} onChange={(event) => { setIncludeQuery(event.target.checked); setArtifact(""); }} /> Include query values after review</label>
        <button className="secondary" disabled={busy || !flowIds.length} onClick={() => void run(async () => { const result = await invoke<{artifact: string; sha256: string}>("preview_share_har", { flowIds, includeQuery, includeBodies }); setArtifact(result.artifact); setPreviewDigest(result.sha256); })}>Prepare redacted preview</button>
        {artifact && <><textarea className="bundle-textarea" value={artifact} readOnly aria-label="Exact share artifact preview" spellCheck={false} />
          <label>Link lifetime<select value={expires} disabled={busy} onChange={(event) => setExpires(Number(event.target.value))}><option value={3600}>1 hour</option><option value={86400}>1 day</option><option value={604800}>7 days</option></select></label>
          <button disabled={busy || !canEdit} onClick={() => void run(async () => { const result = await request<Share>("/v1/shares", "POST", { artifact, expiresInSeconds: expires, sha256: previewDigest }); if (result.sha256 !== previewDigest) throw new Error("Uploaded artifact digest differs from the reviewed preview."); setShares((items) => [result, ...items]); setNotice("Uploaded the reviewed artifact. Anyone with its link can access it until expiry or revocation."); })}>Upload reviewed artifact</button></>}
        <button className="secondary" disabled={busy} onClick={() => void run(async () => { const result = await request<{shares: Share[]}>("/v1/shares"); setShares(result.shares); })}>Load shared links</button>
        {shares.map((share) => <div key={share.id}>{share.urlPath ? <a href={serviceUrl() + share.urlPath} download rel="noreferrer">Open shared HAR</a> : <span>Share {share.id.slice(0, 12)}{share.revoked ? " · revoked" : " · link available only when created"}</span>} · expires {new Date(share.expiresAt * 1000).toLocaleString()} <button className="secondary compact" disabled={busy || !canEdit || share.revoked} onClick={() => void run(async () => { await request(`/v1/shares/${encodeURIComponent(share.id)}`, "DELETE"); setShares((items) => items.filter((item) => item.id !== share.id)); })}>Revoke</button></div>)}
      </section>
      <section className="panel settings-panel">
        <h2>Team rules and fixtures</h2><p>Sync is manual. Publishing replaces the team's selected rules and fixtures; pulling imports a copy with rules disabled. Scripts, local file maps, credentials, and CA material are excluded.</p>
        <button className="secondary" disabled={busy} onClick={() => void run(async () => { setRemote(await request<Workspace>("/v1/workspace")); setPushPreview(null); })}>Load team workspace</button>
        {remote && <><p>Team revision {remote.revision}</p><textarea className="bundle-textarea" value={JSON.stringify(remote, null, 2)} readOnly aria-label="Team workspace pull preview" />
          <button disabled={busy} onClick={() => void run(async () => { await invoke("import_team_workspace", { artifact: JSON.stringify(remote) }); setNotice("Imported reviewed rules as disabled copies and saved fixtures. Nothing was enabled automatically."); })}>Import reviewed team copy</button></>}
        <button className="secondary" disabled={busy} onClick={() => void run(async () => { setRules(await invoke<ProxyRule[]>("list_proxy_rules")); setFixtures(await invoke<{ id: string; name: string }[]>("list_mock_fixtures")); setPushPreview(null); })}>Load local rules and fixtures</button>
        {rules.filter((rule) => !["script_hook", "map_local"].includes(rule.action.type)).map((rule) => <label key={rule.id} style={{ display: "block" }}><input type="checkbox" checked={ruleIds.includes(rule.id)} disabled={busy} onChange={() => { setRuleIds(toggle(ruleIds, rule.id)); setPushPreview(null); }} /> Rule: {rule.name}</label>)}
        {fixtures.map((fixture) => <label key={fixture.id} style={{ display: "block" }}><input type="checkbox" checked={fixtureIds.includes(fixture.id)} disabled={busy} onChange={() => { setFixtureIds(toggle(fixtureIds, fixture.id)); setPushPreview(null); }} /> Fixture: {fixture.name}</label>)}
        <label><input type="checkbox" checked={includeFixtureBodies} disabled={busy} onChange={(event) => { setIncludeFixtureBodies(event.target.checked); setPushPreview(null); }} /> Include rule and fixture bodies after review</label>
        <button className="secondary" disabled={busy || !canEdit || !remote || (!ruleIds.length && !fixtureIds.length)} onClick={() => void run(async () => { const preview = await invoke<Workspace>("export_team_workspace", { ruleIds, fixtureIds, includeBodies: includeFixtureBodies }); setPushPreview({ ...preview, revision: remote!.revision }); })}>Prepare selected team preview</button>
        {pushPreview && <><textarea className="bundle-textarea" value={JSON.stringify(pushPreview, null, 2)} readOnly aria-label="Team workspace publish preview" /><button disabled={busy || !canEdit} onClick={() => void run(async () => { setRemote(await request<Workspace>("/v1/workspace", "PUT", { schemaVersion: 1, expectedRevision: pushPreview.revision, rules: pushPreview.rules, fixtures: pushPreview.fixtures })); setPushPreview(null); setNotice("Published the reviewed selection to the team."); })}>Publish reviewed selection</button></>}
      </section>
      {account.role === "owner" && <section className="panel settings-panel">
        <h2>Team members</h2><button className="secondary" disabled={busy} onClick={() => void run(async () => { const result = await request<{members: Member[]}>("/v1/members"); setMembers(result.members); })}>Load members</button>
        <label>Name<input value={memberName} disabled={busy} onChange={(event) => setMemberName(event.target.value)} /></label>
        <label>Role<select value={memberRole} disabled={busy} onChange={(event) => setMemberRole(event.target.value as Member["role"])}><option value="viewer">Viewer</option><option value="editor">Editor</option><option value="owner">Owner</option></select></label>
        <button disabled={busy || !memberName.trim()} onClick={() => void run(async () => { const result = await request<{ member: Member; accessToken: string }>("/v1/members", "POST", { name: memberName, role: memberRole }); setNewToken(result.accessToken); setMembers((items) => [...items, result.member]); setMemberName(""); })}>Create member token</button>
        {newToken && <><p>Copy this new member token securely. It disappears on sign-out or reload.</p><textarea readOnly value={newToken} aria-label="New member access token" /><button className="secondary" onClick={() => setNewToken("")}>Hide token</button></>}
        {members.map((member) => <div key={member.userId}>{member.userName} <select aria-label={`Role for ${member.userName}`} value={member.role} disabled={busy} onChange={(event) => void run(async () => { await request(`/v1/members/${encodeURIComponent(member.userId)}`, "PATCH", { role: event.target.value }); { const result = await request<{members: Member[]}>("/v1/members"); setMembers(result.members); }; })}><option value="viewer">Viewer</option><option value="editor">Editor</option><option value="owner">Owner</option></select>
          <button className="secondary compact" disabled={busy} onClick={() => void run(async () => { const result = await request<{ accessToken: string }>(`/v1/members/${encodeURIComponent(member.userId)}/token`, "POST"); setNewToken(result.accessToken); if (member.userId === account.userId) setAccessToken(result.accessToken); })}>Rotate token</button>
          <button className="secondary compact" disabled={busy || member.userId === account.userId} onClick={() => void run(async () => { await request(`/v1/members/${encodeURIComponent(member.userId)}`, "DELETE"); setMembers((items) => items.filter((item) => item.userId !== member.userId)); })}>Remove</button></div>)}
      </section>}
    </>}
  </div>;
}
