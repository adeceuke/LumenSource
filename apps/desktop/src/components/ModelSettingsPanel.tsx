import { useEffect, useMemo, useState } from "react";
import { desktopCommands, messageFromError } from "../commands";
import { browserMessages } from "../i18n";
import type {
  ApplicationSettings,
  ModelSettings,
  ModelUpdatePlan,
  RunningModelEntry,
  RuntimeDiagnostics,
  RuntimeMigrationOption,
} from "../types";

const text = browserMessages();

interface ModelSettingsPanelProps {
  model: RunningModelEntry;
  applicationSettings: ApplicationSettings;
  onSaved: (model: RunningModelEntry) => void;
  onInventoryChanged: (models: RunningModelEntry[]) => void;
}

function defaultModelSettings(): ModelSettings {
  return {
    verifyTls: true,
    connectionTimeoutSeconds: 5,
    requestTimeoutSeconds: 120,
    stopSequences: [],
    ollamaPersistentParameters: false,
  };
}

function cloneSettings(model: RunningModelEntry): ModelSettings {
  return structuredClone(model.modelSettings ?? defaultModelSettings());
}

function clearOverrides(current: ModelSettings): ModelSettings {
  return {
    ...defaultModelSettings(),
    performanceProfile: current.performanceProfile,
    runtimeManagementMode: current.runtimeManagementMode,
    inferenceTask: current.inferenceTask,
    endpoint: current.endpoint,
    verifyTls: current.verifyTls,
    connectionTimeoutSeconds: current.connectionTimeoutSeconds,
    requestTimeoutSeconds: current.requestTimeoutSeconds,
    managedContainerEngine: current.managedContainerEngine,
    managedContainerName: current.managedContainerName,
    managedPort: current.managedPort,
  };
}

function clearChatOverrides(current: ModelSettings): ModelSettings {
  return {
    ...current,
    systemPrompt: undefined,
    temperature: undefined,
    maxOutputTokens: undefined,
    topP: undefined,
    topK: undefined,
    minP: undefined,
    repetitionPenalty: undefined,
    seed: undefined,
    stopSequences: [],
    structuredOutput: undefined,
    reasoningLevel: undefined,
  };
}

function optionalNumber(value: string): number | undefined {
  return value === "" ? undefined : Number(value);
}

export function ModelSettingsPanel({
  model,
  applicationSettings,
  onSaved,
  onInventoryChanged,
}: ModelSettingsPanelProps) {
  const [draft, setDraft] = useState(() => cloneSettings(model));
  const [baseline, setBaseline] = useState(() => cloneSettings(model));
  const [saving, setSaving] = useState(false);
  const [notice, setNotice] = useState<string>();
  const [error, setError] = useState<string>();
  const [diagnostics, setDiagnostics] = useState<RuntimeDiagnostics>();
  const [migrationOptions, setMigrationOptions] = useState<RuntimeMigrationOption[]>([]);
  const [migrating, setMigrating] = useState(false);
  const [huggingFaceToken, setHuggingFaceToken] = useState("");
  const [inventoryBusy, setInventoryBusy] = useState(false);
  const [memoryWarning, setMemoryWarning] = useState<string>();
  const [updatePlan, setUpdatePlan] = useState<ModelUpdatePlan>();
  const [updateBusy, setUpdateBusy] = useState(false);
  const [updateLicenseAcknowledged, setUpdateLicenseAcknowledged] = useState(false);
  const dirty = useMemo(
    () => JSON.stringify(draft) !== JSON.stringify(baseline),
    [baseline, draft],
  );
  const external = model.runtimeCapabilities.lifecycle === "external";
  const managedVllm = model.runtimeId === "vllm" && !external;

  useEffect(() => {
    let active = true;
    void Promise.all([
      desktopCommands.runtimeDiagnostics(model.id),
      desktopCommands.runtimeMigrationOptions(model.id),
      desktopCommands.modelUpdatePlan(model.id).catch(() => undefined),
    ]).then(([nextDiagnostics, nextOptions, nextUpdatePlan]) => {
      if (!active) return;
      setDiagnostics(nextDiagnostics);
      setMigrationOptions(nextOptions);
      setUpdatePlan(nextUpdatePlan);
    }).catch(() => {
      if (active) setMigrationOptions([]);
    });
    return () => {
      active = false;
    };
  }, [model.id, model.running]);

  const applyUpdate = async () => {
    setUpdateBusy(true);
    setError(undefined);
    try {
      const updated = await desktopCommands.applyModelUpdate(
        model.id,
        updateLicenseAcknowledged,
      );
      onSaved(updated);
      setUpdatePlan(await desktopCommands.modelUpdatePlan(updated.id));
      setNotice("The candidate passed validation. The previous revision is retained for rollback.");
    } catch (updateError) {
      setError(messageFromError(updateError, text.errors.unexpected));
    } finally {
      setUpdateBusy(false);
    }
  };

  const rollbackUpdate = async () => {
    if (!window.confirm("Restore the previous validated revision? The current candidate will be removed, while shared caches remain.")) return;
    setUpdateBusy(true);
    setError(undefined);
    try {
      const restored = await desktopCommands.rollbackModelUpdate(model.id);
      onSaved(restored);
      setUpdatePlan(await desktopCommands.modelUpdatePlan(restored.id));
      setNotice("The previous validated revision was restored.");
    } catch (rollbackError) {
      setError(messageFromError(rollbackError, text.errors.unexpected));
    } finally {
      setUpdateBusy(false);
    }
  };

  useEffect(() => {
    let active = true;
    const timer = window.setTimeout(() => {
      void desktopCommands.modelSettingsMemoryWarning(model.id, draft)
        .then((warning) => {
          if (active) setMemoryWarning(warning ?? undefined);
        })
        .catch(() => {
          if (active) setMemoryWarning(undefined);
        });
    }, 250);
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [draft, model.id]);

  const update = (change: Partial<ModelSettings>) => {
    setDraft((current) => ({ ...current, ...change }));
    setNotice(undefined);
    setError(undefined);
  };

  const save = async (applyRestart: boolean) => {
    setSaving(true);
    setError(undefined);
    setNotice(undefined);
    try {
      const report = await desktopCommands.saveModelSettings(model.id, draft, applyRestart);
      const saved = structuredClone(report.model.modelSettings ?? draft);
      setDraft(saved);
      setBaseline(saved);
      setNotice(report.message);
      onSaved(report.model);
    } catch (saveError) {
      setError(messageFromError(saveError, text.errors.unexpected));
    } finally {
      setSaving(false);
    }
  };

  const reinstall = async (option: RuntimeMigrationOption) => {
    if (!option.available || !window.confirm(text.modelSettings.reinstallConfirmation(option.runtimeId))) {
      return;
    }
    setMigrating(true);
    setError(undefined);
    setNotice(undefined);
    try {
      if (option.requiresHuggingFaceToken && !option.tokenSaved) {
        const token = huggingFaceToken.trim();
        if (!token) {
          setError(text.modelSettings.gatedTokenRequired);
          return;
        }
        await desktopCommands.saveRuntimeSecret("huggingFaceToken", token);
      }
      const report = await desktopCommands.reinstallWithRuntime(model.id, option.runtimeId);
      setHuggingFaceToken("");
      setNotice(report.message);
      onSaved(report.replacement);
      setMigrationOptions((current) => current.map((candidate) => (
        candidate.runtimeId === option.runtimeId
          ? { ...candidate, available: false, reason: text.modelSettings.alreadyInstalled }
          : candidate
      )));
    } catch (migrationError) {
      setError(messageFromError(migrationError, text.errors.unexpected));
    } finally {
      setMigrating(false);
    }
  };

  const inheritedContext = model.runtimeId === "ollama"
    ? applicationSettings.ollama.contextLength
    : applicationSettings.vllm.maxContextLength;
  const inheritedKeepAlive = model.runtimeId === "ollama"
    ? applicationSettings.ollama.keepAlive
    : undefined;

  const inventoryAction = async (
    action: "rescan" | "repair" | "reconnect" | "adopt" | "forget" | "pin" | "unpin" | "rematch",
  ) => {
    let variantId: string | undefined;
    if (action === "forget" && !window.confirm(
      "Forget this entry? Model weights, runtime services, shared caches, and credentials will not be removed.",
    )) return;
    if (action === "rematch") {
      variantId = window.prompt("Enter the exact catalog variant ID. Model weights will not be changed.")?.trim();
      if (!variantId) return;
    }
    setInventoryBusy(true);
    setError(undefined);
    try {
      const models = await desktopCommands.inventoryAction(model.id, action, variantId);
      onInventoryChanged(models);
    } catch (actionError) {
      setError(messageFromError(actionError, text.errors.unexpected));
    } finally {
      setInventoryBusy(false);
    }
  };

  return (
    <section className="detail-section model-settings-panel" aria-labelledby="model-settings-title">
      <div className="detail-section-header">
        <div>
          <h2 id="model-settings-title">{text.modelSettings.title}</h2>
          <p>{text.modelSettings.description}</p>
        </div>
        <span className={dirty ? "settings-dirty" : "settings-saved"}>
          {dirty ? text.modelSettings.unsaved : text.modelSettings.saved}
        </span>
      </div>

      <fieldset className="model-settings-group inventory-state">
        <legend>Inventory</legend>
        <p>
          Status: <strong>{model.inventoryStatus ?? "available"}</strong>
          {model.discovered ? " · Discovered from the runtime" : " · Added by Lumen Source"}
          {model.lastSeenAt ? ` · Last seen ${new Date(model.lastSeenAt).toLocaleString(text.locale)}` : ""}
        </p>
        <div className="runtime-actions">
          <button className="secondary-button" type="button" disabled={inventoryBusy} onClick={() => void inventoryAction("rescan")}>Rescan</button>
          {model.inventoryStatus === "missing" && model.runtimeId === "ollama" && (
            <button className="secondary-button" type="button" disabled={inventoryBusy} onClick={() => void inventoryAction("repair")}>Repair model</button>
          )}
          {model.inventoryStatus === "needsReconnect" && (
            <button className="secondary-button" type="button" disabled={inventoryBusy} onClick={() => void inventoryAction("reconnect")}>Reconnect</button>
          )}
          {model.discovered && !model.managed && (
            <button className="secondary-button" type="button" disabled={inventoryBusy} onClick={() => void inventoryAction("adopt")}>Adopt without downloading</button>
          )}
          {model.discovered && model.modelId.startsWith("external:") && (
            <button className="secondary-button" type="button" disabled={inventoryBusy} onClick={() => void inventoryAction("rematch")}>Match catalog variant</button>
          )}
          <button className="secondary-button" type="button" disabled={inventoryBusy} onClick={() => void inventoryAction(model.pinned ? "unpin" : "pin")}>
            {model.pinned ? "Allow automatic stopping" : "Keep always available"}
          </button>
          <button className="secondary-button" type="button" disabled={inventoryBusy} onClick={() => void inventoryAction("forget")}>Forget entry only</button>
        </div>
      </fieldset>

      <fieldset className="model-settings-group update-state">
        <legend>Updates and rollback</legend>
        {!updatePlan ? <p>Checking the active catalog…</p> : (
          <>
            <dl className="runtime-diagnostics">
              <div><dt>Current revision</dt><dd>{updatePlan.currentRevision}</dd></div>
              <div><dt>Catalog candidate</dt><dd>{updatePlan.candidateRevision}</dd></div>
              <div><dt>Classification</dt><dd>{updatePlan.classification}</dd></div>
              <div><dt>Changes</dt><dd>{updatePlan.kinds.length > 0 ? updatePlan.kinds.join(", ") : "Metadata only"}</dd></div>
              <div><dt>Download</dt><dd>{updatePlan.downloadBytes.toLocaleString(text.locale)} bytes</dd></div>
              <div><dt>Additional rollback space</dt><dd>{updatePlan.additionalDiskBytes.toLocaleString(text.locale)} bytes</dd></div>
            </dl>
            <p>{updatePlan.compatibility}</p>
            <p>{updatePlan.message}</p>
            {updatePlan.licenseChanged && (
              <div className="api-notice warning">
                <strong>License or usage policy changed</strong>
                <span>Review the current catalog terms before updating. Existing weights and settings are not changed until you continue.</span>
              </div>
            )}
            {updatePlan.requiresLicenseAcknowledgement && (
              <label className="checkbox-row">
                <input type="checkbox" checked={updateLicenseAcknowledged} onChange={(event) => setUpdateLicenseAcknowledged(event.target.checked)} />
                <span>I acknowledge the current cataloged terms for this update.</span>
              </label>
            )}
            <div className="runtime-actions">
              <button
                className="primary-button"
                type="button"
                disabled={!updatePlan.available || updateBusy || (updatePlan.requiresLicenseAcknowledgement && !updateLicenseAcknowledged)}
                onClick={() => void applyUpdate()}
              >
                {updateBusy ? "Working…" : "Install and validate update"}
              </button>
              {model.rollback && (
                <button className="secondary-button" type="button" disabled={updateBusy} onClick={() => void rollbackUpdate()}>
                  Restore previous revision
                </button>
              )}
            </div>
          </>
        )}
      </fieldset>

      {draft.performanceProfile && (
        <p className="model-profile-summary">
          <strong>{text.modelSettings.performanceProfile}:</strong>{" "}
          {text.modelSettings.performanceProfileValue(draft.performanceProfile)}
        </p>
      )}
      {memoryWarning && <div className="api-notice warning" role="status"><strong>Memory warning</strong><span>{memoryWarning}</span></div>}

      {model.installationValidation && (
        <fieldset className="model-settings-group">
          <legend>{text.modelSettings.installationValidation}</legend>
          <p>{model.installationValidation.message}</p>
          <dl className="runtime-diagnostics">
            <div><dt>{text.modelSettings.validatedAt}</dt><dd>{new Date(model.installationValidation.validatedAt).toLocaleString()}</dd></div>
            <div><dt>{text.modelSettings.validationCapability}</dt><dd>{model.installationValidation.capability}</dd></div>
            <div><dt>{text.modelSettings.validationAllocation}</dt><dd>{model.installationValidation.accelerator ?? text.common.unavailable}</dd></div>
            <div><dt>{text.modelSettings.validationHardware}</dt><dd>{model.installationValidation.hardwareSummary ?? text.common.unavailable}</dd></div>
            <div><dt>{text.modelSettings.effectiveContext}</dt><dd>{model.installationValidation.effectiveContextLength?.toLocaleString(text.locale) ?? text.common.unavailable}</dd></div>
          </dl>
        </fieldset>
      )}

      <fieldset className="model-settings-group">
        <legend>{text.modelSettings.chatDefaults}</legend>
        <p>{text.modelSettings.chatDefaultsHint}</p>
        <div className="model-settings-grid">
          <label className="wide-setting">
            <span>{text.modelSettings.systemPrompt}</span>
            <textarea
              rows={3}
              value={draft.systemPrompt ?? ""}
              placeholder={text.modelSettings.runtimeDefault}
              onChange={(event) => update({ systemPrompt: event.target.value || undefined })}
            />
          </label>
          <NumberSetting label={text.modelSettings.temperature} value={draft.temperature} min={0} max={2} step={0.05} onChange={(temperature) => update({ temperature })} />
          <NumberSetting label={text.modelSettings.maxOutputTokens} value={draft.maxOutputTokens} min={1} step={1} onChange={(maxOutputTokens) => update({ maxOutputTokens })} />
          <NumberSetting label={text.modelSettings.topP} value={draft.topP} min={0} max={1} step={0.01} onChange={(topP) => update({ topP })} />
          <NumberSetting label={text.modelSettings.topK} value={draft.topK} min={1} step={1} onChange={(topK) => update({ topK })} />
          <NumberSetting label={text.modelSettings.minP} value={draft.minP} min={0} max={1} step={0.01} onChange={(minP) => update({ minP })} />
          <NumberSetting label={text.modelSettings.repetitionPenalty} value={draft.repetitionPenalty} min={0} max={2} step={0.01} onChange={(repetitionPenalty) => update({ repetitionPenalty })} />
          <NumberSetting label={text.modelSettings.seed} value={draft.seed} step={1} onChange={(seed) => update({ seed })} />
          <label>
            <span>{text.modelSettings.structuredOutput}</span>
            <select
              value={draft.structuredOutput === undefined ? "" : String(draft.structuredOutput)}
              onChange={(event) => update({
                structuredOutput: event.target.value === "" ? undefined : event.target.value === "true",
              })}
            >
              <option value="">{text.modelSettings.runtimeDefault}</option>
              <option value="false">{text.common.disabled}</option>
              <option value="true">{text.common.enabled}</option>
            </select>
          </label>
          <label>
            <span>{text.modelSettings.reasoningLevel}</span>
            <select
              value={draft.reasoningLevel ?? ""}
              onChange={(event) => update({
                reasoningLevel: event.target.value
                  ? event.target.value as ModelSettings["reasoningLevel"]
                  : undefined,
              })}
            >
              <option value="">{text.modelSettings.runtimeDefault}</option>
              {model.runtimeId === "ollama" && <option value="off">Off</option>}
              <option value="low">Low</option>
              <option value="medium">Medium</option>
              <option value="high">High</option>
            </select>
          </label>
          <label className="wide-setting">
            <span>{text.modelSettings.stopSequences}</span>
            <textarea
              rows={3}
              value={draft.stopSequences.join("\n")}
              placeholder={text.modelSettings.onePerLine}
              onChange={(event) => update({
                stopSequences: event.target.value.split("\n").map((value) => value.trim()).filter(Boolean),
              })}
            />
          </label>
        </div>
      </fieldset>

      <fieldset className="model-settings-group">
        <legend>{text.modelSettings.loadSettings}</legend>
        <p>{external ? text.modelSettings.externalReadOnly : text.modelSettings.restartHint}</p>
        <div className="model-settings-grid">
          <label>
            <span>{text.modelSettings.contextLength}</span>
            <input
              type="number"
              min={256}
              value={draft.contextLength ?? ""}
              placeholder={inheritedContext ? String(inheritedContext) : text.modelSettings.auto}
              disabled={external}
              onChange={(event) => update({ contextLength: optionalNumber(event.target.value) })}
            />
            <small>{text.modelSettings.inheritedValue(inheritedContext ? String(inheritedContext) : text.modelSettings.auto)}</small>
          </label>
          <label>
            <span>{text.modelSettings.keepAlive}</span>
            <input
              value={draft.keepAlive ?? ""}
              placeholder={inheritedKeepAlive ?? text.modelSettings.runtimeDefault}
              disabled={external}
              onChange={(event) => update({ keepAlive: event.target.value || undefined })}
            />
            <small>{text.modelSettings.inheritedValue(inheritedKeepAlive ?? text.modelSettings.runtimeDefault)}</small>
          </label>
          <label>
            <span>{text.modelSettings.loadOnStartup}</span>
            <select
              value={draft.loadOnStartup === undefined ? "" : String(draft.loadOnStartup)}
              disabled={external}
              onChange={(event) => update({
                loadOnStartup: event.target.value === "" ? undefined : event.target.value === "true",
              })}
            >
              <option value="">{text.modelSettings.runtimeDefault}</option>
              <option value="true">{text.common.enabled}</option>
              <option value="false">{text.common.disabled}</option>
            </select>
          </label>
          <label>
            <span>{text.modelSettings.accelerator}</span>
            <input
              value={draft.preferredAccelerator ?? ""}
              placeholder={text.modelSettings.auto}
              disabled={external}
              onChange={(event) => update({ preferredAccelerator: event.target.value || undefined })}
            />
          </label>
        </div>
      </fieldset>

      {model.runtimeId === "ollama" && (
        <fieldset className="model-settings-group">
          <legend>{text.modelSettings.ollamaOverrides}</legend>
          <p>{text.modelSettings.ollamaOverridesHint}</p>
          <div className="model-settings-grid">
            <label>
              <span>{text.modelSettings.derivedName}</span>
              <input
                value={draft.ollamaDerivedModelName ?? ""}
                placeholder={text.modelSettings.noDerivedModel}
                onChange={(event) => update({ ollamaDerivedModelName: event.target.value || undefined })}
              />
            </label>
            <label className="check-setting">
              <input
                type="checkbox"
                checked={draft.ollamaPersistentParameters}
                onChange={(event) => update({ ollamaPersistentParameters: event.target.checked })}
              />
              <span><strong>{text.modelSettings.persistentParameters}</strong><small>{text.modelSettings.persistentParametersHint}</small></span>
            </label>
          </div>
        </fieldset>
      )}

      {model.runtimeId === "vllm" && (
        <fieldset className="model-settings-group">
          <legend>{text.modelSettings.vllmEngine}</legend>
          <p>{external ? text.modelSettings.externalVllmEngine : text.modelSettings.managedVllmEngine}</p>
          <div className="model-settings-grid">
            <TextSetting label={text.modelSettings.modelRevision} value={draft.vllmModelRevision} disabled={!managedVllm} onChange={(vllmModelRevision) => update({ vllmModelRevision })} />
            <TextSetting label={text.modelSettings.tokenizerRevision} value={draft.vllmTokenizerRevision} disabled={!managedVllm} onChange={(vllmTokenizerRevision) => update({ vllmTokenizerRevision })} />
            <TextSetting label={text.modelSettings.servedName} value={draft.vllmServedModelName} disabled={!managedVllm} onChange={(vllmServedModelName) => update({ vllmServedModelName })} />
            <TextSetting label={text.modelSettings.taskRunner} value={draft.vllmTask} disabled={!managedVllm} onChange={(vllmTask) => update({ vllmTask })} />
            <TextSetting label={text.modelSettings.weightDtype} value={draft.vllmWeightDtype} disabled={!managedVllm} onChange={(vllmWeightDtype) => update({ vllmWeightDtype })} />
            <TextSetting label={text.modelSettings.quantization} value={draft.vllmQuantization} disabled={!managedVllm} onChange={(vllmQuantization) => update({ vllmQuantization })} />
            <NumberSetting label={text.modelSettings.gpuMemory} value={draft.vllmGpuMemoryUtilization} min={0.1} max={1} step={0.01} disabled={!managedVllm} onChange={(vllmGpuMemoryUtilization) => update({ vllmGpuMemoryUtilization })} />
            <NumberSetting label={text.modelSettings.concurrency} value={draft.vllmMaxConcurrentSequences} min={1} step={1} disabled={!managedVllm} onChange={(vllmMaxConcurrentSequences) => update({ vllmMaxConcurrentSequences })} />
            <NumberSetting label={text.modelSettings.tensorParallel} value={draft.vllmTensorParallelSize} min={1} step={1} disabled={!managedVllm} onChange={(vllmTensorParallelSize) => update({ vllmTensorParallelSize })} />
            <NumberSetting label={text.modelSettings.pipelineParallel} value={draft.vllmPipelineParallelSize} min={1} step={1} disabled={!managedVllm} onChange={(vllmPipelineParallelSize) => update({ vllmPipelineParallelSize })} />
          </div>
        </fieldset>
      )}

      <fieldset className="model-settings-group">
        <legend>{text.modelSettings.diagnostics}</legend>
        {diagnostics ? (
          <dl className="runtime-diagnostics">
            <div><dt>{text.modelSettings.runtime}</dt><dd>{diagnostics.runtimeId} {diagnostics.version}</dd></div>
            <div><dt>{text.modelSettings.health}</dt><dd>{diagnostics.health}</dd></div>
            <div><dt>{text.modelSettings.lifecycle}</dt><dd>{diagnostics.lifecycle}</dd></div>
            <div><dt>{text.modelSettings.endpoint}</dt><dd>{diagnostics.endpoint ?? text.common.unavailable}</dd></div>
            <div><dt>{text.modelSettings.effectiveContext}</dt><dd>{diagnostics.effectiveContextLength?.toLocaleString(text.locale) ?? text.modelSettings.auto}</dd></div>
            {diagnostics.managedContainerName && (
              <div><dt>{text.modelSettings.container}</dt><dd>{diagnostics.managedContainerEngine}: {diagnostics.managedContainerName} ({diagnostics.managedPort})</dd></div>
            )}
          </dl>
        ) : <p>{text.modelSettings.loadingDiagnostics}</p>}
      </fieldset>

      {migrationOptions.length > 0 && (
        <fieldset className="model-settings-group">
          <legend>{text.modelSettings.migration}</legend>
          <p>{text.modelSettings.migrationHint}</p>
          {migrationOptions.map((option) => (
            <div className="runtime-migration-option" key={option.runtimeId}>
              <div>
                <span>{option.reason}</span>
                {option.requiresHuggingFaceToken && !option.tokenSaved && (
                  <label className="gated-model-token">
                    <span>{text.modelSettings.gatedToken}</span>
                    <input
                      type="password"
                      value={huggingFaceToken}
                      autoComplete="off"
                      onChange={(event) => setHuggingFaceToken(event.target.value)}
                    />
                    <small>{text.modelSettings.gatedTokenHint}</small>
                  </label>
                )}
                {option.requiresHuggingFaceToken && option.tokenSaved && (
                  <small>{text.modelSettings.gatedTokenSaved}</small>
                )}
              </div>
              <button
                className="secondary-button"
                type="button"
                disabled={!option.available || migrating || (
                  option.requiresHuggingFaceToken
                  && !option.tokenSaved
                  && !huggingFaceToken.trim()
                )}
                onClick={() => void reinstall(option)}
              >
                {migrating ? text.modelSettings.reinstalling : text.modelSettings.reinstallWith(option.runtimeId)}
              </button>
            </div>
          ))}
        </fieldset>
      )}

      {error && <div className="inline-error" role="alert">{error}</div>}
      {notice && <div className="connection-report success" role="status">{notice}</div>}

      <div className="model-settings-actions">
        <button className="secondary-button" type="button" disabled={saving || !dirty} onClick={() => setDraft(structuredClone(baseline))}>{text.modelSettings.discard}</button>
        <button className="secondary-button" type="button" disabled={saving} onClick={() => setDraft(clearChatOverrides(draft))}>{text.modelSettings.useRuntimeDefaults}</button>
        <button className="secondary-button" type="button" disabled={saving} onClick={() => setDraft(clearOverrides(draft))}>{text.modelSettings.resetOverrides}</button>
        <button className="primary-button" type="button" disabled={saving || !dirty} onClick={() => void save(false)}>{saving ? text.common.saving : text.common.save}</button>
        {!external && model.runtimeCapabilities.modelStartStop && (
          <button className="primary-button" type="button" disabled={saving || !dirty} onClick={() => void save(true)}>{text.modelSettings.applyRestart}</button>
        )}
      </div>
    </section>
  );
}

function NumberSetting({
  label,
  value,
  min,
  max,
  step,
  disabled,
  onChange,
}: {
  label: string;
  value?: number;
  min?: number;
  max?: number;
  step: number;
  disabled?: boolean;
  onChange: (value?: number) => void;
}) {
  return (
    <label>
      <span>{label}</span>
      <input type="number" value={value ?? ""} min={min} max={max} step={step} disabled={disabled} placeholder={text.modelSettings.runtimeDefault} onChange={(event) => onChange(optionalNumber(event.target.value))} />
    </label>
  );
}

function TextSetting({
  label,
  value,
  disabled,
  onChange,
}: {
  label: string;
  value?: string;
  disabled?: boolean;
  onChange: (value?: string) => void;
}) {
  return (
    <label>
      <span>{label}</span>
      <input value={value ?? ""} disabled={disabled} placeholder={text.modelSettings.runtimeDefault} onChange={(event) => onChange(event.target.value || undefined)} />
    </label>
  );
}
