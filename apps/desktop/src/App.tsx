import { useEffect, useMemo, useRef, useState, type FormEvent, type KeyboardEvent, type MouseEvent } from "react";
import { desktopCommands, messageFromError } from "./commands";
import { detectHardware } from "./hardwareDetection";
import type { CatalogSummary, HardwareProfile, RunningModelEntry, WizardStep } from "./types";

function formatBytes(bytes: number): string {
  const gibibytes = bytes / (1024 ** 3);
  return `${gibibytes.toFixed(gibibytes >= 10 ? 0 : 1)} GiB`;
}

function App() {
  const [catalog, setCatalog] = useState<CatalogSummary>();
  const [runningModels, setRunningModels] = useState<RunningModelEntry[]>([]);
  const [showSidebar, setShowSidebar] = useState(false);
  const [wizardOpen, setWizardOpen] = useState(false);
  const [wizardStep, setWizardStep] = useState<WizardStep>("location");
  const [wizardLocation, setWizardLocation] = useState<"local" | "remote">("local");
  const [downloadComplete, setDownloadComplete] = useState(false);
  const [downloadBusy, setDownloadBusy] = useState(false);
  const [useCase, setUseCase] = useState<"coding" | "assistant" | "creative" | "research">("assistant");
  const [newName, setNewName] = useState("");
  const [selectedModelId, setSelectedModelId] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const [menuOpenId, setMenuOpenId] = useState<string>();
  const [menuPosition, setMenuPosition] = useState<{ top: number; left: number } | null>(null);
  const [renameId, setRenameId] = useState<string>();
  const [renameValue, setRenameValue] = useState("");
  const [pendingRemovalId, setPendingRemovalId] = useState<string>();
  const [confirmCloseWizard, setConfirmCloseWizard] = useState(false);
  const [hardwareInfo, setHardwareInfo] = useState<HardwareProfile>();
  const [hardwareLoading, setHardwareLoading] = useState(false);
  const [hardwareError, setHardwareError] = useState<string>();
  const renameInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    void desktopCommands.loadCatalog().then((value) => {
      setCatalog(value);
      if (!value.models.length) return;
      if (!selectedModelId) {
        setSelectedModelId(value.models[0].id);
      }
    }).catch(() => undefined);

    void desktopCommands.loadModels().then((saved) => {
      if (saved.length) {
        setRunningModels(saved);
      }
    }).catch(() => undefined);
  }, []);

  useEffect(() => {
    void desktopCommands.saveModels(runningModels).catch(() => undefined);
  }, [runningModels]);

  useEffect(() => {
    if (renameId) {
      window.requestAnimationFrame(() => renameInputRef.current?.focus());
    }
  }, [renameId]);

  // Detect hardware when wizard moves to hardware step
  useEffect(() => {
    if (wizardStep === "hardware" && !hardwareInfo && !hardwareLoading && !hardwareError) {
      setHardwareLoading(true);
      setHardwareError(undefined);
      void detectHardware()
        .then((info) => {
          setHardwareInfo(info);
          setHardwareLoading(false);
        })
        .catch((detectionError: unknown) => {
          setHardwareError(messageFromError(detectionError));
          setHardwareLoading(false);
        });
    }
  }, [wizardStep, hardwareInfo, hardwareLoading, hardwareError]);

  const availableModels = useMemo(() => catalog?.models ?? [], [catalog]);

  const suggestedModel = useMemo(() => {
    const preferredOrder = [
      ["coding", ["code", "coder", "dev", "programming"]],
      ["assistant", ["assistant", "chat", "general", "instruct"]],
      ["creative", ["creative", "writer", "chat", "vision"]],
      ["research", ["research", "analysis", "reasoning", "qwen", "mistral"]],
    ] as const;

    const tokens = preferredOrder.find(([kind]) => kind === useCase)?.[1] ?? [];
    const match = availableModels.find((model) => {
      const haystack = `${model.id} ${model.displayName} ${model.description}`.toLowerCase();
      return tokens.some((token) => haystack.includes(token));
    });

    return match ?? availableModels[0];
  }, [availableModels, useCase]);

  const addModel = () => {
    if (!selectedModelId) return;
    const selected = availableModels.find((model) => model.id === selectedModelId);
    if (!selected) return;

    const entry: RunningModelEntry = {
      id: `${selected.id}-${Date.now()}`,
      name: newName.trim() || selected.displayName,
      modelId: selected.id,
      modelName: selected.displayName,
      version: selected.version,
      location: "local",
      running: false,
      logs: ["Model added to the list."],
    };

    setRunningModels((current) => [entry, ...current]);
    setNewName("");
    setSelectedModelId(selected.id);
    setWizardOpen(false);
    setWizardStep("location");
    setWizardLocation("local");
    setDownloadComplete(false);
    setDownloadBusy(false);
    setShowSidebar(false);
    setError(undefined);
  };

  const toggleRunning = (id: string) => {
    setRunningModels((current) => current.map((model) => model.id === id ? { ...model, running: !model.running } : model));
  };

  const openRename = (model: RunningModelEntry) => {
    setRenameId(model.id);
    setRenameValue(model.name);
    setMenuOpenId(undefined);
    setMenuPosition(null);
  };

  const saveRename = () => {
    if (!renameId) return;
    setRunningModels((current) => current.map((model) => model.id === renameId ? { ...model, name: renameValue.trim() || model.name } : model));
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

  const toggleMenu = (id: string, trigger: HTMLButtonElement) => {
    if (menuOpenId === id) {
      setMenuOpenId(undefined);
      setMenuPosition(null);
      return;
    }

    const rect = trigger.getBoundingClientRect();
    setMenuOpenId(id);
    setMenuPosition({
      top: rect.bottom + 8,
      left: Math.max(12, rect.right - 160),
    });
  };

  const confirmRemoval = () => {
    if (!pendingRemovalId) return;
    setRunningModels((current) => current.filter((model) => model.id !== pendingRemovalId));
    setPendingRemovalId(undefined);
  };

  const openWizard = () => {
    setShowSidebar(true);
    setWizardOpen(true);
    setWizardStep("location");
    setWizardLocation("local");
    setUseCase("assistant");
    setDownloadComplete(false);
    setDownloadBusy(false);
    setNewName("");
    setError(undefined);
    setHardwareInfo(undefined);
    setHardwareLoading(false);
    setHardwareError(undefined);
  };

  const cancelWizard = () => {
    if (wizardStep === "location") {
      confirmWizardClose();
      return;
    }
    setConfirmCloseWizard(true);
  };

  const confirmWizardClose = () => {
    setWizardOpen(false);
    setWizardStep("location");
    setWizardLocation("local");
    setUseCase("assistant");
    setDownloadComplete(false);
    setDownloadBusy(false);
    setShowSidebar(false);
    setNewName("");
    setError(undefined);
    setConfirmCloseWizard(false);
    setHardwareInfo(undefined);
    setHardwareLoading(false);
    setHardwareError(undefined);
  };

  const dismissWizardClose = () => {
    setConfirmCloseWizard(false);
  };

  const goToNextStep = () => {
    if (wizardStep === "location") {
      setWizardStep("hardware");
    } else if (wizardStep === "hardware") {
      setWizardStep("intent");
    } else if (wizardStep === "intent") {
      setSelectedModelId(suggestedModel?.id ?? selectedModelId);
      setWizardStep("suggestion");
    } else if (wizardStep === "suggestion") {
      setWizardStep("requirements");
    } else if (wizardStep === "requirements") {
      setWizardStep("download");
    }
  };

  const goToPreviousStep = () => {
    if (wizardStep === "hardware") {
      setWizardStep("location");
    } else if (wizardStep === "intent") {
      setWizardStep("hardware");
    } else if (wizardStep === "suggestion") {
      setWizardStep("intent");
    } else if (wizardStep === "requirements") {
      setWizardStep("suggestion");
    }
  };

  const startDownload = () => {
    setDownloadBusy(true);
    setError(undefined);
    window.setTimeout(() => {
      setDownloadComplete(true);
      setDownloadBusy(false);
    }, 1200);
  };

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(undefined);
    try {
      addModel();
    } catch (cause) {
      setError(messageFromError(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="app-shell">
      <header className="app-header">
        <a className="brand" href="#" aria-label="Lumen Source home">
          <span className="brand-mark" aria-hidden="true">L</span>
          <span>Lumen Source</span>
        </a>
        <span className="local-badge"><i /> Private by design</span>
      </header>

      <main className="main-panel" aria-busy={busy}>
        {error && (
          <div className="error-banner" role="alert">
            <span>!</span>
            <div><strong>We hit a snag</strong><p>{error}</p></div>
            <button onClick={() => setError(undefined)} aria-label="Dismiss error">×</button>
          </div>
        )}

        <section className={`models-page ${wizardOpen ? "wizard-open" : ""}`}>
          <aside className="sidebar" aria-label="Navigation panel" />

          <div className="content-panel">
            {wizardOpen ? (
              <section className="wizard-card" aria-label="Model setup wizard">
                <div className="wizard-header">
                  <div>
                    <span className="eyebrow">Add model</span>
                    <h2>{wizardStep === "location" ? "Choose deployment" : wizardStep === "hardware" ? "Detect hardware" : wizardStep === "intent" ? "Choose your use case" : wizardStep === "suggestion" ? "Review recommendation" : wizardStep === "requirements" ? "Check requirements" : "Download model"}</h2>
                  </div>
                  <button className="icon-button" type="button" onClick={cancelWizard} aria-label="Cancel wizard">
                    ✕
                  </button>
                </div>

                <div className="wizard-steps" aria-label="Wizard steps">
                  {(["location", "hardware", "intent", "suggestion", "requirements", "download"] as WizardStep[]).map((step, index) => (
                    <span key={step} className={`wizard-step ${wizardStep === step ? "active" : index < (["location", "hardware", "intent", "suggestion", "requirements", "download"] as WizardStep[]).indexOf(wizardStep) ? "complete" : ""}`}>
                      {index + 1}
                    </span>
                  ))}
                </div>

                {wizardStep === "location" ? (
                  <div className="wizard-body">
                    <p>Choose where the runtime should live. Remote is planned for a later release.</p>
                    <div className="choice-list">
                      <button type="button" className={`choice ${wizardLocation === "local" ? "selected" : ""}`} onClick={() => setWizardLocation("local")}>
                        <span className="choice-icon">L</span>
                        <div>
                          <strong>Local runtime</strong>
                          <small>Run the model on this machine.</small>
                        </div>
                        <i />
                      </button>
                      <button type="button" className={`choice ${wizardLocation === "remote" ? "selected" : ""}`} disabled>
                        <span className="choice-icon">R</span>
                        <div>
                          <strong>Remote runtime</strong>
                          <small>Coming soon.</small>
                        </div>
                        <i />
                      </button>
                    </div>
                    <div className="actions">
                      <button className="primary-button" type="button" onClick={goToNextStep}>
                        Continue
                      </button>
                    </div>
                  </div>
                ) : wizardStep === "hardware" ? (
                  <div className="wizard-body">
                    <p>Here is a detailed hardware summary before the model is prepared.</p>
                    <div className="hardware-summary">
                      <div className="scan-card">
                        <div className="scan-visual"><span>⌘</span><i /><i /></div>
                        <div>
                          <strong>
                            {hardwareLoading
                              ? "Detecting hardware..."
                              : hardwareError
                                ? "Hardware detection failed"
                                : "Hardware detected"}
                          </strong>
                          <p>
                            {hardwareLoading
                              ? "Scanning your system..."
                              : hardwareError ?? hardwareInfo?.platform}
                          </p>
                        </div>
                      </div>
                      {hardwareInfo && (
                        <div className="hardware-grid">
                          <div className="hardware-card">
                            <h3>CPU</h3>
                            <ul>
                              <li><span>Model</span><strong>{hardwareInfo.cpu}</strong></li>
                              <li><span>Logical cores</span><strong>{hardwareInfo.cpuCores}</strong></li>
                            </ul>
                          </div>
                          <div className="hardware-card">
                            <h3>Memory</h3>
                            <ul>
                              <li><span>Capacity</span><strong>{formatBytes(hardwareInfo.memoryBytes)}</strong></li>
                            </ul>
                          </div>
                          <div className="hardware-card">
                            <h3>GPU</h3>
                            {hardwareInfo.gpu ? (
                              <ul>
                                <li><span>Model</span><strong>{hardwareInfo.gpu.name}</strong></li>
                                {hardwareInfo.gpu.memoryBytes !== undefined && (
                                  <li><span>VRAM</span><strong>{formatBytes(hardwareInfo.gpu.memoryBytes)}</strong></li>
                                )}
                                <li><span>Backend</span><strong>{hardwareInfo.gpu.backend}</strong></li>
                              </ul>
                            ) : (
                              <p>No supported GPU detected.</p>
                            )}
                          </div>
                          <div className="hardware-card">
                            <h3>Storage</h3>
                            <ul>
                              <li><span>Mount</span><strong>{hardwareInfo.storage.mountPoint}</strong></li>
                              <li><span>Free space</span><strong>{formatBytes(hardwareInfo.storage.availableBytes)}</strong></li>
                              <li><span>Capacity</span><strong>{formatBytes(hardwareInfo.storage.totalBytes)}</strong></li>
                            </ul>
                          </div>
                        </div>
                      )}
                    </div>
                    <div className="actions">
                      <button className="back-button" type="button" onClick={goToPreviousStep}>
                        ← Back
                      </button>
                      <button className="primary-button" type="button" onClick={goToNextStep} disabled={hardwareLoading}>
                        Continue
                      </button>
                    </div>
                  </div>
                ) : wizardStep === "intent" ? (
                  <div className="wizard-body">
                    <p>Select the main way you plan to use the model. This helps tailor the recommendation.</p>
                    <div className="choice-list">
                      <button type="button" className={`choice ${useCase === "coding" ? "selected" : ""}`} onClick={() => setUseCase("coding")}>
                        <span className="choice-icon">⌨</span>
                        <div>
                          <strong>Coding assistant</strong>
                          <small>Good for code generation, debugging, and refactors.</small>
                        </div>
                        <i />
                      </button>
                      <button type="button" className={`choice ${useCase === "assistant" ? "selected" : ""}`} onClick={() => setUseCase("assistant")}>
                        <span className="choice-icon">☁</span>
                        <div>
                          <strong>Personal assistant</strong>
                          <small>Best for conversations, writing, and everyday help.</small>
                        </div>
                        <i />
                      </button>
                      <button type="button" className={`choice ${useCase === "creative" ? "selected" : ""}`} onClick={() => setUseCase("creative")}>
                        <span className="choice-icon">✦</span>
                        <div>
                          <strong>Creative work</strong>
                          <small>Useful for brainstorming, storytelling, and ideation.</small>
                        </div>
                        <i />
                      </button>
                      <button type="button" className={`choice ${useCase === "research" ? "selected" : ""}`} onClick={() => setUseCase("research")}>
                        <span className="choice-icon">🔎</span>
                        <div>
                          <strong>Research & analysis</strong>
                          <small>Great for summarizing documents and exploring ideas.</small>
                        </div>
                        <i />
                      </button>
                    </div>
                    <div className="actions">
                      <button className="back-button" type="button" onClick={goToPreviousStep}>
                        ← Back
                      </button>
                      <button className="primary-button" type="button" onClick={goToNextStep}>
                        Continue
                      </button>
                    </div>
                  </div>
                ) : wizardStep === "suggestion" ? (
                  <div className="wizard-body">
                    <p>Based on your {useCase === "coding" ? "coding" : useCase === "creative" ? "creative" : useCase === "research" ? "research" : "assistant"} use case, the catalog suggests the following model for setup.</p>
                    <div className="choice-list">
                      {availableModels.map((model) => {
                        const rationale = useCase === "coding"
                          ? "Good fit for code generation, debugging, and repo-aware assistance."
                          : useCase === "creative"
                            ? "Well suited for writing, brainstorming, and creative ideation."
                            : useCase === "research"
                              ? "Strong for summarizing, reasoning, and document-heavy analysis."
                              : "Balanced for chat, writing, and general-purpose assistance.";

                        return (
                          <button
                            key={model.id}
                            type="button"
                            className={`choice ${selectedModelId === model.id ? "selected" : ""}`}
                            onClick={() => setSelectedModelId(model.id)}
                          >
                            <span className="choice-icon">{model.displayName.charAt(0)}</span>
                            <div>
                              <strong>{model.displayName}</strong>
                              <small>{model.description}</small>
                              <span className="recommendation-rationale">{rationale}</span>
                            </div>
                            <i />
                          </button>
                        );
                      })}
                    </div>
                    <div className="actions">
                      <button className="back-button" type="button" onClick={goToPreviousStep}>
                        ← Back
                      </button>
                      <button className="primary-button" type="button" onClick={goToNextStep} disabled={!selectedModelId}>
                        Continue
                      </button>
                    </div>
                  </div>
                ) : wizardStep === "requirements" ? (
                  <div className="wizard-body">
                    <p>Check the requirements before the model is downloaded.</p>
                    <div className="check-list">
                      <div className="check">
                        <span>✓</span>
                        <div><strong>Storage</strong><small>At least 4 GB of free disk space.</small></div>
                      </div>
                      <div className="check">
                        <span>✓</span>
                        <div><strong>Runtime</strong><small>Local runtime support is available.</small></div>
                      </div>
                      <div className="check">
                        <span>✓</span>
                        <div><strong>Catalog entry</strong><small>The selected model is available from the local catalog.</small></div>
                      </div>
                    </div>
                    <div className="actions">
                      <button className="back-button" type="button" onClick={goToPreviousStep}>
                        ← Back
                      </button>
                      <button className="primary-button" type="button" onClick={goToNextStep}>
                        Continue
                      </button>
                    </div>
                  </div>
                ) : (
                  <div className="wizard-body">
                    <p>Download the selected model and finish setup.</p>
                    <div className="install-stage">
                      <div className="model-orb"><span>⬇</span><i /></div>
                      <h3>{downloadComplete ? "Download complete" : downloadBusy ? "Download in progress" : "Waiting for download"}</h3>
                      <p>{downloadComplete ? "Everything is ready. You can finish the setup now." : downloadBusy ? "The download is running in the background." : "Press the button below to start the model download."}</p>
                      {downloadBusy && <div className="progress-track"><i style={{ width: "72%" }} /></div>}
                    </div>
                    <div className="actions">
                      <button className="primary-button" type="button" onClick={downloadComplete ? handleSubmit : startDownload} disabled={busy || (downloadBusy && !downloadComplete)}>
                        {downloadBusy ? "Download in progress" : downloadComplete ? "Done" : "Download model"}
                      </button>
                    </div>
                  </div>
                )}
              </section>
            ) : (
              <>
                <div className="models-toolbar">
                  <div>
                    <span className="eyebrow">Models</span>
                    <h1>Local models</h1>
                  </div>
                  <button className="primary-button" type="button" onClick={openWizard}>
                    Add model
                  </button>
                </div>

                <div className="models-table" role="table" aria-label="Running models">
                  {runningModels.length > 0 ? runningModels.map((model) => (
                    <div className="models-table-row" role="row" key={model.id}>
                      <div className="model-main">
                        <span className={`led ${model.running ? "on" : "off"}`} aria-label={model.running ? "Running" : "Stopped"} />
                        <div>
                          <strong role="cell">{model.name}</strong>
                          <p>{model.modelName} · {model.version}</p>
                        </div>
                      </div>
                      <div className="model-actions">
                        <button className="icon-button" type="button" onClick={() => toggleRunning(model.id)}>
                          {model.running ? "⏹" : "▶"}
                        </button>
                        <div className="menu-wrap">
                          <button
                            className="icon-button"
                            type="button"
                            onClick={(event: MouseEvent<HTMLButtonElement>) => toggleMenu(model.id, event.currentTarget)}
                          >
                            ⋯
                          </button>
                          {menuOpenId === model.id && menuPosition && (
                            <div className="menu-popover" style={{ top: `${menuPosition.top}px`, left: `${menuPosition.left}px` }}>
                              <button type="button" onClick={() => openRename(model)}>Rename</button>
                              <button type="button">Logs</button>
                              <button type="button">Performance</button>
                              <button type="button" onClick={() => requestRemoval(model.id)}>Remove</button>
                            </div>
                          )}
                        </div>
                      </div>
                    </div>
                  )) : (
                    <div className="empty-state" role="row">
                      <strong>No running models yet</strong>
                      <p>Add one from the catalog to start building your local list.</p>
                    </div>
                  )}
                </div>
              </>
            )}
          </div>
        </section>
      </main>

      <footer className="app-footer">
        {catalog && (
          <span>
            Catalog {catalog.revision} · {catalog.modelCount} models available · {catalog.source}
          </span>
        )}
      </footer>

      {renameId && (
        <div className="modal-backdrop" onClick={() => setRenameId(undefined)}>
          <form className="modal-card" onClick={(event) => event.stopPropagation()} onSubmit={(event) => {
            event.preventDefault();
            saveRename();
          }}>
            <h3>Rename model</h3>
            <input ref={renameInputRef} value={renameValue} onChange={(event) => setRenameValue(event.target.value)} onKeyDown={handleRenameKeyDown} />
            <div className="modal-actions">
              <button className="secondary-button" type="button" onClick={() => setRenameId(undefined)}>Cancel</button>
              <button className="primary-button" type="submit">Save</button>
            </div>
          </form>
        </div>
      )}

      {pendingRemovalId && (
        <div className="modal-backdrop" onClick={() => setPendingRemovalId(undefined)}>
          <div className="modal-card" onClick={(event) => event.stopPropagation()}>
            <h3>Remove model?</h3>
            <p>This will remove the model from your list. Continue?</p>
            <div className="modal-actions">
              <button className="secondary-button" type="button" onClick={() => setPendingRemovalId(undefined)}>Cancel</button>
              <button className="primary-button" type="button" onClick={confirmRemoval}>Remove</button>
            </div>
          </div>
        </div>
      )}

      {confirmCloseWizard && (
        <div className="modal-backdrop" onClick={dismissWizardClose}>
          <div className="modal-card" onClick={(event) => event.stopPropagation()}>
            <h3>Leave the setup?</h3>
            <p>Your progress will be discarded if you close the assistant now. Continue?</p>
            <div className="modal-actions">
              <button className="secondary-button" type="button" onClick={dismissWizardClose}>Stay</button>
              <button className="primary-button" type="button" onClick={confirmWizardClose}>Leave</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default App;
