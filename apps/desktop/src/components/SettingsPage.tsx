import { useEffect, useMemo, useRef, useState } from "react";
import { APP_VERSION } from "../appVersion";
import { desktopCommands, messageFromError } from "../commands";
import { browserMessages } from "../i18n";
import { installProgressMessage, progressPercent } from "../modelUi";
import type {
  ApplicationSettings,
  InstallProgress,
  ManagedVllmSupport,
  OllamaConnectionReport,
  RemoteTargetProfile,
  RunningModelEntry,
  SharingStatus,
  SettingsValidationError,
} from "../types";

const text = browserMessages();

interface SettingsPageProps {
  settings: ApplicationSettings;
  onSaved: (settings: ApplicationSettings) => void;
  onDirtyChange: (dirty: boolean) => void;
  onError: (error: string) => void;
}

function cloneSettings(settings: ApplicationSettings): ApplicationSettings {
  return structuredClone(settings);
}

function valueFromNumber(value: string): number {
  return Number.parseInt(value, 10) || 0;
}

async function writeClipboardText(value: string): Promise<void> {
  // Keep the fallback inside the original click gesture. WebView can reject
  // navigator.clipboard asynchronously, after the gesture permission expires.
  const textarea = document.createElement("textarea");
  textarea.value = value;
  textarea.setAttribute("readonly", "");
  textarea.style.position = "fixed";
  textarea.style.opacity = "0";
  document.body.appendChild(textarea);
  textarea.select();
  const copied = document.execCommand("copy");
  textarea.remove();
  if (copied) return;
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(value);
    return;
  }
  throw new Error("The clipboard is unavailable. Select and copy the token manually.");
}

export function SettingsPage({
  settings,
  onSaved,
  onDirtyChange,
  onError,
}: SettingsPageProps) {
  const [draft, setDraft] = useState(() => cloneSettings(settings));
  const [targets, setTargets] = useState<RemoteTargetProfile[]>([]);
  const [validation, setValidation] = useState<SettingsValidationError[]>([]);
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);
  const [restarting, setRestarting] = useState(false);
  const [repairingOllama, setRepairingOllama] = useState(false);
  const [ollamaRepairProgress, setOllamaRepairProgress] = useState<InstallProgress>();
  const [restartRequired, setRestartRequired] = useState(false);
  const [connection, setConnection] = useState<OllamaConnectionReport>();
  const [managedVllmSupport, setManagedVllmSupport] = useState<ManagedVllmSupport>();
  const [huggingFaceToken, setHuggingFaceToken] = useState("");
  const [huggingFaceTokenSaved, setHuggingFaceTokenSaved] = useState(false);
  const [vllmBusy, setVllmBusy] = useState(false);
  const [models, setModels] = useState<RunningModelEntry[]>([]);
  const [sharingStatus, setSharingStatus] = useState<SharingStatus>();
  const [sharingBusy, setSharingBusy] = useState(false);
  const [generatedToken, setGeneratedToken] = useState("");
  const [tokenCopyStatus, setTokenCopyStatus] = useState<"idle" | "copied" | "failed">("idle");
  const [sharingEnabled, setSharingEnabled] = useState(settings.sharing.enabled);
  const [allowOtherDevices, setAllowOtherDevices] = useState(settings.sharing.allowOtherDevices);
  const [sharingPort, setSharingPort] = useState(settings.sharing.port);
  const [exposedModelIds, setExposedModelIds] = useState(settings.sharing.exposedModelIds);
  const restoreInput = useRef<HTMLInputElement>(null);
  const [supportBusy, setSupportBusy] = useState(false);
  const [supportNotice, setSupportNotice] = useState("");
  const dirty = useMemo(
    () => JSON.stringify(draft) !== JSON.stringify(settings),
    [draft, settings],
  );
  const errorsByField = useMemo(
    () => new Map(validation.map((error) => [error.field, error.message])),
    [validation],
  );

  useEffect(() => {
    setDraft(cloneSettings(settings));
    setSharingEnabled(settings.sharing.enabled);
    setAllowOtherDevices(settings.sharing.allowOtherDevices);
    setSharingPort(settings.sharing.port);
    setExposedModelIds(settings.sharing.exposedModelIds);
  }, [settings]);

  useEffect(() => {
    onDirtyChange(dirty);
    const warn = (event: BeforeUnloadEvent) => {
      if (!dirty) return;
      event.preventDefault();
    };
    window.addEventListener("beforeunload", warn);
    return () => {
      window.removeEventListener("beforeunload", warn);
      onDirtyChange(false);
    };
  }, [dirty, onDirtyChange]);

  useEffect(() => {
    void desktopCommands.loadRemoteTargets()
      .then(setTargets)
      .catch(() => setTargets([]));
    void desktopCommands.managedVllmSupport()
      .then(setManagedVllmSupport)
      .catch(() => setManagedVllmSupport(undefined));
    void desktopCommands.runtimeSecretStatus("huggingFaceToken")
      .then(setHuggingFaceTokenSaved)
      .catch(() => setHuggingFaceTokenSaved(false));
    void desktopCommands.loadModels().then(setModels).catch(() => setModels([]));
    void desktopCommands.sharingStatus().then(setSharingStatus).catch(() => setSharingStatus(undefined));
  }, []);

  const shareableModels = useMemo(
    () => models.filter((model) =>
      model.managed
      && model.targetId === "local"
      && model.inventoryStatus === "available"
      && (model.runtimeId === "ollama" || model.runtimeId === "vllm")
      && model.runtimeCapabilities.lifecycle !== "external"),
    [models],
  );

  const generateSharingToken = async () => {
    setSharingBusy(true);
    try {
      const token = await desktopCommands.generateSharingToken();
      setGeneratedToken(token);
      setTokenCopyStatus("idle");
      setSharingStatus(await desktopCommands.sharingStatus());
    } catch (error) {
      onError(messageFromError(error, text.errors.unexpected));
    } finally {
      setSharingBusy(false);
    }
  };

  const copyGeneratedToken = async () => {
    try {
      await writeClipboardText(generatedToken);
      setTokenCopyStatus("copied");
    } catch (error) {
      setTokenCopyStatus("failed");
      onError(messageFromError(error, "Could not copy the API token."));
    }
  };

  const applySharing = async () => {
    const confirmed = !sharingEnabled
      || !allowOtherDevices
      || window.confirm(
        "Other devices will reach an authenticated HTTP endpoint. Traffic is not encrypted; use only a trusted network or a TLS reverse proxy. Lumen Source does not change your firewall or router.",
      );
    if (!confirmed) return;
    setSharingBusy(true);
    try {
      const status = await desktopCommands.configureSharing(
        sharingEnabled,
        allowOtherDevices,
        sharingPort,
        exposedModelIds,
        confirmed,
      );
      setSharingStatus(status);
      const saved = await desktopCommands.loadSettings();
      setDraft(cloneSettings(saved));
      onSaved(saved);
    } catch (error) {
      onError(messageFromError(error, text.errors.unexpected));
    } finally {
      setSharingBusy(false);
    }
  };

  const revokeSharingToken = async () => {
    if (!window.confirm("Revoke the sharing token and immediately restore loopback-only access?")) return;
    setSharingBusy(true);
    try {
      await desktopCommands.revokeSharingToken();
      setGeneratedToken("");
      setSharingEnabled(false);
      setSharingStatus(await desktopCommands.sharingStatus());
      const saved = await desktopCommands.loadSettings();
      setDraft(cloneSettings(saved));
      onSaved(saved);
    } catch (error) {
      onError(messageFromError(error, text.errors.unexpected));
    } finally {
      setSharingBusy(false);
    }
  };

  const exportDiagnostics = async () => {
    setSupportBusy(true);
    setSupportNotice("Preparing redacted diagnostics…");
    try {
      const path = await desktopCommands.exportDiagnosticBundleFile();
      setSupportNotice(`Redacted diagnostics saved to ${path}`);
    } catch (error) {
      onError(messageFromError(error, text.errors.unexpected));
    } finally {
      setSupportBusy(false);
    }
  };

  const exportStateBackup = async () => {
    setSupportBusy(true);
    setSupportNotice("Preparing state backup…");
    try {
      const path = await desktopCommands.exportStateBackupFile();
      setSupportNotice(`State backup saved to ${path}`);
    } catch (error) {
      onError(messageFromError(error, text.errors.unexpected));
    } finally {
      setSupportBusy(false);
    }
  };

  const restoreStateBackup = async (file?: File) => {
    if (!file || !window.confirm(
      "Replace current settings and inventory with this backup? Credentials and installed model weights are not changed.",
    )) return;
    setSupportBusy(true);
    try {
      const restored = await desktopCommands.restoreStateBackup(await file.text(), true);
      setDraft(cloneSettings(restored));
      onSaved(restored);
      setModels(await desktopCommands.loadModels());
      setSharingStatus(await desktopCommands.sharingStatus());
      setSupportNotice("State backup restored.");
    } catch (error) {
      onError(messageFromError(error, text.errors.unexpected));
    } finally {
      if (restoreInput.current) restoreInput.current.value = "";
      setSupportBusy(false);
    }
  };

  const safeReset = async (clearInventory: boolean) => {
    const scope = clearInventory
      ? "Reset settings and forget the Lumen Source inventory? Model weights, shared caches, and credentials remain on disk."
      : "Reset settings and operation records? Installed models, inventory, weights, caches, and credentials are preserved.";
    if (!window.confirm(scope)) return;
    setSupportBusy(true);
    try {
      const resetSettings = await desktopCommands.safeReset(clearInventory, true);
      setDraft(cloneSettings(resetSettings));
      onSaved(resetSettings);
      setModels(await desktopCommands.loadModels());
      setSharingStatus(await desktopCommands.sharingStatus());
    } catch (error) {
      onError(messageFromError(error, text.errors.unexpected));
    } finally {
      setSupportBusy(false);
    }
  };

  const saveHuggingFaceToken = async () => {
    if (!huggingFaceToken.trim()) return;
    setVllmBusy(true);
    try {
      await desktopCommands.saveRuntimeSecret("huggingFaceToken", huggingFaceToken);
      setHuggingFaceToken("");
      setHuggingFaceTokenSaved(true);
    } catch (error) {
      onError(messageFromError(error, text.errors.unexpected));
    } finally {
      setVllmBusy(false);
    }
  };

  const forgetHuggingFaceToken = async () => {
    setVllmBusy(true);
    try {
      await desktopCommands.deleteRuntimeSecret("huggingFaceToken");
      setHuggingFaceToken("");
      setHuggingFaceTokenSaved(false);
    } catch (error) {
      onError(messageFromError(error, text.errors.unexpected));
    } finally {
      setVllmBusy(false);
    }
  };

  const deleteVllmCaches = async () => {
    if (!window.confirm(text.settings.vllm.deleteCachesConfirmation)) return;
    setVllmBusy(true);
    try {
      await desktopCommands.deleteManagedVllmCaches(true);
    } catch (error) {
      onError(messageFromError(error, text.errors.unexpected));
    } finally {
      setVllmBusy(false);
    }
  };

  const update = (recipe: (next: ApplicationSettings) => void) => {
    setDraft((current) => {
      const next = cloneSettings(current);
      recipe(next);
      return next;
    });
    setValidation([]);
    setConnection(undefined);
  };

  const save = async () => {
    setSaving(true);
    try {
      const errors = await desktopCommands.validateSettings(draft);
      setValidation(errors);
      if (errors.length > 0) return;
      const report = await desktopCommands.saveSettings(draft, false);
      setRestartRequired(report.runtimeRestartRequired);
      setDraft(cloneSettings(report.settings));
      onSaved(report.settings);
    } catch (error) {
      onError(messageFromError(error, text.errors.unexpected));
    } finally {
      setSaving(false);
    }
  };

  const reset = async () => {
    if (!window.confirm(text.settings.resetConfirmation)) return;
    setSaving(true);
    try {
      const report = await desktopCommands.resetSettings();
      setValidation([]);
      setConnection(undefined);
      setRestartRequired(report.runtimeRestartRequired);
      setDraft(cloneSettings(report.settings));
      onSaved(report.settings);
    } catch (error) {
      onError(messageFromError(error, text.errors.unexpected));
    } finally {
      setSaving(false);
    }
  };

  const testConnection = async () => {
    setTesting(true);
    try {
      setConnection(await desktopCommands.testOllamaConnection(draft));
    } catch (error) {
      onError(messageFromError(error, text.errors.unexpected));
    } finally {
      setTesting(false);
    }
  };

  const restart = async () => {
    setRestarting(true);
    try {
      const report = await desktopCommands.restartManagedOllama();
      setConnection(report);
      setRestartRequired(false);
    } catch (error) {
      onError(messageFromError(error, text.errors.unexpected));
    } finally {
      setRestarting(false);
    }
  };

  const repairOllama = async () => {
    if (!window.confirm(text.settings.ollama.repairConfirmation)) return;
    setRepairingOllama(true);
    setOllamaRepairProgress(undefined);
    let unlisten: (() => void) | undefined;
    try {
      unlisten = await desktopCommands.onInstallProgress((progress) => {
        if (progress.modelId === "ollama-runtime-repair") setOllamaRepairProgress(progress);
      });
      const report = await desktopCommands.repairManagedOllama();
      setConnection(report);
    } catch (error) {
      setOllamaRepairProgress((current) => current && ({
        ...current,
        phase: "failed",
        messageKey: "runtimeRepairFailed",
      }));
      onError(messageFromError(error, text.errors.unexpected));
    } finally {
      unlisten?.();
      setRepairingOllama(false);
    }
  };

  const fieldError = (field: string) => {
    const error = errorsByField.get(field);
    return error ? <p className="field-error" role="alert">{error}</p> : null;
  };

  return (
    <div className="settings-page">
      <header className="settings-heading">
        <div>
          <span className="eyebrow">{text.settings.eyebrow}</span>
          <h1>{text.settings.title}</h1>
          <p>{text.settings.description}</p>
        </div>
        <div className="settings-actions">
          <button type="button" className="secondary-button" onClick={() => void reset()} disabled={saving}>
            {text.settings.reset}
          </button>
          <button type="button" className="primary-button" onClick={() => void save()} disabled={saving || !dirty}>
            {saving ? text.common.saving : text.common.save}
          </button>
        </div>
      </header>

      {dirty && <div className="settings-notice" role="status">{text.settings.unsaved}</div>}
      {validation.length > 0 && (
        <div className="settings-validation-summary" role="alert">
          <strong>{text.settings.validationTitle}</strong>
          <ul>{validation.map((error) => <li key={`${error.field}:${error.message}`}>{error.message}</li>)}</ul>
        </div>
      )}
      {restartRequired && (
        <div className="settings-notice restart-notice" role="status">
          <div>
            <strong>{text.settings.restartTitle}</strong>
            <p>{text.settings.restartDescription}</p>
          </div>
          <button type="button" onClick={() => void restart()} disabled={restarting}>
            {restarting ? text.settings.restarting : text.settings.restart}
          </button>
        </div>
      )}

      <section className="settings-section" aria-labelledby="settings-general">
        <div className="settings-section-heading">
          <h2 id="settings-general">{text.settings.general.title}</h2>
          <p>{text.settings.general.description}</p>
        </div>
        <div className="settings-grid">
          <label>
            <span>{text.settings.general.defaultRuntime}</span>
            <select
              value={draft.defaultRuntime}
              onChange={(event) => update((next) => { next.defaultRuntime = event.target.value as ApplicationSettings["defaultRuntime"]; })}
            >
              <option value="ollama">Ollama</option>
              <option value="vllm" disabled>vLLM ({text.settings.externalConnections})</option>
            </select>
            <small>{text.settings.general.runtimeHint}</small>
          </label>
          <label>
            <span>{text.settings.general.defaultTarget}</span>
            <select
              value={draft.defaultTargetId}
              onChange={(event) => update((next) => { next.defaultTargetId = event.target.value; })}
            >
              <option value="local">{text.settings.general.localMachine}</option>
              {targets.map((target) => (
                <option key={target.targetId} value={target.targetId}>{target.targetName}</option>
              ))}
            </select>
            {fieldError("defaultTargetId")}
          </label>
          <label>
            <span>{text.settings.general.defaultUseCase}</span>
            <select
              value={draft.defaultUseIntent}
              onChange={(event) => update((next) => { next.defaultUseIntent = event.target.value as ApplicationSettings["defaultUseIntent"]; })}
            >
              <option value="chat">{text.wizard.intent.chatTitle}</option>
              <option value="code">{text.wizard.intent.codeTitle}</option>
              <option value="creative">{text.wizard.intent.creativeTitle}</option>
              <option value="research">{text.wizard.intent.researchTitle}</option>
            </select>
            <small>{text.settings.general.installDefaultsHint}</small>
          </label>
          <label>
            <span>{text.settings.general.defaultPerformance}</span>
            <select
              value={draft.defaultPerformanceProfile}
              onChange={(event) => update((next) => { next.defaultPerformanceProfile = event.target.value as ApplicationSettings["defaultPerformanceProfile"]; })}
            >
              <option value="safe">{text.wizard.profile.safeTitle}</option>
              <option value="balanced">{text.wizard.profile.balancedTitle}</option>
              <option value="fast">{text.wizard.profile.fastTitle}</option>
              <option value="custom">{text.wizard.profile.customTitle}</option>
            </select>
            <small>{text.settings.general.installDefaultsHint}</small>
          </label>
          <label className="check-setting">
            <input
              type="checkbox"
              checked={draft.startAfterInstall}
              onChange={(event) => update((next) => { next.startAfterInstall = event.target.checked; })}
            />
            <span><strong>{text.settings.general.startAfterInstall}</strong><small>{text.settings.general.newInstalls}</small></span>
          </label>
          <label className="check-setting">
            <input
              type="checkbox"
              checked={draft.autoStartManagedRuntimes}
              onChange={(event) => update((next) => { next.autoStartManagedRuntimes = event.target.checked; })}
            />
            <span><strong>{text.settings.general.autoStart}</strong><small>{text.settings.general.autoStartHint}</small></span>
          </label>
        </div>
      </section>

      <section className="settings-section" aria-labelledby="settings-runtimes">
        <div className="settings-section-heading">
          <h2 id="settings-runtimes">{text.settings.runtimes.title}</h2>
          <p>{text.settings.runtimes.description}</p>
        </div>
        <div className="runtime-card active-runtime">
          <div className="runtime-card-heading">
            <div><strong>Ollama</strong><span>{text.settings.runtimes.defaultBadge}</span></div>
          </div>
          <p className="managed-runtime-note">{text.settings.ollama.managedHint}</p>
          <div className="settings-grid">
            <label>
              <span>{text.settings.ollama.endpoint}</span>
              <input
                value={draft.ollama.endpoint}
                onChange={(event) => update((next) => { next.ollama.endpoint = event.target.value; })}
                aria-invalid={errorsByField.has("ollama.endpoint")}
              />
              {fieldError("ollama.endpoint")}
            </label>
            <label>
              <span>{text.settings.ollama.executable}</span>
              <input
                value={draft.ollama.executablePath ?? ""}
                placeholder={text.settings.ollama.autoDetect}
                onChange={(event) => update((next) => { next.ollama.executablePath = event.target.value || undefined; })}
              />
            </label>
            <label>
              <span>{text.settings.ollama.contextLength}</span>
              <input
                type="number"
                min={256}
                max={1_048_576}
                value={draft.ollama.contextLength ?? ""}
                placeholder={text.settings.ollama.auto}
                onChange={(event) => update((next) => { next.ollama.contextLength = event.target.value ? valueFromNumber(event.target.value) : undefined; })}
              />
              {fieldError("ollama.contextLength")}
            </label>
            <label>
              <span>{text.settings.ollama.keepAlive}</span>
              <input
                value={draft.ollama.keepAlive}
                onChange={(event) => update((next) => { next.ollama.keepAlive = event.target.value; })}
              />
              {fieldError("ollama.keepAlive")}
            </label>
            <label>
              <span>{text.settings.ollama.maxLoaded}</span>
              <input
                type="number"
                min={1}
                max={128}
                value={draft.ollama.maxLoadedModels}
                onChange={(event) => update((next) => { next.ollama.maxLoadedModels = valueFromNumber(event.target.value); })}
              />
              {fieldError("ollama.maxLoadedModels")}
            </label>
            <label>
              <span>{text.settings.ollama.parallel}</span>
              <input
                type="number"
                min={1}
                max={128}
                value={draft.ollama.parallelRequests}
                onChange={(event) => update((next) => { next.ollama.parallelRequests = valueFromNumber(event.target.value); })}
              />
              {fieldError("ollama.parallelRequests")}
            </label>
            <label>
              <span>{text.settings.ollama.maxQueue}</span>
              <input
                type="number"
                min={1}
                max={100_000}
                value={draft.ollama.maxQueuedRequests}
                onChange={(event) => update((next) => { next.ollama.maxQueuedRequests = valueFromNumber(event.target.value); })}
              />
              {fieldError("ollama.maxQueuedRequests")}
            </label>
          </div>
          <p className="memory-estimate">
            {draft.ollama.contextLength
              ? text.settings.ollama.memoryEstimate(draft.ollama.contextLength, draft.ollama.parallelRequests)
              : text.settings.ollama.memoryAuto}
          </p>
          <details className="advanced-settings">
            <summary>{text.settings.ollama.advanced}</summary>
            <div className="settings-grid">
              <label>
                <span>{text.settings.ollama.gpuSelection}</span>
                <input
                  value={draft.ollama.gpuSelection ?? ""}
                  placeholder={text.settings.ollama.allDetected}
                  onChange={(event) => update((next) => { next.ollama.gpuSelection = event.target.value || undefined; })}
                />
                {fieldError("ollama.gpuSelection")}
              </label>
              <label className="check-setting">
                <input
                  type="checkbox"
                  checked={draft.ollama.experimentalVulkan}
                  onChange={(event) => update((next) => { next.ollama.experimentalVulkan = event.target.checked; })}
                />
                <span><strong>{text.settings.ollama.vulkan}</strong></span>
              </label>
              <label className="check-setting">
                <input
                  type="checkbox"
                  checked={draft.ollama.debugLogging}
                  onChange={(event) => update((next) => { next.ollama.debugLogging = event.target.checked; })}
                />
                <span><strong>{text.settings.ollama.debug}</strong></span>
              </label>
            </div>
          </details>
          <div className="runtime-actions">
            <button type="button" className="secondary-button" onClick={() => void testConnection()} disabled={testing}>
              {testing ? text.settings.ollama.testing : text.settings.ollama.test}
            </button>
            <button type="button" className="secondary-button" onClick={() => void repairOllama()} disabled={repairingOllama}>
              {repairingOllama ? text.settings.ollama.repairing : text.settings.ollama.repair}
            </button>
            {connection && (
              <p className={connection.healthy ? "connection-ok" : "connection-error"} role="status">
                {connection.message}
              </p>
            )}
          </div>
          {ollamaRepairProgress && (
            <div className="runtime-repair-progress" role="status" aria-live="polite">
              <div><span>{installProgressMessage(ollamaRepairProgress)}</span><strong>{Math.round(progressPercent(ollamaRepairProgress))}%</strong></div>
              <div className="runtime-repair-track"><i style={{ width: `${progressPercent(ollamaRepairProgress)}%` }} /></div>
              <small>{text.settings.ollama.repairHint}</small>
            </div>
          )}
        </div>
        <div className="runtime-card">
          <div className="runtime-card-heading">
            <div><strong>vLLM</strong><span>{text.settings.vllm.managedLinux}</span></div>
          </div>
          <p>{text.settings.vllm.managedHint}</p>
          {managedVllmSupport && (
            <p className={managedVllmSupport.supported ? "connection-ok" : "connection-error"}>
              {managedVllmSupport.message}
            </p>
          )}
          <div className="settings-grid">
            <label>
              <span>{text.settings.vllm.huggingFaceToken}</span>
              <input
                type="password"
                value={huggingFaceToken}
                autoComplete="off"
                placeholder={huggingFaceTokenSaved ? text.settings.vllm.tokenSaved : text.settings.vllm.tokenOptional}
                onChange={(event) => setHuggingFaceToken(event.target.value)}
              />
              <small>{text.settings.vllm.tokenHint}</small>
            </label>
            <label>
              <span>{text.settings.vllm.gpuMemory}</span>
              <input type="number" min={0.1} max={1} step={0.01} value={draft.vllm.gpuMemoryUtilization} onChange={(event) => update((next) => { next.vllm.gpuMemoryUtilization = Number(event.target.value); })} />
              {fieldError("vllm.gpuMemoryUtilization")}
            </label>
            <label>
              <span>{text.settings.vllm.contextLength}</span>
              <input type="number" min={256} value={draft.vllm.maxContextLength ?? ""} placeholder={text.settings.ollama.auto} onChange={(event) => update((next) => { next.vllm.maxContextLength = event.target.value ? valueFromNumber(event.target.value) : undefined; })} />
              {fieldError("vllm.maxContextLength")}
            </label>
            <label>
              <span>{text.settings.vllm.concurrency}</span>
              <input type="number" min={1} value={draft.vllm.maxConcurrentSequences} onChange={(event) => update((next) => { next.vllm.maxConcurrentSequences = valueFromNumber(event.target.value); })} />
            </label>
            <label>
              <span>{text.settings.vllm.weightDtype}</span>
              <input value={draft.vllm.weightDtype} onChange={(event) => update((next) => { next.vllm.weightDtype = event.target.value; })} />
            </label>
            <label>
              <span>{text.settings.vllm.kvCacheDtype}</span>
              <input value={draft.vllm.kvCacheDtype} onChange={(event) => update((next) => { next.vllm.kvCacheDtype = event.target.value; })} />
            </label>
            <label>
              <span>{text.settings.vllm.tensorParallel}</span>
              <input type="number" min={1} value={draft.vllm.tensorParallelSize} onChange={(event) => update((next) => { next.vllm.tensorParallelSize = valueFromNumber(event.target.value); })} />
            </label>
            <label>
              <span>{text.settings.vllm.pipelineParallel}</span>
              <input type="number" min={1} value={draft.vllm.pipelineParallelSize} onChange={(event) => update((next) => { next.vllm.pipelineParallelSize = valueFromNumber(event.target.value); })} />
            </label>
            <label>
              <span>{text.settings.vllm.portRange}</span>
              <span className="inline-number-fields">
                <input type="number" min={1024} value={draft.vllm.managedPortStart} onChange={(event) => update((next) => { next.vllm.managedPortStart = valueFromNumber(event.target.value); })} />
                <input type="number" min={1024} value={draft.vllm.managedPortEnd} onChange={(event) => update((next) => { next.vllm.managedPortEnd = valueFromNumber(event.target.value); })} />
              </span>
            </label>
            <label className="check-setting">
              <input type="checkbox" checked={draft.vllm.prefixCaching} onChange={(event) => update((next) => { next.vllm.prefixCaching = event.target.checked; })} />
              <span><strong>{text.settings.vllm.prefixCaching}</strong></span>
            </label>
          </div>
          <div className="runtime-actions">
            <button className="secondary-button" type="button" disabled={vllmBusy || !huggingFaceToken.trim()} onClick={() => void saveHuggingFaceToken()}>{text.settings.vllm.saveToken}</button>
            {huggingFaceTokenSaved && <button className="secondary-button" type="button" disabled={vllmBusy} onClick={() => void forgetHuggingFaceToken()}>{text.settings.vllm.forgetToken}</button>}
            <button className="secondary-button" type="button" disabled={vllmBusy || !managedVllmSupport?.supported} onClick={() => void deleteVllmCaches()}>{text.settings.vllm.deleteCaches}</button>
          </div>
        </div>
      </section>

      <section className="settings-section" aria-labelledby="settings-sharing">
        <div className="settings-section-heading">
          <h2 id="settings-sharing">Share an API</h2>
          <p>Use an authenticated OpenAI-compatible gateway. Ollama and managed vLLM remain private on this machine.</p>
        </div>
        <div className="sharing-settings-content">
          <div className="settings-grid">
          <label className="check-setting">
            <input
              type="checkbox"
              checked={sharingEnabled}
              onChange={(event) => setSharingEnabled(event.target.checked)}
            />
            <span>
              <strong>Enable API sharing</strong>
              <small>Localhost only unless you explicitly allow other devices.</small>
            </span>
          </label>
          <label className="check-setting">
            <input
              type="checkbox"
              checked={allowOtherDevices}
              disabled={!sharingEnabled}
              onChange={(event) => setAllowOtherDevices(event.target.checked)}
            />
            <span>
              <strong>Allow other devices</strong>
              <small>Requires authentication and an explicit unencrypted-transport confirmation.</small>
            </span>
          </label>
          <label>
            <span>Gateway port</span>
            <input
              type="number"
              min={1024}
              max={65535}
              value={sharingPort}
              disabled={!sharingEnabled}
              onChange={(event) => setSharingPort(valueFromNumber(event.target.value))}
            />
          </label>
          </div>
          <fieldset className="settings-choice-group sharing-model-picker" disabled={!sharingEnabled}>
            <legend>Models available through the gateway</legend>
            {shareableModels.length === 0 && (
              <p className="sharing-empty-state">No eligible local managed models are installed.</p>
            )}
          {shareableModels.map((model) => (
            <label className="check-setting" key={model.id}>
              <input
                type="checkbox"
                checked={exposedModelIds.includes(model.id)}
                onChange={(event) => setExposedModelIds((current) =>
                  event.target.checked
                    ? [...new Set([...current, model.id])]
                    : current.filter((id) => id !== model.id))}
              />
              <span><strong>{model.name}</strong><small>{model.runtimeId} · {model.runtimeModelId}</small></span>
            </label>
          ))}
          </fieldset>
          <div className="runtime-actions">
          <button type="button" className="secondary-button" disabled={sharingBusy} onClick={() => void generateSharingToken()}>
            {sharingStatus?.tokenSaved ? "Rotate API token" : "Generate API token"}
          </button>
          <button type="button" className="primary-button" disabled={sharingBusy || (sharingEnabled && exposedModelIds.length === 0)} onClick={() => void applySharing()}>
            Apply API sharing settings
          </button>
          {sharingStatus?.tokenSaved && (
            <button type="button" className="danger-button" disabled={sharingBusy} onClick={() => void revokeSharingToken()}>
              Revoke token and disable
            </button>
          )}
          </div>
          {generatedToken && (
            <div className="sharing-token-card" role="status">
              <div>
                <strong>Copy this token now</strong>
                <p>The full token is shown only after generation or rotation.</p>
              </div>
              <div className="sharing-token-value">
                <code>{generatedToken}</code>
                <button type="button" className="secondary-button" onClick={() => void copyGeneratedToken()}>
                  {tokenCopyStatus === "copied" ? "Copied" : tokenCopyStatus === "failed" ? "Try copying again" : "Copy token"}
                </button>
              </div>
            </div>
          )}
          {sharingStatus?.running && (
            <div className="connection-ok sharing-running-status" role="status">
              <strong>Gateway running at {sharingStatus.address}</strong>
              <p>Exposed models: {sharingStatus.exposedModels.join(", ")}</p>
            </div>
          )}
          {sharingStatus?.transportWarning && <p className="connection-error" role="alert">{sharingStatus.transportWarning}</p>}
          <p className="managed-runtime-note">
            Lumen Source does not open firewall ports or configure your router. For internet or untrusted-network use, terminate TLS and enforce network policy in a trusted reverse proxy.
          </p>
        </div>
      </section>

      <section className="settings-section" aria-labelledby="settings-storage">
        <div className="settings-section-heading">
          <h2 id="settings-storage">{text.settings.storage.title}</h2>
          <p>{text.settings.storage.description}</p>
        </div>
        <div className="settings-grid">
          <label>
            <span>{text.settings.storage.models}</span>
            <input
              value={draft.storage.modelDirectory ?? ""}
              placeholder={text.settings.ollama.systemDefault}
              onChange={(event) => update((next) => { next.storage.modelDirectory = event.target.value || undefined; })}
            />
          </label>
          <label>
            <span>{text.settings.storage.cache}</span>
            <input
              value={draft.storage.cacheDirectory ?? ""}
              placeholder={text.settings.ollama.systemDefault}
              onChange={(event) => update((next) => { next.storage.cacheDirectory = event.target.value || undefined; })}
            />
          </label>
        </div>
      </section>

      <section className="settings-section" aria-labelledby="settings-privacy">
        <div className="settings-section-heading">
          <h2 id="settings-privacy">{text.settings.privacy.title}</h2>
          <p>{text.settings.privacy.description}</p>
        </div>
        <div className="settings-grid">
          <label className="check-setting">
            <input
              type="checkbox"
              checked={draft.privacy.telemetryEnabled}
              onChange={(event) => update((next) => { next.privacy.telemetryEnabled = event.target.checked; })}
            />
            <span><strong>{text.settings.privacy.telemetry}</strong><small>{text.settings.privacy.telemetryHint}</small></span>
          </label>
          <label className="check-setting">
            <input
              type="checkbox"
              checked={draft.privacy.confirmModelDeletion}
              onChange={(event) => update((next) => { next.privacy.confirmModelDeletion = event.target.checked; })}
            />
            <span><strong>{text.settings.privacy.confirmDeletion}</strong></span>
          </label>
          <label>
            <span>{text.settings.privacy.logRetention}</span>
            <input
              type="number"
              min={0}
              max={10_000}
              value={draft.privacy.lifecycleLogRetention}
              onChange={(event) => update((next) => { next.privacy.lifecycleLogRetention = valueFromNumber(event.target.value); })}
            />
            {fieldError("privacy.lifecycleLogRetention")}
          </label>
        </div>
      </section>

      <section className="settings-section" aria-labelledby="settings-support">
        <div className="settings-section-heading">
          <h2 id="settings-support">Diagnostics and recovery</h2>
          <p>Export privacy-safe support information or protect and restore Lumen Source state.</p>
        </div>
        <div className="runtime-actions">
          <button type="button" className="secondary-button" disabled={supportBusy} onClick={() => void exportDiagnostics()}>
            Export redacted diagnostics
          </button>
          <button type="button" className="secondary-button" disabled={supportBusy} onClick={() => void exportStateBackup()}>
            Back up settings and inventory
          </button>
          <button type="button" className="secondary-button" disabled={supportBusy} onClick={() => restoreInput.current?.click()}>
            Restore a backup
          </button>
          <input
            ref={restoreInput}
            className="visually-hidden"
            type="file"
            accept="application/json,.json"
            aria-label="Choose a Lumen Source state backup"
            onChange={(event) => void restoreStateBackup(event.target.files?.[0])}
          />
        </div>
        {supportNotice && <p className="settings-saved support-notice" role="status">{supportNotice}</p>}
        <p className="managed-runtime-note">
          Diagnostics exclude secrets, prompts, responses, user paths, hostnames, and raw addresses. State backups exclude credential-store secrets.
        </p>
        <div className="runtime-actions">
          <button type="button" className="secondary-button" disabled={supportBusy} onClick={() => void safeReset(false)}>
            Safe reset
          </button>
          <button type="button" className="danger-button" disabled={supportBusy} onClick={() => void safeReset(true)}>
            Reset and forget inventory
          </button>
        </div>
        <p className="managed-runtime-note">
          Neither reset deletes model weights, runtime installations, or shared caches. Use Models and Storage for explicit data removal.
        </p>
      </section>

      <section className="settings-section about-settings" aria-labelledby="settings-about">
        <div className="settings-section-heading">
          <h2 id="settings-about">{text.settings.about.title}</h2>
          <p>{text.settings.about.description}</p>
        </div>
        <dl><div><dt>{text.settings.about.version}</dt><dd>{APP_VERSION}</dd></div><div><dt>{text.settings.about.settingsSchema}</dt><dd>{draft.schemaVersion}</dd></div></dl>
      </section>
    </div>
  );
}
