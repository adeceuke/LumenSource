import { useEffect, useMemo, useState } from "react";
import { desktopCommands, messageFromError } from "../commands";
import { browserMessages } from "../i18n";
import type {
  ApplicationSettings,
  OllamaConnectionReport,
  RemoteTargetProfile,
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

function exposesNetwork(bindAddress: string): boolean {
  const address = bindAddress.trim().toLowerCase();
  return !(address.startsWith("127.")
    || address.startsWith("localhost:")
    || address.startsWith("[::1]:")
    || address === "::1");
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
  const [restartRequired, setRestartRequired] = useState(false);
  const [connection, setConnection] = useState<OllamaConnectionReport>();
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
  }, []);

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
      const confirmationRequired = exposesNetwork(draft.ollama.bindAddress);
      const confirmed = !confirmationRequired || window.confirm(text.settings.networkWarning);
      if (!confirmed) return;
      const report = await desktopCommands.saveSettings(draft, confirmed);
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
              <option value="vllm" disabled>vLLM ({text.settings.comingNext})</option>
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
              <label>
                <span>{text.settings.ollama.bindAddress}</span>
                <input
                  value={draft.ollama.bindAddress}
                  onChange={(event) => update((next) => { next.ollama.bindAddress = event.target.value; })}
                />
                {fieldError("ollama.bindAddress")}
              </label>
              <label className="wide-setting">
                <span>{text.settings.ollama.allowedOrigins}</span>
                <input
                  value={draft.ollama.allowedOrigins.join(", ")}
                  placeholder="https://app.example.com"
                  onChange={(event) => update((next) => {
                    next.ollama.allowedOrigins = event.target.value.split(",").map((value) => value.trim()).filter(Boolean);
                  })}
                />
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
            {connection && (
              <p className={connection.healthy ? "connection-ok" : "connection-error"} role="status">
                {connection.message}
              </p>
            )}
          </div>
        </div>
        <div className="runtime-card disabled-runtime">
          <div className="runtime-card-heading"><div><strong>vLLM</strong><span>{text.settings.comingNext}</span></div></div>
          <p>{text.settings.runtimes.vllmPreview}</p>
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

      <section className="settings-section about-settings" aria-labelledby="settings-about">
        <div className="settings-section-heading">
          <h2 id="settings-about">{text.settings.about.title}</h2>
          <p>{text.settings.about.description}</p>
        </div>
        <dl><div><dt>{text.settings.about.version}</dt><dd>0.4.0</dd></div><div><dt>{text.settings.about.settingsSchema}</dt><dd>{draft.schemaVersion}</dd></div></dl>
      </section>
    </div>
  );
}
