import { useCallback, useEffect, useRef, useState, type KeyboardEvent } from "react";
import appIcon from "../../../icon.png";
import {
  LeaveWizardDialog,
  RemotePasswordDialog,
  RemoteTargetDialog,
  RemoveDialog,
  RenameDialog,
  TelemetryDialog,
} from "./components/AppDialogs";
import { ModelDetails, type DetailTab } from "./components/ModelDetails";
import { ModelSetupWizard } from "./components/ModelSetupWizard";
import { MachinesPage } from "./components/MachinesPage";
import { ModelsList } from "./components/ModelsList";
import { desktopCommands, messageFromError } from "./commands";
import { useModels } from "./hooks/useModels";
import { useModelWizard } from "./hooks/useModelWizard";
import { browserMessages } from "./i18n";
import type { CatalogSummary, RunningModelEntry } from "./types";

const text = browserMessages();

function localizedError(error: unknown): string {
  return messageFromError(error, text.errors.unexpected);
}

function App() {
  const [section, setSection] = useState<"models" | "machines">("models");
  const [catalog, setCatalog] = useState<CatalogSummary>();
  const [error, setError] = useState<string>();
  const reportError = useCallback(
    (loadError: unknown) => setError(localizedError(loadError)),
    [],
  );
  const {
    models: runningModels,
    setModels: setRunningModels,
    action: modelAction,
    remotePasswordRequest,
    credentialBusy,
    toggleRunning,
    submitRemotePassword,
    dismissRemotePassword,
    remove: removeModel,
    rename: renameModel,
    clearLogs: clearModelLogs,
  } = useModels(reportError);
  const [menuOpenId, setMenuOpenId] = useState<string>();
  const [menuPosition, setMenuPosition] = useState<{ top: number; left: number } | null>(null);
  const [detailModelId, setDetailModelId] = useState<string>();
  const [detailTab, setDetailTab] = useState<DetailTab>("logs");
  const [renameId, setRenameId] = useState<string>();
  const [renameValue, setRenameValue] = useState("");
  const [pendingRemovalId, setPendingRemovalId] = useState<string>();
  const [removalBusy, setRemovalBusy] = useState(false);
  const [copiedField, setCopiedField] = useState<string>();
  const [telemetryEnabled, setTelemetryEnabled] = useState<boolean>();
  const [telemetryDialogOpen, setTelemetryDialogOpen] = useState(false);
  const [telemetrySaving, setTelemetrySaving] = useState(false);
  const renameInputRef = useRef<HTMLInputElement>(null);
  const contentPanelRef = useRef<HTMLDivElement>(null);
  const copyFeedbackTimerRef = useRef<number | undefined>(undefined);
  const detailModel = runningModels.find((model) => model.id === detailModelId);

  const copyText = useCallback(async (value: string, feedbackKey: string) => {
    try {
      await navigator.clipboard.writeText(value);
      setCopiedField(feedbackKey);
      if (copyFeedbackTimerRef.current !== undefined) {
        window.clearTimeout(copyFeedbackTimerRef.current);
      }
      copyFeedbackTimerRef.current = window.setTimeout(() => {
        setCopiedField((current) => current === feedbackKey ? undefined : current);
        copyFeedbackTimerRef.current = undefined;
      }, 1_800);
    } catch (copyError: unknown) {
      setError(localizedError(copyError));
    }
  }, []);

  const wizard = useModelWizard({
    catalog,
    setModels: setRunningModels,
    setError,
    onOpen: () => setDetailModelId(undefined),
    copiedField,
    copyText,
  });

  useEffect(() => {
    void desktopCommands.refreshCatalog().then(setCatalog).catch((catalogError: unknown) => {
      setError(localizedError(catalogError));
    });

    void desktopCommands.telemetryPreference().then((preference) => {
      setTelemetryEnabled(preference ?? false);
      if (preference === null) setTelemetryDialogOpen(true);
    }).catch(() => {
      // Telemetry is optional. A local preference read failure must not affect
      // the application or enable collection.
      setTelemetryEnabled(false);
    });
  }, []);

  useEffect(() => {
    if (renameId) {
      window.requestAnimationFrame(() => renameInputRef.current?.focus());
    }
  }, [renameId]);

  useEffect(() => () => {
    if (copyFeedbackTimerRef.current !== undefined) {
      window.clearTimeout(copyFeedbackTimerRef.current);
    }
  }, []);

  useEffect(() => {
    if (!detailModelId) return;
    const frame = window.requestAnimationFrame(() => {
      contentPanelRef.current?.scrollTo({ top: 0, left: 0 });
    });
    return () => window.cancelAnimationFrame(frame);
  }, [detailModelId, detailTab]);

  useEffect(() => {
    if (!menuOpenId) return;

    const dismissMenu = (event: PointerEvent) => {
      const target = event.target;
      if (target instanceof Element && target.closest("[data-model-menu]")) return;
      setMenuOpenId(undefined);
      setMenuPosition(null);
    };
    const dismissMenuWithKeyboard = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "Escape") return;
      setMenuOpenId(undefined);
      setMenuPosition(null);
    };

    document.addEventListener("pointerdown", dismissMenu, true);
    document.addEventListener("keydown", dismissMenuWithKeyboard);
    return () => {
      document.removeEventListener("pointerdown", dismissMenu, true);
      document.removeEventListener("keydown", dismissMenuWithKeyboard);
    };
  }, [menuOpenId]);

  const openRename = (model: RunningModelEntry) => {
    setRenameId(model.id);
    setRenameValue(model.name);
    setMenuOpenId(undefined);
    setMenuPosition(null);
  };

  const saveRename = () => {
    if (!renameId) return;
    renameModel(renameId, renameValue);
    setRenameId(undefined);
    setRenameValue("");
  };

  const handleRenameKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter") {
      event.preventDefault();
      saveRename();
    }
  };

  const requestRemoval = (id: string) => {
    setPendingRemovalId(id);
    setMenuOpenId(undefined);
    setMenuPosition(null);
  };

  const openDetail = (model: RunningModelEntry, tab: DetailTab) => {
    setDetailModelId(model.id);
    setDetailTab(tab);
    setMenuOpenId(undefined);
    setMenuPosition(null);
  };

  const clearLogs = () => {
    if (detailModelId) clearModelLogs(detailModelId);
  };

  const toggleMenu = (id: string, trigger: HTMLButtonElement) => {
    if (menuOpenId === id) {
      setMenuOpenId(undefined);
      setMenuPosition(null);
      return;
    }

    const rect = trigger.getBoundingClientRect();
    const menuHeight = 210;
    const top = rect.bottom + 8 + menuHeight <= window.innerHeight
      ? rect.bottom + 8
      : Math.max(8, rect.top - menuHeight - 8);
    setMenuOpenId(id);
    setMenuPosition({
      top,
      left: Math.max(8, Math.min(Math.max(12, rect.right - 160), window.innerWidth - 152)),
    });
  };

  const confirmRemoval = async () => {
    if (!pendingRemovalId) return;
    const modelId = pendingRemovalId;
    setRemovalBusy(true);
    try {
      await removeModel(modelId);
      setPendingRemovalId(undefined);
    } catch (removalError) {
      setError(localizedError(removalError));
    } finally {
      setRemovalBusy(false);
    }
  };

  const saveTelemetryPreference = async (enabled: boolean) => {
    setTelemetrySaving(true);
    try {
      await desktopCommands.setTelemetryEnabled(enabled);
      setTelemetryEnabled(enabled);
      setTelemetryDialogOpen(false);
    } catch {
      // Collection stays disabled unless the preference was persisted.
      setTelemetryEnabled(false);
      setTelemetryDialogOpen(false);
    } finally {
      setTelemetrySaving(false);
    }
  };

  return (
    <div className="app-shell">
      <header className="app-header">
        <a className="brand" href="#" aria-label={text.brand.homeLabel}>
          <img className="brand-mark" src={appIcon} alt="" />
          <span>{text.brand.name}</span>
        </a>
        <span className="local-badge"><i /> {text.brand.privacyBadge}</span>
      </header>

      <main className="main-panel" aria-busy={wizard.busy || modelAction !== undefined}>
        {error && (
          <div className="error-banner" role="alert">
            <span>!</span>
            <div><strong>{text.errors.title}</strong><p>{error}</p></div>
            <button onClick={() => setError(undefined)} aria-label={text.errors.dismiss}>×</button>
          </div>
        )}

        <section className={`models-page ${wizard.open ? "wizard-open" : ""}`}>
          <aside className="sidebar">
            <nav className="sidebar-navigation" aria-label={text.navigation.label}>
              <button
                type="button"
                className={section === "models" ? "active" : ""}
                onClick={() => setSection("models")}
                disabled={wizard.open}
              >
                <span aria-hidden="true">◫</span>
                {text.navigation.models}
              </button>
              <button
                type="button"
                className={section === "machines" ? "active" : ""}
                onClick={() => setSection("machines")}
                disabled={wizard.open}
              >
                <span aria-hidden="true">⌁</span>
                {text.navigation.machines}
              </button>
            </nav>
          </aside>

          <div ref={contentPanelRef} className={`content-panel ${detailModel ? "detail-open" : ""}`}>
            {wizard.open ? (
              <ModelSetupWizard {...wizard.props} />
            ) : section === "machines" ? (
              <MachinesPage />
            ) : detailModel ? (
              <ModelDetails
                model={detailModel}
                tab={detailTab}
                modelAction={modelAction}
                copiedField={copiedField}
                errorFrom={localizedError}
                onTab={setDetailTab}
                onBack={() => setDetailModelId(undefined)}
                onToggleRunning={() => void toggleRunning(detailModel.id)}
                onClearLogs={clearLogs}
                onCopy={(value, key) => void copyText(value, key)}
              />
            ) : (
              <ModelsList
                models={runningModels}
                modelAction={modelAction}
                menuOpenId={menuOpenId}
                menuPosition={menuPosition}
                onAdd={wizard.openWizard}
                onOpen={(model) => openDetail(model, "logs")}
                onToggleRunning={(id) => void toggleRunning(id)}
                onToggleMenu={toggleMenu}
                onRename={openRename}
                onPerformance={(model) => openDetail(model, "performance")}
                onApi={(model) => openDetail(model, "api")}
                onRemove={requestRemoval}
              />
            )}
          </div>
        </section>
      </main>

      <footer className="app-footer">
        <button
          className="telemetry-settings-button"
          type="button"
          onClick={() => setTelemetryDialogOpen(true)}
        >
          {text.footer.usageStatistics(Boolean(telemetryEnabled))}
        </button>
        {catalog && (
          <div
            className={`catalog-status ${catalog.source}`}
            role="status"
            title={text.footer.catalogSourceDetails[catalog.source]}
          >
            <i aria-hidden="true" />
            <strong>{text.footer.catalogSourceLabels[catalog.source]}</strong>
            <span>{text.footer.catalog(catalog.revision, catalog.modelCount)}</span>
          </div>
        )}
      </footer>

      {telemetryDialogOpen && (
        <TelemetryDialog
          enabled={telemetryEnabled}
          saving={telemetrySaving}
          onClose={() => setTelemetryDialogOpen(false)}
          onSave={(enabled) => void saveTelemetryPreference(enabled)}
        />
      )}

      {wizard.remoteTargetDialog.open && (
        <RemoteTargetDialog
          config={wizard.remoteTargetDialog.config}
          saving={wizard.remoteTargetDialog.saving}
          error={wizard.remoteTargetDialog.error}
          setConfig={wizard.remoteTargetDialog.setConfig}
          onClose={wizard.remoteTargetDialog.close}
          onSave={() => void wizard.remoteTargetDialog.save()}
        />
      )}

      {remotePasswordRequest && (
        <RemotePasswordDialog
          model={remotePasswordRequest}
          busy={credentialBusy}
          onCancel={dismissRemotePassword}
          onSubmit={(password, save) => void submitRemotePassword(password, save)}
        />
      )}

      {renameId && (
        <RenameDialog
          inputRef={renameInputRef}
          value={renameValue}
          onChange={setRenameValue}
          onKeyDown={handleRenameKeyDown}
          onCancel={() => setRenameId(undefined)}
          onSave={saveRename}
        />
      )}

      {pendingRemovalId && (
        <RemoveDialog
          busy={removalBusy}
          onCancel={() => setPendingRemovalId(undefined)}
          onConfirm={() => void confirmRemoval()}
        />
      )}

      {wizard.closeConfirmationOpen && (
        <LeaveWizardDialog
          onStay={wizard.dismissCloseConfirmation}
          onLeave={wizard.confirmClose}
        />
      )}
    </div>
  );
}

export default App;
