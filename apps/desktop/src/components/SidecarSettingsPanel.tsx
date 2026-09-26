import { invoke } from "../api/invoke";
import { useEffect, useState } from "react";

interface CaptureExecutableSetting {
  configured: string | null;
  effectiveAfterRestart: string;
  usesAutoDiscovery: boolean;
}

export function SidecarSettingsPanel() {
  const [setting, setSetting] = useState<CaptureExecutableSetting | null>(null);
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<CaptureExecutableSetting>("capture_executable_setting")
      .then((next) => {
        setSetting(next);
        setValue(next.configured ?? "");
      })
      .catch((reason) => setError(formatInvokeError(reason)));
  }, []);

  async function save() {
    setBusy(true);
    try {
      const next = await invoke<CaptureExecutableSetting>("set_capture_executable", {
        executable: value.trim() || null,
      });
      setSetting(next);
      setValue(next.configured ?? "");
      setMessage("Capture executable preference saved. Restart Mobile API Studio before the next capture.");
      setError(null);
    } catch (reason) {
      setError(formatInvokeError(reason));
    } finally {
      setBusy(false);
    }
  }

  async function reset() {
    setValue("");
    setBusy(true);
    try {
      const next = await invoke<CaptureExecutableSetting>("set_capture_executable", {
        executable: null,
      });
      setSetting(next);
      setMessage("Automatic PATH discovery restored. Restart Mobile API Studio before the next capture.");
      setError(null);
    } catch (reason) {
      setError(formatInvokeError(reason));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="panel settings-panel" style={{ marginTop: 16 }}>
      <div className="panel-heading">
        <div>
          <strong>Capture sidecar</strong>
          <span>Use automatic discovery or point Mobile API Studio at a managed mitmdump executable</span>
        </div>
        <span className="pill">restart to apply</span>
      </div>
      {error ? <div className="error-banner settings-error">{error}</div> : null}
      {message ? <div className="settings-message">{message}</div> : null}
      <div className="privacy-note">
        <strong>Current startup strategy:</strong>{" "}
        {setting?.usesAutoDiscovery
          ? "automatic `mitmdump` discovery through PATH or standard macOS install locations"
          : setting?.effectiveAfterRestart ?? "custom executable"}.
        A custom path can target a centrally managed or future bundled sidecar without changing capture code.
      </div>
      <div className="settings-actions">
        <input
          className="text-input"
          style={{ flex: "1 1 420px" }}
          value={value}
          onChange={(event) => setValue(event.target.value)}
          placeholder="Optional executable path or command, e.g. /opt/homebrew/bin/mitmdump"
          disabled={busy}
        />
        <button className="primary compact" onClick={() => void save()} disabled={busy}>
          Save executable
        </button>
        <button className="secondary compact" onClick={() => void reset()} disabled={busy}>
          Use automatic discovery
        </button>
      </div>
    </section>
  );
}

function formatInvokeError(value: unknown) {
  if (typeof value === "string") return value;
  if (value && typeof value === "object" && "message" in value) {
    return String((value as { message: unknown }).message);
  }
  return String(value);
}
