import { invoke } from "../api/invoke";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
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
  const [indexBusy, setIndexBusy] = useState(false);
  const indexCancelled = useRef(false);
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
    setBundleText("");
    try {
      const bundle = await invoke<PortableWorkspaceBundle>("export_workspace");
      const json = JSON.stringify(bundle, null, 2);
      setBundleText(json);
      setMessage(`Review this bundle snapshot before downloading: ${bundle.sessions.length} sessions, ${bundle.collections.length} collections, ${bundle.environments.length} environments, ${bundle.proxyRules.length} proxy rules, ${bundle.networkProfiles?.length ?? 0} network profiles, and ${bundle.websocketMessages?.length ?? 0} WebSocket messages.`);
      setError(null);
    } catch (value) {
      setError(formatInvokeError(value));
    } finally {
      setBusy(false);
    }
  }

  async function downloadBundle() {
    if (!bundleText) return;
    setBusy(true);
    try {
      const url = URL.createObjectURL(new Blob([bundleText], { type: "application/json" }));
      const link = document.createElement("a");
      link.href = url;
      link.download = `mobile-api-studio-workspace-${new Date().toISOString().slice(0, 10)}.mas.json`;
      link.click();
      window.setTimeout(() => URL.revokeObjectURL(url), 1000);
      await invoke<OnboardingStep>("set_onboarding_step", {
        key: "export_recovery_bundle",
        completed: true,
      });
      await refreshSteps();
      setMessage("Downloaded the reviewed workspace bundle snapshot.");
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

  async function rebuildIndex() {
    setIndexBusy(true); indexCancelled.current = false;
    let processed = 0;
    try {
      for (const phase of ["flows", "websocket"]) {
        let offset = 0;
        while (!indexCancelled.current) {
          const progress = await invoke<{ processed: number; nextOffset: number; done: boolean }>("rebuild_search_index", { phase, offset });
          processed += progress.processed; offset = progress.nextOffset;
          setMessage(`Indexed ${processed} local records.`);
          if (progress.done) break;
        }
      }
      setMessage(`${indexCancelled.current ? "Stopped rebuilding" : "Rebuilt"} local search: ${processed} records processed.`);
      setError(null);
    } catch (value) { setError(formatInvokeError(value)); }
    finally { setIndexBusy(false); }
  }

  async function importBundle() {
    if (!importText.trim()) return;
    if (
      importMode === "replace" &&
      !window.confirm(
        "Replace current workspace data with this bundle? Existing sessions, flows, collections, environments, and proxy rules will be removed first.",
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
        `Imported ${summary.sessions} sessions, ${summary.flows} flows, and ${summary.proxyRules} proxy rules. ${summary.proxyRulesDisabled} imported rules and ${summary.networkProfilesDisabled ?? 0} network profiles are disabled until reviewed. ${summary.secretValuesOmitted} secret values require re-entry.`,
      );
      setError(summary.searchIndexWarning ?? null);
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

      <div className="panel settings-panel">
        <div className="panel-heading"><div><strong>Local search index</strong><span>Redacted headers, trailers and JSON/form bodies. Binary and unstructured text stay in the raw viewer. The index is excluded from exports.</span></div></div>
        <button className="secondary compact" disabled={indexBusy || busy} onClick={() => void rebuildIndex()}>Rebuild local search</button>
        {indexBusy ? <button className="secondary compact" onClick={() => { indexCancelled.current = true; }}>Stop after current batch</button> : null}
      </div>
      <div className="panel settings-panel bundle-panel">
        <div className="panel-heading">
          <div>
            <strong>Workspace export</strong>
            <span>Versioned local bundle with sessions, flow bodies, collections, environments, and proxy rules</span>
          </div>
          <button className="primary compact" onClick={() => void exportBundle()} disabled={busy}>
            Preview .mas.json
          </button>
        </div>
        <div className="privacy-note">
          <strong>Review before download:</strong> environment secret values, Keychain references, sensitive rule actions,
          Map Local files, and rule audit metadata are omitted; sensitive headers and trailers are redacted. Captured HTTP and WebSocket bodies, protocol and public connection details are included
          and may contain application data. Imported proxy rules and network profiles start disabled.
        </div>
        {bundleText ? (
          <>
            <textarea className="bundle-textarea" value={bundleText} readOnly spellCheck={false} aria-label="Workspace export JSON preview" />
            <div className="settings-actions">
              <button className="primary compact" onClick={() => void downloadBundle()} disabled={busy}>Download previewed .mas.json</button>
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
            aria-label="Workspace import file"
            accept="application/json,.json,.mas.json"
            onChange={(event) => void loadImportFile(event.target.files?.[0] ?? null)}
          />
          <select aria-label="Workspace import mode" value={importMode} onChange={(event) => setImportMode(event.target.value as ImportMode)}>
            <option value="merge">Merge</option>
            <option value="replace">Replace workspace</option>
          </select>
          <button className="primary compact" onClick={() => void importBundle()} disabled={busy || !importText.trim()}>
            {busy ? "Working…" : "Import bundle"}
          </button>
        </div>
        <textarea
          className="bundle-textarea import-textarea"
          aria-label="Workspace import JSON"
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
            <span>Proxy rules <strong>{importSummary.proxyRules}</strong></span>
            <span>Rules disabled <strong>{importSummary.proxyRulesDisabled}</strong></span>
            <span>Map Local files omitted <strong>{importSummary.mapLocalFilesOmitted}</strong></span>
            <span>Rule actions omitted <strong>{importSummary.ruleActionsOmitted}</strong></span>
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
