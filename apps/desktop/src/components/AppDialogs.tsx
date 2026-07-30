import { useState, type Dispatch, type KeyboardEvent, type RefObject, type SetStateAction } from "react";
import { browserMessages } from "../i18n";
import type { RemoteTargetConfig, RunningModelEntry } from "../types";

const text = browserMessages();

interface TelemetryDialogProps {
  enabled?: boolean;
  saving: boolean;
  onClose: () => void;
  onSave: (enabled: boolean) => void;
}

export function TelemetryDialog({ enabled, saving, onClose, onSave }: TelemetryDialogProps) {
  return (
    <div className="modal-backdrop" onClick={() => {
      if (enabled !== undefined) onClose();
    }}>
      <div
        className="modal-card telemetry-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="telemetry-dialog-title"
        onClick={(event) => event.stopPropagation()}
      >
        <span className="eyebrow">{text.telemetry.eyebrow}</span>
        <h3 id="telemetry-dialog-title">{text.telemetry.title}</h3>
        <p>{text.telemetry.description}</p>
        <p>{text.telemetry.privacy}</p>
        <div className="modal-actions">
          <button className="secondary-button" type="button" disabled={saving} onClick={() => onSave(false)}>
            {text.telemetry.decline}
          </button>
          <button className="primary-button" type="button" disabled={saving} onClick={() => onSave(true)}>
            {saving ? text.common.saving : text.telemetry.share}
          </button>
        </div>
      </div>
    </div>
  );
}

interface RemoteTargetDialogProps {
  config: RemoteTargetConfig;
  saving: boolean;
  error?: string;
  context?: "wizard" | "machines";
  password?: string;
  setPassword?: (password: string) => void;
  rememberPassword?: boolean;
  setRememberPassword?: (remember: boolean) => void;
  setConfig: Dispatch<SetStateAction<RemoteTargetConfig>>;
  onClose: () => void;
  onSave: () => void;
}

export function RemoteTargetDialog({
  config,
  saving,
  error,
  context = "wizard",
  password = "",
  setPassword,
  rememberPassword = true,
  setRememberPassword,
  setConfig,
  onClose,
  onSave,
}: RemoteTargetDialogProps) {
  return (
    <div className="modal-backdrop" onClick={onClose}>
      <form
        className="modal-card remote-target-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="remote-target-dialog-title"
        onClick={(event) => event.stopPropagation()}
        onSubmit={(event) => {
          event.preventDefault();
          onSave();
        }}
      >
        <div className="remote-target-dialog-heading">
          <div>
            <span className="eyebrow">{context === "machines" ? text.machines.addDialogEyebrow : text.remoteDialog.eyebrow}</span>
            <h3 id="remote-target-dialog-title">{context === "machines" ? text.machines.addDialogTitle : text.wizard.location.addTarget}</h3>
          </div>
          <button className="icon-button" type="button" onClick={onClose} disabled={saving} aria-label={text.remoteDialog.closeAria}>✕</button>
        </div>
        <p>{context === "machines" ? text.machines.addDialogDescription : text.remoteDialog.description}</p>
        <div className="wizard-form remote-target-form">
          <label>
            <span>{text.remoteDialog.targetName}</span>
            <input value={config.name} maxLength={80} onChange={(event) => setConfig((current) => ({ ...current, name: event.target.value }))} placeholder={text.remoteDialog.targetPlaceholder} />
            <small>{text.remoteDialog.targetHint}</small>
          </label>
          <label>
            <span>{text.remoteDialog.host}</span>
            <input value={config.host} maxLength={255} required onChange={(event) => setConfig((current) => ({ ...current, host: event.target.value }))} placeholder={text.remoteDialog.hostPlaceholder} autoCapitalize="none" spellCheck={false} />
          </label>
          <label>
            <span>{text.remoteDialog.username}</span>
            <input value={config.username} maxLength={64} required onChange={(event) => setConfig((current) => ({ ...current, username: event.target.value }))} placeholder={text.remoteDialog.usernamePlaceholder} autoCapitalize="none" spellCheck={false} />
          </label>
          <label>
            <span>{text.remoteDialog.port}</span>
            <input type="number" min={1} max={65535} required value={config.port} onChange={(event) => setConfig((current) => ({ ...current, port: Number(event.target.value) }))} />
          </label>
          <fieldset className="remote-authentication">
            <legend>{text.remoteDialog.authentication}</legend>
            <label className={config.authentication === "key" ? "selected" : ""}>
              <input type="radio" name="new-remote-authentication" value="key" checked={config.authentication === "key"} onChange={() => setConfig((current) => ({ ...current, authentication: "key" }))} />
              <span><strong>{text.remoteDialog.keyTitle}</strong><small>{text.remoteDialog.keyDescription}</small></span>
            </label>
            <label className={config.authentication === "password" ? "selected" : ""}>
              <input type="radio" name="new-remote-authentication" value="password" checked={config.authentication === "password"} onChange={() => setConfig((current) => ({ ...current, authentication: "password", identityFile: undefined }))} />
              <span><strong>{text.remoteDialog.passwordTitle}</strong><small>{text.remoteDialog.passwordDescription}</small></span>
            </label>
          </fieldset>
          {config.authentication === "key" && (
            <label className="remote-key-field">
              <span>{text.remoteDialog.identityFile}</span>
              <input value={config.identityFile ?? ""} onChange={(event) => setConfig((current) => ({ ...current, identityFile: event.target.value || undefined }))} placeholder={text.remoteDialog.identityPlaceholder} autoCapitalize="none" spellCheck={false} />
              <small>{text.remoteDialog.identityHint}</small>
            </label>
          )}
          {config.authentication === "password" && setPassword && (
            <>
              <label className="remote-key-field">
                <span>{text.remoteDialog.passwordLabel}</span>
                <input
                  type="password"
                  value={password}
                  required
                  autoComplete="off"
                  onChange={(event) => setPassword(event.target.value)}
                />
                <small>{text.remoteDialog.passwordHint}</small>
              </label>
              {setRememberPassword && (
                <label className="remote-key-field credential-save-choice">
                  <input
                    type="checkbox"
                    checked={rememberPassword}
                    onChange={(event) => setRememberPassword(event.target.checked)}
                  />
                  <span>
                    <strong>{text.machines.savePassword}</strong>
                    <small>{text.machines.savePasswordHint}</small>
                  </span>
                </label>
              )}
            </>
          )}
        </div>
        {error && <div className="inline-error" role="alert">{error}</div>}
        <div className="modal-actions">
          <button className="secondary-button" type="button" onClick={onClose} disabled={saving}>{text.common.cancel}</button>
          <button className="primary-button" type="submit" disabled={saving || !config.host.trim() || !config.username.trim() || !Number.isInteger(config.port) || config.port < 1 || config.port > 65535 || (config.authentication === "password" && Boolean(setPassword) && !password)}>
            {saving ? text.common.saving : text.common.save}
          </button>
        </div>
      </form>
    </div>
  );
}

interface RemotePasswordDialogProps {
  model: RunningModelEntry;
  busy: boolean;
  onCancel: () => void;
  onSubmit: (password: string, save: boolean) => void;
}

export function RemotePasswordDialog({
  model,
  busy,
  onCancel,
  onSubmit,
}: RemotePasswordDialogProps) {
  const [password, setPassword] = useState("");
  const [save, setSave] = useState(true);
  const machine = model.targetName ?? text.models.remoteLinux;

  return (
    <div className="modal-backdrop" onClick={() => {
      if (!busy) onCancel();
    }}>
      <form
        className="modal-card remote-password-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="remote-password-dialog-title"
        onClick={(event) => event.stopPropagation()}
        onSubmit={(event) => {
          event.preventDefault();
          onSubmit(password, save);
        }}
      >
        <h3 id="remote-password-dialog-title">
          {text.dialogs.remotePasswordTitle(machine)}
        </h3>
        <p>{text.dialogs.remotePasswordDescription}</p>
        <label>
          <span>{text.dialogs.remotePasswordLabel}</span>
          <input
            type="password"
            value={password}
            required
            autoComplete="off"
            onChange={(event) => setPassword(event.target.value)}
          />
        </label>
        <label className="credential-save-choice">
          <input
            type="checkbox"
            checked={save}
            onChange={(event) => setSave(event.target.checked)}
          />
          <span>
            <strong>{text.dialogs.remotePasswordSave}</strong>
            <small>{text.dialogs.remotePasswordSaveHint}</small>
          </span>
        </label>
        <div className="modal-actions">
          <button className="secondary-button" type="button" onClick={onCancel} disabled={busy}>
            {text.common.cancel}
          </button>
          <button className="primary-button" type="submit" disabled={busy || !password}>
            {busy ? text.common.saving : text.dialogs.remotePasswordContinue}
          </button>
        </div>
      </form>
    </div>
  );
}

interface RenameDialogProps {
  inputRef: RefObject<HTMLInputElement | null>;
  value: string;
  onChange: (value: string) => void;
  onKeyDown: (event: KeyboardEvent<HTMLInputElement>) => void;
  onCancel: () => void;
  onSave: () => void;
}

export function RenameDialog({ inputRef, value, onChange, onKeyDown, onCancel, onSave }: RenameDialogProps) {
  return (
    <div className="modal-backdrop" onClick={onCancel}>
      <form className="modal-card" onClick={(event) => event.stopPropagation()} onSubmit={(event) => {
        event.preventDefault();
        onSave();
      }}>
        <h3>{text.dialogs.renameTitle}</h3>
        <input ref={inputRef} value={value} onChange={(event) => onChange(event.target.value)} onKeyDown={onKeyDown} />
        <div className="modal-actions">
          <button className="secondary-button" type="button" onClick={onCancel}>{text.common.cancel}</button>
          <button className="primary-button" type="submit">{text.common.save}</button>
        </div>
      </form>
    </div>
  );
}

interface RemoveDialogProps {
  model?: RunningModelEntry;
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

export function RemoveDialog({ model, busy, onCancel, onConfirm }: RemoveDialogProps) {
  const external = model?.runtimeCapabilities.lifecycle === "external";
  const managedVllm = model?.runtimeId === "vllm"
    && model.runtimeCapabilities.lifecycle === "managed";
  const title = external
    ? text.dialogs.removeExternalTitle
    : managedVllm
      ? text.dialogs.removeManagedVllmTitle
      : text.dialogs.removeTitle;
  const description = external
    ? text.dialogs.removeExternalDescription
    : managedVllm
      ? text.dialogs.removeManagedVllmDescription
      : text.dialogs.removeDescription;
  return (
    <div className="modal-backdrop" onClick={() => {
      if (!busy) onCancel();
    }}>
      <div className="modal-card" onClick={(event) => event.stopPropagation()}>
        <h3>{title}</h3>
        <p>{description}</p>
        <div className="modal-actions">
          <button className="secondary-button" type="button" disabled={busy} onClick={onCancel}>{text.common.cancel}</button>
          <button className="primary-button" type="button" disabled={busy} onClick={onConfirm}>{busy ? text.dialogs.removing : external ? text.dialogs.removeConnection : text.dialogs.removeModel}</button>
        </div>
      </div>
    </div>
  );
}

export function LeaveWizardDialog({ onStay, onLeave }: { onStay: () => void; onLeave: () => void }) {
  return (
    <div className="modal-backdrop" onClick={onStay}>
      <div className="modal-card" onClick={(event) => event.stopPropagation()}>
        <h3>{text.dialogs.leaveTitle}</h3>
        <p>{text.dialogs.leaveDescription}</p>
        <div className="modal-actions">
          <button className="secondary-button" type="button" onClick={onStay}>{text.dialogs.stay}</button>
          <button className="primary-button" type="button" onClick={onLeave}>{text.dialogs.leave}</button>
        </div>
      </div>
    </div>
  );
}
