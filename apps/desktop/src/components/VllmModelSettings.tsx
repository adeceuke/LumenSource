import { useEffect, useState } from "react";
import { desktopCommands, messageFromError } from "../commands";
import { browserMessages } from "../i18n";
import { useModalFocus } from "../hooks/useModalFocus";
import type {
  ExternalVllmConfig,
  RunningModelEntry,
  VllmConnectionReport,
} from "../types";

const text = browserMessages();

interface VllmModelSettingsProps {
  model?: RunningModelEntry;
  dialog?: boolean;
  onCancel?: () => void;
  onSaved: (model: RunningModelEntry) => void;
}

function initialConfig(model?: RunningModelEntry): ExternalVllmConfig {
  return {
    endpoint: model?.modelSettings?.endpoint ?? "http://127.0.0.1:8000",
    servedModel: model?.runtimeModelId ?? "",
    inferenceTask: model?.modelSettings?.inferenceTask ?? "chat",
    verifyTls: model?.modelSettings?.verifyTls ?? true,
    connectionTimeoutSeconds: model?.modelSettings?.connectionTimeoutSeconds ?? 5,
    requestTimeoutSeconds: model?.modelSettings?.requestTimeoutSeconds ?? 120,
  };
}

export function VllmModelSettings({
  model,
  dialog = false,
  onCancel,
  onSaved,
}: VllmModelSettingsProps) {
  const dialogRef = useModalFocus<HTMLFormElement>(onCancel, dialog);
  const [displayName, setDisplayName] = useState(model?.name ?? "");
  const [config, setConfig] = useState(() => initialConfig(model));
  const [apiKey, setApiKey] = useState("");
  const [credentialSaved, setCredentialSaved] = useState(false);
  const [clearApiKey, setClearApiKey] = useState(false);
  const [report, setReport] = useState<VllmConnectionReport>();
  const [testing, setTesting] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string>();

  useEffect(() => {
    if (!model) return;
    void desktopCommands.vllmCredentialStatus(model.id)
      .then(setCredentialSaved)
      .catch(() => setCredentialSaved(false));
  }, [model]);

  const updateConfig = (change: Partial<ExternalVllmConfig>) => {
    setConfig((current) => ({ ...current, ...change }));
    setReport(undefined);
    setError(undefined);
  };

  const test = async () => {
    setTesting(true);
    setError(undefined);
    try {
      const result = await desktopCommands.testVllmConnection(
        config,
        apiKey || undefined,
        model?.id,
      );
      setReport(result);
      if (result.healthy && result.models.length > 0 && !result.models.includes(config.servedModel)) {
        setConfig((current) => ({ ...current, servedModel: result.models[0] }));
      }
    } catch (testError) {
      setError(messageFromError(testError, text.errors.unexpected));
    } finally {
      setTesting(false);
    }
  };

  const save = async () => {
    setSaving(true);
    setError(undefined);
    try {
      const saved = await desktopCommands.saveVllmModel(
        model?.id,
        displayName,
        config,
        apiKey || undefined,
        clearApiKey,
      );
      setApiKey("");
      setCredentialSaved(!clearApiKey && (credentialSaved || Boolean(apiKey)));
      setClearApiKey(false);
      onSaved(saved);
    } catch (saveError) {
      setError(messageFromError(saveError, text.errors.unexpected));
    } finally {
      setSaving(false);
    }
  };

  const content = (
    <form
      ref={dialogRef}
      tabIndex={dialog ? -1 : undefined}
      className={dialog ? "modal-card vllm-dialog" : "detail-section vllm-settings-form"}
      role={dialog ? "dialog" : undefined}
      aria-modal={dialog || undefined}
      aria-labelledby="vllm-settings-title"
      onClick={(event) => event.stopPropagation()}
      onSubmit={(event) => {
        event.preventDefault();
        void save();
      }}
    >
      <div>
        <span className="eyebrow">{text.vllm.eyebrow}</span>
        <h2 id="vllm-settings-title">{model ? text.vllm.editTitle : text.vllm.connectTitle}</h2>
        <p>{text.vllm.externalDescription}</p>
      </div>

      <div className="vllm-form-grid">
        <label>
          <span>{text.vllm.displayName}</span>
          <input
            value={displayName}
            placeholder={config.servedModel || text.vllm.displayNamePlaceholder}
            onChange={(event) => setDisplayName(event.target.value)}
          />
        </label>
        <label>
          <span>{text.vllm.endpoint}</span>
          <input
            value={config.endpoint}
            placeholder="http://127.0.0.1:8000"
            onChange={(event) => updateConfig({ endpoint: event.target.value })}
          />
        </label>
        <label>
          <span>{text.vllm.inferenceTask}</span>
          <select
            value={config.inferenceTask}
            onChange={(event) => updateConfig({
              inferenceTask: event.target.value as ExternalVllmConfig["inferenceTask"],
            })}
          >
            <option value="chat">{text.vllm.chatTask}</option>
            <option value="embeddings">{text.vllm.embeddingsTask}</option>
          </select>
          <small>{text.vllm.inferenceTaskHint}</small>
        </label>
        <label className="wide-setting">
          <span>{text.vllm.apiKey}</span>
          <input
            type="password"
            autoComplete="new-password"
            value={apiKey}
            placeholder={credentialSaved ? text.vllm.savedCredential : text.vllm.optionalApiKey}
            disabled={clearApiKey}
            onChange={(event) => {
              setApiKey(event.target.value);
              setClearApiKey(false);
            }}
          />
          <small>{text.vllm.credentialHint}</small>
        </label>
        {credentialSaved && (
          <label className="check-setting wide-setting">
            <input
              type="checkbox"
              checked={clearApiKey}
              onChange={(event) => {
                setClearApiKey(event.target.checked);
                if (event.target.checked) setApiKey("");
              }}
            />
            <span><strong>{text.vllm.removeSavedKey}</strong></span>
          </label>
        )}
        <label>
          <span>{text.vllm.connectionTimeout}</span>
          <input
            type="number"
            min={1}
            max={300}
            value={config.connectionTimeoutSeconds}
            onChange={(event) => updateConfig({ connectionTimeoutSeconds: Number(event.target.value) })}
          />
        </label>
        <label>
          <span>{text.vllm.requestTimeout}</span>
          <input
            type="number"
            min={1}
            max={3_600}
            value={config.requestTimeoutSeconds}
            onChange={(event) => updateConfig({ requestTimeoutSeconds: Number(event.target.value) })}
          />
        </label>
        <label className="check-setting wide-setting">
          <input
            type="checkbox"
            checked={config.verifyTls}
            onChange={(event) => updateConfig({ verifyTls: event.target.checked })}
          />
          <span><strong>{text.vllm.verifyTls}</strong><small>{text.vllm.verifyTlsHint}</small></span>
        </label>
        <label className="wide-setting">
          <span>{text.vllm.servedModel}</span>
          {report?.models.length ? (
            <select
              value={config.servedModel}
              onChange={(event) => updateConfig({ servedModel: event.target.value })}
            >
              {report.models.map((servedModel) => (
                <option key={servedModel} value={servedModel}>{servedModel}</option>
              ))}
            </select>
          ) : (
            <input
              value={config.servedModel}
              placeholder={text.vllm.testToDiscover}
              onChange={(event) => updateConfig({ servedModel: event.target.value })}
            />
          )}
          <small>{text.vllm.oneModelHint}</small>
        </label>
      </div>

      {error && <div className="inline-error" role="alert">{error}</div>}
      {report && (
        <div className={report.healthy ? "connection-report success" : "connection-report failure"} role="status">
          <strong>{report.healthy ? text.vllm.connected : text.vllm.connectionFailed}</strong>
          <span>{report.message}</span>
        </div>
      )}

      <div className="modal-actions">
        {onCancel && <button className="secondary-button" type="button" onClick={onCancel}>{text.common.cancel}</button>}
        <button className="secondary-button" type="button" disabled={testing || saving} onClick={() => void test()}>
          {testing ? text.vllm.testing : text.vllm.test}
        </button>
        <button className="primary-button" type="submit" disabled={saving || !config.servedModel.trim()}>
          {saving ? text.common.saving : model ? text.common.save : text.vllm.connect}
        </button>
      </div>
    </form>
  );

  return dialog ? <div className="modal-backdrop" onClick={onCancel}>{content}</div> : content;
}
