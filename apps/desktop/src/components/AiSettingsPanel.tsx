import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import type { AiSettingsSnapshot } from "../aiTypes";

export function AiSettingsPanel() {
  const [settings, setSettings] = useState<AiSettingsSnapshot | null>(null);
  const [model, setModel] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [secretKeys, setSecretKeys] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function load() {
    try {
      const next = await invoke<AiSettingsSnapshot>("ai_settings");
      setSettings(next);
      setModel(next.model);
      setSecretKeys(next.secretJsonKeys.join("\n"));
      setError(null);
    } catch (value) {
      setError(formatInvokeError(value));
    }
  }

  useEffect(() => { void load(); }, []);

  async function save() {
    setBusy(true);
    try {
      const next = await invoke<AiSettingsSnapshot>("set_ai_settings", {
        input: {
          provider: "openai",
          model: model.trim(),
          apiKey: apiKey.trim() || null,
          secretJsonKeys: secretKeys.split(/[\n,]/).map((item) => item.trim()).filter(Boolean),
        },
      });
      setSettings(next);
      setApiKey("");
      setMessage("AI settings saved");
      setError(null);
    } catch (value) {
      setError(formatInvokeError(value));
    } finally {
      setBusy(false);
    }
  }

  async function clearKey() {
    setBusy(true);
    try {
      const next = await invoke<AiSettingsSnapshot>("clear_ai_api_key");
      setSettings(next);
      setApiKey("");
      setMessage("API key removed from secure storage");
      setError(null);
    } catch (value) {
      setError(formatInvokeError(value));
    } finally {
      setBusy(false);
    }
  }

  return <section className="panel ai-settings-panel">
    <div className="panel-heading">
      <div><strong>AI provider</strong><span>Optional · BYOK · redaction before external requests</span></div>
      <span className={settings?.apiKeyConfigured ? "ai-key-pill configured" : "ai-key-pill"}>{settings?.apiKeyConfigured ? "key configured" : "no key"}</span>
    </div>
    {error ? <div className="error-banner">{error}</div> : null}
    {message ? <div className="success-banner">{message}</div> : null}
    <div className="ai-settings-grid">
      <label className="field-label">Provider<input className="text-input" value="OpenAI" disabled /></label>
      <label className="field-label">Model<input className="text-input" value={model} onChange={(event) => setModel(event.target.value)} placeholder="gpt-5.6-luna" /></label>
      <label className="field-label ai-key-field">OpenAI API key<input className="text-input" type="password" value={apiKey} onChange={(event) => setApiKey(event.target.value)} placeholder={settings?.apiKeyConfigured ? "Stored securely — enter only to replace" : "sk-…"} autoComplete="off" /></label>
      <label className="field-label ai-secret-keys">JSON keys to redact<textarea value={secretKeys} onChange={(event) => setSecretKeys(event.target.value)} placeholder="password\ntoken\nclient_secret" /></label>
    </div>
    <div className="ai-settings-footer">
      <p className="muted-copy">The API key is stored in the OS credential store, not the app database. Context preview is generated locally before any external request. OpenAI requests use the Responses API with response storage disabled.</p>
      <div className="workspace-actions">
        {settings?.apiKeyConfigured ? <button className="secondary danger-action" disabled={busy} onClick={() => void clearKey()}>Remove API key</button> : null}
        <button className="primary" disabled={busy || !model.trim() || settings?.secureStoreAvailable === false} onClick={() => void save()}>{busy ? "Saving…" : "Save AI settings"}</button>
      </div>
      {settings?.secureStoreAvailable === false ? <p className="muted-copy">Secure credential storage is not available on this platform build, so BYOK is disabled.</p> : null}
    </div>
  </section>;
}

function formatInvokeError(value: unknown) {
  if (typeof value === "string") return value;
  if (value && typeof value === "object" && "message" in value) return String((value as { message: unknown }).message);
  return String(value);
}
