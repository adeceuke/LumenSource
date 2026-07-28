import { useEffect, useState } from "react";
import { desktopCommands, isRemotePasswordRequiredMessage } from "../commands";
import { browserMessages } from "../i18n";
import { useMachineMonitor } from "../hooks/useMachineMonitor";
import { formatBytes } from "../modelUi";
import type { RemoteTargetConfig } from "../types";
import { MachineUsageChart } from "./MachineUsageChart";

const text = browserMessages();

export interface MachineDefinition {
  targetId: string;
  name: string;
  config?: RemoteTargetConfig;
}

interface MachineDetailsProps {
  machine: MachineDefinition;
  password?: string;
  onPassword: (password: string, save: boolean) => Promise<void>;
  onForgetPassword: () => Promise<void>;
  onBack: () => void;
  errorFrom: (error: unknown) => string;
}

export function MachineDetails({
  machine,
  password,
  onPassword,
  onForgetPassword,
  onBack,
  errorFrom,
}: MachineDetailsProps) {
  const [tab, setTab] = useState<"hardware" | "usage">("hardware");
  const [passwordDraft, setPasswordDraft] = useState("");
  const [rememberPassword, setRememberPassword] = useState(true);
  const [credentialSaved, setCredentialSaved] = useState(false);
  const [credentialLoading, setCredentialLoading] = useState(false);
  const [credentialBusy, setCredentialBusy] = useState(false);
  const [credentialError, setCredentialError] = useState<string>();
  const passwordRequired = machine.config?.authentication === "password";
  const passwordMissing = passwordRequired && !password && !credentialSaved;
  const monitor = useMachineMonitor(
    machine.targetId,
    password,
    passwordMissing,
    errorFrom,
  );
  const credentialRejected = [monitor.hardwareError, monitor.usageError]
    .some((message) => message !== undefined && isRemotePasswordRequiredMessage(message));
  const showPasswordForm = passwordRequired && (passwordMissing || credentialRejected);

  useEffect(() => {
    if (!passwordRequired) return;
    let disposed = false;
    setCredentialLoading(true);
    void desktopCommands.remoteCredentialStatus(machine.targetId)
      .then((status) => {
        if (!disposed) {
          setCredentialSaved(status.passwordSaved);
          setCredentialError(undefined);
        }
      })
      .catch((failure: unknown) => {
        if (!disposed) {
          setCredentialSaved(false);
          setCredentialError(errorFrom(failure));
        }
      })
      .finally(() => {
        if (!disposed) setCredentialLoading(false);
      });
    return () => {
      disposed = true;
    };
  }, [errorFrom, machine.targetId, passwordRequired]);

  return (
    <article className="model-detail-page machine-detail-page" aria-label={text.machines.detailsLabel}>
      <div className="models-toolbar detail-toolbar">
        <div>
          <button className="back-to-models" type="button" onClick={onBack}>
            {text.machines.back}
          </button>
          <div className="model-title-line">
            <h1>{machine.name}</h1>
            <span className="runtime-state running">
              <i /> {machine.targetId === "local" ? text.models.thisMachine : text.models.remoteLinux}
            </span>
          </div>
          {machine.config && (
            <p className="machine-address">
              {text.machines.remoteDescription(
                machine.config.username,
                machine.config.host,
                machine.config.port,
              )}
            </p>
          )}
        </div>
      </div>

      <nav className="detail-tabs" aria-label={text.machines.tabsLabel}>
        <button
          type="button"
          className={tab === "hardware" ? "active" : ""}
          onClick={() => setTab("hardware")}
        >
          {text.machines.hardwareTab}
        </button>
        <button
          type="button"
          className={tab === "usage" ? "active" : ""}
          onClick={() => setTab("usage")}
        >
          {text.machines.liveUsageTab}
        </button>
      </nav>

      {credentialError && <div className="inline-error" role="alert">{credentialError}</div>}

      {credentialSaved && !credentialRejected && (
        <div className="machine-credential-status">
          <span><i /> {text.machines.passwordSaved}</span>
          <button
            className="text-button"
            type="button"
            disabled={credentialBusy}
            onClick={() => {
              setCredentialBusy(true);
              setCredentialError(undefined);
              void onForgetPassword()
                .then(() => setCredentialSaved(false))
                .catch((failure: unknown) => setCredentialError(errorFrom(failure)))
                .finally(() => setCredentialBusy(false));
            }}
          >
            {text.machines.forgetPassword}
          </button>
        </div>
      )}

      {showPasswordForm && (
        <form
          className="machine-password-card"
          onSubmit={(event) => {
            event.preventDefault();
            setCredentialBusy(true);
            setCredentialError(undefined);
            void (async () => {
              if (!rememberPassword && credentialSaved && credentialRejected) {
                await onForgetPassword();
                setCredentialSaved(false);
              }
              await onPassword(passwordDraft, rememberPassword);
            })()
              .then(() => {
                if (rememberPassword) setCredentialSaved(true);
                setPasswordDraft("");
              })
              .catch((failure: unknown) => setCredentialError(errorFrom(failure)))
              .finally(() => setCredentialBusy(false));
          }}
        >
          <div>
            <strong>{text.machines.passwordTitle}</strong>
            <p>{text.machines.passwordDescription}</p>
          </div>
          <label>
            <span>{text.machines.passwordLabel}</span>
            <input
              type="password"
              value={passwordDraft}
              required
              autoComplete="off"
              onChange={(event) => setPasswordDraft(event.target.value)}
            />
          </label>
          <label className="credential-save-choice machine-credential-choice">
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
          <button
            className="secondary-button"
            type="submit"
            disabled={credentialBusy || credentialLoading || !passwordDraft}
          >
            {text.machines.connect}
          </button>
        </form>
      )}

      {tab === "hardware" ? (
        <HardwareTab monitor={monitor} passwordMissing={passwordMissing} />
      ) : (
        <LiveUsageTab monitor={monitor} passwordMissing={passwordMissing} />
      )}
    </article>
  );
}

type MachineMonitor = ReturnType<typeof useMachineMonitor>;

function HardwareTab({
  monitor,
  passwordMissing,
}: {
  monitor: MachineMonitor;
  passwordMissing: boolean;
}) {
  return (
    <section className="detail-section" aria-labelledby="machine-hardware-title">
      <div className="detail-section-header">
        <div>
          <h2 id="machine-hardware-title">{text.machines.hardwareTitle}</h2>
          <p>{text.machines.hardwareDescription}</p>
        </div>
      </div>
      {monitor.hardwareError && (
        <div className="inline-error" role="alert">{monitor.hardwareError}</div>
      )}
      {passwordMissing ? (
        <div className="details-empty full-detail-empty">
          {text.machines.passwordDescription}
        </div>
      ) : monitor.hardwareLoading && !monitor.hardware ? (
        <div className="details-empty full-detail-empty">
          {text.machines.detectingHardware}
        </div>
      ) : monitor.hardware ? (
        <div className="hardware-grid machine-hardware-grid">
          <div className="hardware-card">
            <h3>{text.wizard.hardware.cpu}</h3>
            <ul>
              <li><span>{text.common.model}</span><strong>{monitor.hardware.cpu}</strong></li>
              <li><span>{text.wizard.hardware.logicalCores}</span><strong>{monitor.hardware.cpuCores}</strong></li>
              <li>
                <span>{text.wizard.hardware.frequency}</span>
                <strong>
                  {monitor.hardware.cpuFrequencyMhz
                    ? text.format.megahertz(monitor.hardware.cpuFrequencyMhz)
                    : text.common.unavailable}
                </strong>
              </li>
            </ul>
          </div>
          <div className="hardware-card">
            <h3>{text.wizard.hardware.memory}</h3>
            <ul>
              <li><span>{text.wizard.hardware.capacity}</span><strong>{formatBytes(monitor.hardware.memoryBytes)}</strong></li>
              <li><span>{text.wizard.hardware.type}</span><strong>{monitor.hardware.memoryKind ?? text.common.unavailable}</strong></li>
              <li>
                <span>{text.wizard.hardware.speed}</span>
                <strong>
                  {monitor.hardware.memorySpeedMts
                    ? text.format.transfersPerSecond(monitor.hardware.memorySpeedMts)
                    : text.common.unavailable}
                </strong>
              </li>
            </ul>
          </div>
          <div className="hardware-card">
            <h3>{text.wizard.hardware.gpu}</h3>
            {monitor.hardware.gpu ? (
              <ul>
                <li><span>{text.common.model}</span><strong>{monitor.hardware.gpu.name}</strong></li>
                <li>
                  <span>{text.wizard.hardware.vram}</span>
                  <strong>
                    {monitor.hardware.gpu.memoryBytes
                      ? formatBytes(monitor.hardware.gpu.memoryBytes)
                      : text.common.unavailable}
                  </strong>
                </li>
                <li><span>{text.wizard.hardware.backend}</span><strong>{monitor.hardware.gpu.backend}</strong></li>
              </ul>
            ) : (
              <p>{text.wizard.hardware.noGpu}</p>
            )}
          </div>
          <div className="hardware-card">
            <h3>{text.wizard.hardware.storage}</h3>
            <ul>
              <li><span>{text.wizard.hardware.mount}</span><strong>{monitor.hardware.storage.mountPoint}</strong></li>
              <li><span>{text.wizard.hardware.freeSpace}</span><strong>{formatBytes(monitor.hardware.storage.availableBytes)}</strong></li>
              <li><span>{text.wizard.hardware.capacity}</span><strong>{formatBytes(monitor.hardware.storage.totalBytes)}</strong></li>
            </ul>
          </div>
        </div>
      ) : (
        <div className="details-empty full-detail-empty">
          <button className="secondary-button" type="button" onClick={monitor.retryHardware}>
            {text.machines.retryHardware}
          </button>
        </div>
      )}
    </section>
  );
}

function LiveUsageTab({
  monitor,
  passwordMissing,
}: {
  monitor: MachineMonitor;
  passwordMissing: boolean;
}) {
  const snapshot = monitor.snapshot;
  const totalMemory = snapshot
    ? snapshot.usedMemoryBytes + snapshot.availableMemoryBytes
    : 0;
  const accelerator = snapshot?.accelerators[0];
  const acceleratorUtilization = accelerator?.utilizationPercent ?? undefined;
  const usedVramBytes = accelerator?.usedVramBytes ?? undefined;

  return (
    <section className="detail-section" aria-labelledby="machine-live-usage-title">
      <div className="detail-section-header">
        <div>
          <h2 id="machine-live-usage-title">{text.machines.liveTitle}</h2>
          <p>{text.machines.liveDescription}</p>
        </div>
      </div>
      {monitor.usageError && <div className="inline-error" role="alert">{monitor.usageError}</div>}
      {passwordMissing || (monitor.usageLoading && !snapshot) ? (
        <div className="details-empty full-detail-empty">{passwordMissing
          ? text.machines.passwordDescription
          : text.machines.sampling}</div>
      ) : snapshot ? (
        <>
          <MachineUsageChart history={monitor.history} />
          <div className="performance-status detail-performance-status">
            <span className="led on" />
            <strong>{text.machines.liveUsageTab}</strong>
            <small>
              {text.machines.sampledAt(
                new Date(snapshot.sampledAtUnixMs).toLocaleTimeString(text.locale),
              )}
            </small>
          </div>
          <div className="performance-grid machine-usage-grid">
            <MetricCard
              label={text.machines.cpuUsage}
              value={text.machines.percent(snapshot.cpuUtilizationPercent)}
              percentage={snapshot.cpuUtilizationPercent}
            />
            <MetricCard
              label={text.machines.memoryUsage}
              value={text.machines.percent(
                totalMemory > 0 ? (snapshot.usedMemoryBytes / totalMemory) * 100 : 0,
              )}
              hint={text.machines.memoryUsed(
                formatBytes(snapshot.usedMemoryBytes),
                formatBytes(totalMemory),
              )}
              percentage={totalMemory > 0 ? (snapshot.usedMemoryBytes / totalMemory) * 100 : 0}
            />
            <MetricCard
              label={text.machines.gpuUsage}
              value={acceleratorUtilization === undefined
                ? text.common.unavailable
                : text.machines.percent(acceleratorUtilization)}
              hint={usedVramBytes === undefined
                ? text.machines.gpuNotReported
                : text.machines.vramUsed(formatBytes(usedVramBytes))}
              percentage={acceleratorUtilization}
            />
          </div>
        </>
      ) : (
        <div className="details-empty full-detail-empty">{text.machines.sampling}</div>
      )}
    </section>
  );
}

function MetricCard({
  label,
  value,
  hint,
  percentage,
}: {
  label: string;
  value: string;
  hint?: string;
  percentage?: number;
}) {
  return (
    <div className="performance-card">
      <span>{label}</span>
      <strong>{value}</strong>
      {hint && <small>{hint}</small>}
      {percentage !== undefined && (
        <div className="metric-track">
          <i style={{ width: `${Math.max(0, Math.min(100, percentage))}%` }} />
        </div>
      )}
    </div>
  );
}
