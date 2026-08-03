import { useCallback, useEffect, useRef, useState, type KeyboardEvent } from "react";
import appIcon from "../../../icon.png";
import { APP_VERSION } from "./appVersion";
import {
  LeaveWizardDialog,
  RemotePasswordDialog,
  RemoteTargetDialog,
  RemoveDialog,
  RenameDialog,
} from "./components/AppDialogs";
import { ModelDetails, type DetailTab } from "./components/ModelDetails";
import { ModelSetupWizard } from "./components/ModelSetupWizard";
import { MachinesPage } from "./components/MachinesPage";
import { LumenChatWorkspace } from "./components/LumenChatWorkspace";
import { ModelsList } from "./components/ModelsList";
import { SettingsPage } from "./components/SettingsPage";
import { StoragePage } from "./components/StoragePage";
import { VllmModelSettings } from "./components/VllmModelSettings";
import { desktopCommands, messageFromError } from "./commands";
import { useModels } from "./hooks/useModels";
import { useModelWizard } from "./hooks/useModelWizard";
import { browserMessages } from "./i18n";
import { lifecycleLog } from "./modelUi";
import type { ApplicationSettings, CatalogSummary, RunningModelEntry } from "./types";

const text = browserMessages();

function localizedError(error: unknown): string {
  return messageFromError(error, text.errors.unexpected);
}

function App() {
  const [section, setSection] = useState<"models" | "lumenChat" | "machines" | "storage" | "settings">("models");
  const [settings, setSettings] = useState<ApplicationSettings>();
  const [settingsDirty, setSettingsDirty] = useState(false);
  const [vllmDialogOpen, setVllmDialogOpen] = useState(false);
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
  const [detailTab, setDetailTab] = useState<DetailTab>("chat");
  const [renameId, setRenameId] = useState<string>();
  const [renameValue, setRenameValue] = useState("");
  const [pendingRemovalId, setPendingRemovalId] = useState<string>();
  const [removalBusy, setRemovalBusy] = useState(false);
  const [copiedField, setCopiedField] = useState<string>();
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

  const appendModelLog = useCallback((modelId: string, message: string) => {
    setRunningModels((current) => current.map((model) => model.id === modelId
      ? { ...model, logs: [...model.logs, lifecycleLog(message)] }
      : model));
  }, [setRunningModels]);

  const wizard = useModelWizard({
    catalog,
    setModels: setRunningModels,
    setError,
    onOpen: () => setDetailModelId(undefined),
    onOpenInstalledModel: () => {
      setSection("models");
      setDetailModelId(undefined);
    },
    copiedField,
    copyText,
    defaultStartAfterInstall: settings?.startAfterInstall ?? true,
    defaultTargetId: settings?.defaultTargetId ?? "local",
    defaultUseIntent: settings?.defaultUseIntent ?? "chat",
    defaultPerformanceProfile: settings?.defaultPerformanceProfile ?? "balanced",
  });

  useEffect(() => {
    void desktopCommands.loadSettings().then(async (loadedSettings) => {
      setSettings(loadedSettings);
      setCatalog(await desktopCommands.refreshCatalog());
    }).catch((loadError: unknown) => {
      setError(localizedError(loadError));
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
    setMenuOpenId(undefined);
    setMenuPosition(null);
    if (settings?.privacy.confirmModelDeletion ?? true) {
      setPendingRemovalId(id);
    } else {
      void performRemoval(id);
    }
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

  const performRemoval = async (modelId: string) => {
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

  const confirmRemoval = async () => {
    if (pendingRemovalId) await performRemoval(pendingRemovalId);
  };

  const navigate = (next: "models" | "lumenChat" | "machines" | "storage" | "settings") => {
    if (section === "settings" && settingsDirty && next !== "settings"
      && !window.confirm(text.settings.unsavedConfirmation)) return;
    setSection(next);
    setDetailModelId(undefined);
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
                onClick={() => navigate("models")}
                disabled={wizard.open}
              >
                <span aria-hidden="true">
                  <svg viewBox="0 0 24 24">
                    <rect x="4" y="5" width="16" height="14" rx="2" />
                    <path d="M8 9h8M8 13h8M8 17h5" />
                  </svg>
                </span>
                {text.navigation.models}
              </button>
              <button
                type="button"
                className={section === "lumenChat" ? "active" : ""}
                onClick={() => navigate("lumenChat")}
                disabled={wizard.open}
              >
                <span aria-hidden="true">
                  <svg viewBox="0 0 24 24">
                    <path d="M4 5h16v11H8l-4 4V5Z" />
                    <path d="M8 9h8M8 12h5" />
                  </svg>
                </span>
                {text.navigation.lumenChat}
              </button>
              <button
                type="button"
                className={section === "machines" ? "active" : ""}
                onClick={() => navigate("machines")}
                disabled={wizard.open}
              >
                <span aria-hidden="true">
                  <svg viewBox="0 0 24 24">
                    <rect x="3" y="4" width="18" height="13" rx="2" />
                    <path d="M8 21h8M12 17v4" />
                  </svg>
                </span>
                {text.navigation.machines}
              </button>
              <button
                type="button"
                className={section === "storage" ? "active" : ""}
                onClick={() => navigate("storage")}
                disabled={wizard.open}
              >
                <span aria-hidden="true">
                  <svg viewBox="0 0 24 24">
                    <ellipse cx="12" cy="6" rx="8" ry="3" />
                    <path d="M4 6v6c0 1.7 3.6 3 8 3s8-1.3 8-3V6M4 12v6c0 1.7 3.6 3 8 3s8-1.3 8-3v-6" />
                  </svg>
                </span>
                {text.navigation.storage}
              </button>
              <button
                type="button"
                className={section === "settings" ? "active" : ""}
                onClick={() => navigate("settings")}
                disabled={wizard.open}
              >
                <span aria-hidden="true">
                  <svg viewBox="0 0 24 24">
                    <path d="M4 7h10M18 7h2M4 17h2M10 17h10M14 4v6M6 14v6" />
                  </svg>
                </span>
                {text.navigation.settings}
              </button>
            </nav>
          </aside>

          <div
            ref={contentPanelRef}
            className={`content-panel ${detailModel ? "detail-open" : ""} ${section === "lumenChat" || (detailModel && detailTab === "chat") ? "chat-open" : ""}`}
          >
            {wizard.open ? (
              <ModelSetupWizard {...wizard.props} />
            ) : section === "settings" && settings ? (
              <SettingsPage
                settings={settings}
                onSaved={setSettings}
                onDirtyChange={setSettingsDirty}
                onError={setError}
              />
            ) : section === "lumenChat" ? (
              <LumenChatWorkspace
                models={runningModels}
                settings={settings}
                copiedField={copiedField}
                errorFrom={localizedError}
                onCopy={(value, key) => void copyText(value, key)}
                onModelLog={appendModelLog}
                onSettingsChanged={setSettings}
                onStartModel={(modelId) => void toggleRunning(modelId)}
              />
            ) : section === "machines" ? (
              <MachinesPage />
            ) : section === "storage" ? (
              <StoragePage onError={setError} onModelsChanged={setRunningModels} />
            ) : detailModel ? (
              <ModelDetails
                model={detailModel}
                models={runningModels}
                applicationSettings={settings}
                tab={detailTab}
                modelAction={modelAction}
                copiedField={copiedField}
                errorFrom={localizedError}
                onTab={setDetailTab}
                onBack={() => setDetailModelId(undefined)}
                onToggleRunning={() => void toggleRunning(detailModel.id)}
                onClearLogs={clearLogs}
                onCopy={(value, key) => void copyText(value, key)}
                onModelLog={appendModelLog}
                onSettingsChanged={setSettings}
                onStartModel={(modelId) => void toggleRunning(modelId)}
                onModelSaved={(savedModel) => {
                  setRunningModels((current) => [
                    savedModel,
                    ...current.filter((model) => model.id !== savedModel.id),
                  ]);
                }}
                onInventoryChanged={setRunningModels}
              />
            ) : (
              <ModelsList
                models={runningModels}
                modelAction={modelAction}
                menuOpenId={menuOpenId}
                menuPosition={menuPosition}
                onAdd={wizard.openWizard}
                onConnectVllm={() => setVllmDialogOpen(true)}
                onOpen={(model) => openDetail(model, "chat")}
                onToggleRunning={(id) => void toggleRunning(id)}
                onToggleMenu={toggleMenu}
                onRename={openRename}
                onPerformance={(model) => openDetail(model, "performance")}
                onApi={(model) => openDetail(model, "api")}
                onSettings={(model) => openDetail(model, "settings")}
                onRemove={requestRemoval}
              />
            )}
          </div>
        </section>
      </main>

      <footer className="app-footer">
        <span className="app-version">{text.footer.version(APP_VERSION)}</span>
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
          model={runningModels.find((model) => model.id === pendingRemovalId)}
          busy={removalBusy}
          onCancel={() => setPendingRemovalId(undefined)}
          onConfirm={() => void confirmRemoval()}
        />
      )}

      {vllmDialogOpen && (
        <VllmModelSettings
          dialog
          onCancel={() => setVllmDialogOpen(false)}
          onSaved={(savedModel) => {
            setRunningModels((current) => [
              savedModel,
              ...current.filter((model) => model.id !== savedModel.id),
            ]);
            setVllmDialogOpen(false);
          }}
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
