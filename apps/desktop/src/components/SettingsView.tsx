import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useMemo, useState } from "react";
import type {
  ConnectionDoctorReport,
  ImportMode,
  ImportSummary,
  OnboardingStep,
  PortableWorkspaceBundle,
} from "../types";

const ONBOARDING = [
  {
    key: "review_connection_doctor",
    title: "Review Connection Doctor",
    detail: "Confirm the capture engine and at least one mobile toolchain are ready.",
  },
  {
    key: "complete_first_capture",
    title: "Complete a first capture",
    detail: "Connect a Simulator or Emulator and confirm traffic appears in Traffic.",
  },
  {
    key: "configure_workspace",
    title: "Configure your workspace",
    detail: "Create a collection or environment so common debugging requests are reusable.",
  },
  {
    key: "export_recovery_bundle",
    title: "Create a recovery bundle",
    detail: "Export the workspace once so you know where portable backups live.",
  },
] as const;

export function SettingsView() {
  const [doctor, setDoctor] = useState<ConnectionDoctorReport | null>(null);
  const [steps, setSteps] = useState<OnboardingStep[]>([]);
  const [doctorBusy, setDoctorBusy] = useState(false);
  const [bundleText, setBundleText] = useState("");
  const [importText, setImportText] = useState("");
  const [importMode, setImportMode] = useState<ImportMode>("merge");
  const [importSummary, setImportSummary] = useState<ImportSummary | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refreshDoctor = useCallback(async () => {
    setDoctorBusy(true);
    try {
      setDoctor(await invoke<ConnectionDoctorReport>("connection_doctor"));
      setError(null);
    } catch (value) {
      setError(formatInvokeError(value));
    } finally {
      setDoctorBusy(false);
    }
  }, []);

  const refreshSteps = useCallback(async () => {
    try {
      setSteps(await invoke<OnboardingStep[]>("list_onboarding_steps"));
    } catch (value) {
      setError(formatInvokeError(value));
    }
  }, []);

  useEffect(() => {
    void Promise.all([refreshDoctor(), refreshSteps()]);
  }, [refreshDoctor, refreshSteps]);

  const completedKeys = useMemo(
    () => new Set(steps.filter((step) => step.completed).map((step) => step.key)),
    [steps],
  );

  async function toggleStep(key: string, completed: boolean) {
    try {
      await invoke<OnboardingStep>("set_onboarding_step", { key, completed });
      await refreshSteps();
      setError(null);
    } catch (value) {
      setError(formatInvokeError(value));
    }
  }

  async function exportBundle() {
    setBusy(true);
    try {
      const result = await invoke<{ bundle: PortableWorkspaceBundle; path: string }>(
        "export_workspace_to_download",
      );
      const bundle = result.bundle;
      const json = JSON.stringify(bundle, null, 2);
      setBundleText(json);
      await invoke<OnboardingStep>("set_onboarding_step", {
        key: "export_recovery_bundle",
        completed: true,
      });
      await refreshSteps();
      setMessage(
        `Exported ${bundle.sessions.length} sessions, ${bundle.collections.length} collections, and ${bundle.environments.length} environments to ${result.path}.`,
      );
      setError(null);
    } catch (value) {
      setError(formatInvokeError(value));
    } finally {
      setBusy(false);
    }
  }

  async function copyBundle() {
    if (!bundleText) return;
    try {
      await navigator.clipboard.writeText(bundleText);
      setMessage("Workspace bundle copied to clipboard.");
    } catch (value) {
      setError(formatInvokeError(value));
    }
  }

  async function importBundle() {
    if (!importText.trim()) return;
    if (
      importMode === "replace" &&
      !window.confirm(
        "Replace current workspace data with this bundle? Existing sessions, flows, collections, and environments will be removed first.",
      )
    ) {
      return;
    }

    setBusy(true);
    try {
      const bundle = JSON.parse(importText) as PortableWorkspaceBundle;
      const summary = await invoke<ImportSummary>("import_workspace", { bundle, mode: importMode });
      setImportSummary(summary);
      setMessage(
        `Imported ${summary.sessions} sessions and ${summary.flows} flows. ${summary.secretValuesOmitted} secret values require re-entry.`,
      );
      setError(null);
    } catch (value) {
      setError(formatInvokeError(value));
    } finally {
      setBusy(false);
    }
  }

  async function loadImportFile(file: File | null) {
    if (!file) return;
    try {
      setImportText(await file.text());
      setImportSummary(null);
      setMessage(`Loaded ${file.name}. Review the import mode before importing.`);
      setError(null);
    } catch (value) {
      setError(formatInvokeError(value));
    }
  }

  return (
    <section className="settings-layout">
      <div className="panel settings-panel">
        <div className="panel-heading">
          <div>
            <strong>Connection Doctor</strong>
            <span>Local prerequisites, runtimes, recovery state, and secure storage</span>
          </div>
          <button className="secondary compact" onClick={() => void refreshDoctor()} disabled={doctorBusy}>
            {doctorBusy ? "Checking…" : "Run Doctor"}
          </button>
        </div>

        {error ? <div className="error-banner settings-error">{error}</div> : null}
        {message ? <div className="settings-message">{message}</div> : null}

        {doctor ? (
          <>
            <div className="doctor-summary">
              <span>Booted iOS <strong>{doctor.bootedIosCount}</strong></span>
              <span>Android emulators <strong>{doctor.androidEmulatorCount}</strong></span>
              <span>Capture engine <strong>{doctor.captureExecutable ?? "missing"}</strong></span>
              <span>Recovery <strong>{doctor.pendingRollback ? "needed" : "clear"}</strong></span>
            </div>
            <div className="doctor-checks">
              {doctor.checks.map((check) => (
                <article className={`doctor-check doctor-${check.status}`} key={check.id}>
                  <div className="doctor-check-heading">
                    <strong>{check.title}</strong>
                    <span>{check.status}</span>
                  </div>
                  <p>{check.detail}</p>
                  {check.action ? <small>{check.action}</small> : null}
                </article>
              ))}
            </div>
          </>
        ) : (
          <p className="empty-state">Run Connection Doctor to inspect local prerequisites.</p>
        )}
      </div>

      <div className="panel settings-panel">
        <div className="panel-heading">
          <div>
            <strong>First-run guide</strong>
            <span>Progress is stored locally and can be revisited anytime</span>
          </div>
          <span className="pill">{completedKeys.size}/{ONBOARDING.length} complete</span>
        </div>
        <div className="onboarding-list">
          {ONBOARDING.map((item, index) => {
            const completed = completedKeys.has(item.key);
            return (
              <label className={completed ? "onboarding-item completed" : "onboarding-item"} key={item.key}>
                <input
                  type="checkbox"
                  checked={completed}
                  onChange={(event) => void toggleStep(item.key, event.target.checked)}
                />
                <span className="onboarding-number">{index + 1}</span>
                <span>
                  <strong>{item.title}</strong>
                  <small>{item.detail}</small>
                </span>
              </label>
            );
          })}
        </div>
      </div>

      <div className="panel settings-panel bundle-panel">
        <div className="panel-heading">
          <div>
            <strong>Workspace export</strong>
            <span>Versioned local bundle with sessions, flow bodies, collections, and environments</span>
          </div>
          <button className="primary compact" onClick={() => void exportBundle()} disabled={busy}>
            Export .mas.json
          </button>
        </div>
        <div className="privacy-note">
          <strong>Safe defaults:</strong> environment secret values and Keychain references are omitted,
          and sensitive headers are redacted. Captured request/response bodies are preserved so sessions
          can be restored; inspect a bundle before sharing because bodies may contain application data.
        </div>
        {bundleText ? (
          <>
            <textarea className="bundle-textarea" value={bundleText} readOnly spellCheck={false} />
            <div className="settings-actions">
              <button className="secondary compact" onClick={() => void copyBundle()}>Copy JSON</button>
            </div>
          </>
        ) : null}
      </div>

      <div className="panel settings-panel bundle-panel">
        <div className="panel-heading">
          <div>
            <strong>Workspace import</strong>
            <span>Merge with existing data or replace the current workspace</span>
          </div>
        </div>
        <div className="import-controls">
          <input
            type="file"
            accept="application/json,.json,.mas.json"
            onChange={(event) => void loadImportFile(event.target.files?.[0] ?? null)}
          />
          <select value={importMode} onChange={(event) => setImportMode(event.target.value as ImportMode)}>
            <option value="merge">Merge</option>
            <option value="replace">Replace workspace</option>
          </select>
          <button className="primary compact" onClick={() => void importBundle()} disabled={busy || !importText.trim()}>
            {busy ? "Working…" : "Import bundle"}
          </button>
        </div>
        <textarea
          className="bundle-textarea import-textarea"
          value={importText}
          onChange={(event) => setImportText(event.target.value)}
          placeholder="Choose a .mas.json file or paste bundle JSON here"
          spellCheck={false}
        />
        {importSummary ? (
          <div className="import-summary">
            <span>Sessions <strong>{importSummary.sessions}</strong></span>
            <span>Flows <strong>{importSummary.flows}</strong></span>
            <span>Collections <strong>{importSummary.collections}</strong></span>
            <span>Saved requests <strong>{importSummary.savedRequests}</strong></span>
            <span>Environments <strong>{importSummary.environments}</strong></span>
            <span>Secrets to re-enter <strong>{importSummary.secretValuesOmitted}</strong></span>
          </div>
        ) : null}
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
