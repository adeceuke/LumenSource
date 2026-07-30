import { useCallback, useEffect, useState } from "react";
import { desktopCommands, messageFromError } from "../commands";
import type { InterruptedInstall, QueuedOperation, RunningModelEntry, StorageReport } from "../types";

function formatBytes(value: number): string {
  if (value <= 0) return "0 B";
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  const index = Math.min(Math.floor(Math.log(value) / Math.log(1024)), units.length - 1);
  return `${(value / (1024 ** index)).toFixed(index > 1 ? 1 : 0)} ${units[index]}`;
}

interface StoragePageProps {
  onError: (message: string) => void;
  onModelsChanged: (models: RunningModelEntry[]) => void;
}

export function StoragePage({ onError, onModelsChanged }: StoragePageProps) {
  const [report, setReport] = useState<StorageReport>();
  const [busy, setBusy] = useState<string>();
  const [notice, setNotice] = useState<string>();
  const [profiles, setProfiles] = useState("");
  const [interrupted, setInterrupted] = useState<InterruptedInstall>();
  const [queued, setQueued] = useState<QueuedOperation[]>([]);

  const refresh = useCallback(async () => {
    try {
      setReport(await desktopCommands.storageReport());
    } catch (error) {
      onError(messageFromError(error, "Could not scan storage."));
    }
  }, [onError]);

  useEffect(() => {
    void refresh();
    void desktopCommands.interruptedInstall().then((operation) => {
      setInterrupted(operation ?? undefined);
    }).catch(() => setInterrupted(undefined));
    void desktopCommands.queuedOperations().then(setQueued).catch(() => setQueued([]));
  }, [refresh]);

  const cleanup = async (entryId: string, label: string, effect: string) => {
    if (!window.confirm(`Remove ${label}?\n\nScope: ${effect}`)) return;
    setBusy(entryId);
    setNotice(undefined);
    try {
      const result = await desktopCommands.cleanupStorage(entryId, true);
      setNotice(`${result.scope}: ${formatBytes(result.removedBytes)} reclaimed. ${result.effect}`);
      await refresh();
    } catch (error) {
      onError(messageFromError(error, "Storage cleanup failed."));
    } finally {
      setBusy(undefined);
    }
  };

  const exportProfiles = async () => {
    try {
      const document = await desktopCommands.exportConnectionProfiles();
      setProfiles(document);
      await navigator.clipboard.writeText(document);
      setNotice("Connection profiles copied. Credentials are intentionally excluded.");
    } catch (error) {
      onError(messageFromError(error, "Could not export connection profiles."));
    }
  };

  const importProfiles = async () => {
    if (!profiles.trim()) return;
    try {
      const models = await desktopCommands.importConnectionProfiles(profiles);
      onModelsChanged(models);
      setNotice(`Connection profiles imported. ${models.length} model entries are now recorded. Credentials must be entered separately.`);
    } catch (error) {
      onError(messageFromError(error, "Could not import connection profiles."));
    }
  };

  const resumeInstall = async () => {
    setBusy("interrupted-install");
    try {
      await desktopCommands.resumeInterruptedInstall();
      onModelsChanged(await desktopCommands.loadModels());
      setInterrupted(undefined);
      setNotice("The interrupted installation resumed and passed its validation request.");
      await refresh();
    } catch (error) {
      onError(messageFromError(error, "Could not resume the interrupted installation."));
      onModelsChanged(await desktopCommands.loadModels().catch(() => []));
    } finally {
      setBusy(undefined);
    }
  };

  const discardInstall = async () => {
    if (!window.confirm("Discard only the recovery record? Any data already committed to Ollama or the Hugging Face cache will be retained and rediscovered.")) return;
    try {
      await desktopCommands.discardInterruptedInstall(true);
      setInterrupted(undefined);
      setNotice("Recovery record discarded. Runtime model data and caches were retained.");
    } catch (error) {
      onError(messageFromError(error, "Could not discard the recovery record."));
    }
  };

  return (
    <section className="management-page storage-page" aria-labelledby="storage-title">
      <header className="management-header">
        <div>
          <span>Storage manager</span>
          <h1 id="storage-title">Storage</h1>
          <p>See where model data is stored and what each cleanup action affects.</p>
        </div>
        <button className="secondary-button" type="button" onClick={() => void refresh()}>
          Rescan
        </button>
      </header>

      {interrupted && (
        <section className="interrupted-operation" role="status">
          <h2>Interrupted installation found</h2>
          <p>
            {interrupted.modelId} on {interrupted.targetId}, started {new Date(interrupted.startedAt).toLocaleString()}.
            Runtime downloads resume where supported; otherwise the idempotent pull restarts and reuses committed data.
          </p>
          <div className="runtime-actions">
            <button className="primary-button" type="button" disabled={busy !== undefined} onClick={() => void resumeInstall()}>Resume and validate</button>
            <button className="secondary-button" type="button" disabled={busy !== undefined} onClick={() => void discardInstall()}>Discard recovery record</button>
          </div>
        </section>
      )}
      {queued.length > 0 && (
        <section className="interrupted-operation">
          <h2>Queued model operations</h2>
          {queued.map((operation) => (
            <div key={`${operation.modelId}:${operation.action}`}>
              <p>
                <strong>{operation.action}</strong> for {operation.modelId} · queued {new Date(operation.queuedAt).toLocaleString()}
                {operation.needsRetry ? " · App restarted; retry this action from the model page." : " · Waiting for the conflicting operation to finish."}
              </p>
              {operation.needsRetry && (
                <button className="secondary-button" type="button" onClick={() => {
                  void desktopCommands.dismissQueuedOperation(operation.modelId, operation.action)
                    .then(() => setQueued((current) => current.filter((item) => item !== operation)));
                }}>Dismiss recovered queue item</button>
              )}
            </div>
          ))}
        </section>
      )}

      {!report ? <p>Scanning storage…</p> : (
        <>
          <div className="storage-summary">
            <div><span>Managed and shared storage</span><strong>{formatBytes(report.totalBytes)}</strong></div>
            <div><span>Safely reclaimable caches</span><strong>{formatBytes(report.reclaimableBytes)}</strong></div>
          </div>
          {notice && <div className="connection-report success" role="status">{notice}</div>}
          <div className="storage-list">
            {report.entries.map((entry) => (
              <article className="storage-entry" key={entry.id}>
                <div className="storage-entry-heading">
                  <div>
                    <h2>{entry.label}</h2>
                    <p>{entry.path ?? "Reported by the runtime"}</p>
                  </div>
                  <strong>{entry.exact ? "" : "≈ "}{formatBytes(entry.sizeBytes)}</strong>
                </div>
                <div className="storage-badges">
                  <span>{entry.exact ? "Exact" : "Estimate"}</span>
                  {entry.shared && <span>Shared cache</span>}
                  {entry.owners.length > 0 && <span>{entry.owners.length} model owner{entry.owners.length === 1 ? "" : "s"}</span>}
                </div>
                {entry.owners.length > 0 && <p>Used by: {entry.owners.join(", ")}</p>}
                <p>{entry.cleanupEffect}</p>
                {entry.cleanupEligible && (
                  <button
                    className="secondary-button"
                    type="button"
                    disabled={busy !== undefined}
                    onClick={() => void cleanup(entry.id, entry.label, entry.cleanupEffect)}
                  >
                    {busy === entry.id ? "Cleaning…" : "Clean this cache"}
                  </button>
                )}
              </article>
            ))}
          </div>
          <section className="connection-profile-transfer" aria-labelledby="profile-transfer-title">
            <h2 id="profile-transfer-title">Connection profiles</h2>
            <p>Export or import external vLLM and remote Ollama connection details. API keys, passwords, and tokens are never included.</p>
            <textarea
              aria-label="Connection-profile JSON"
              rows={8}
              value={profiles}
              onChange={(event) => setProfiles(event.target.value)}
              placeholder="Exported connection-profile JSON appears here, or paste a profile to import."
            />
            <div className="runtime-actions">
              <button className="secondary-button" type="button" onClick={() => void exportProfiles()}>Export and copy</button>
              <button className="secondary-button" type="button" disabled={!profiles.trim()} onClick={() => void importProfiles()}>Import profiles</button>
            </div>
          </section>
        </>
      )}
    </section>
  );
}
