import { useEffect, useRef, useState, type KeyboardEvent, type MouseEvent } from "react";
import { createPortal } from "react-dom";
import { openUrl } from "@tauri-apps/plugin-opener";
import appIcon from "../../../icon.png";
import { desktopCommands, messageFromError } from "./commands";
import { detectHardware } from "./hardwareDetection";
import type {
  CatalogSummary,
  ChatMessage,
  EndpointDetails,
  HardwareProfile,
  InstallProgress,
  PerformanceSnapshot,
  PreflightReport,
  Recommendation,
  RemoteConnectionReport,
  RemoteTargetConfig,
  RemoteTargetProfile,
  RunningModelEntry,
  UseIntent,
  WizardStep,
} from "./types";

const WIZARD_STEPS: WizardStep[] = [
  "location",
  "hardware",
  "intent",
  "suggestion",
  "license",
  "download",
  "ready",
];

const WIZARD_TITLES: Record<WizardStep, string> = {
  location: "Choose deployment",
  hardware: "Detect hardware",
  intent: "Choose your use case",
  suggestion: "Review recommendations",
  license: "Review license",
  download: "Install model",
  ready: "Ready to connect",
};

function emptyRemoteTarget(): RemoteTargetConfig {
  return {
    name: "",
    host: "",
    port: 22,
    username: "",
    authentication: "key",
  };
}

function formatBytes(bytes: number): string {
  const gibibytes = bytes / (1024 ** 3);
  return `${gibibytes.toFixed(gibibytes >= 10 ? 0 : 1)} GiB`;
}

function formatLegalValue(value: string): string {
  return value.replaceAll("-", " ").replace(/^./, (letter) => letter.toUpperCase());
}

const CATALOG_SOURCE_LABELS: Record<CatalogSummary["source"], string> = {
  network: "Latest catalog",
  cache: "Cached catalog",
  bundled: "Bundled catalog",
};

const CATALOG_SOURCE_DETAILS: Record<CatalogSummary["source"], string> = {
  network: "Fetched from lumensource.dev and signature-verified at startup.",
  cache: "The online catalog was unavailable. Using the last signature-verified copy saved on this device.",
  bundled: "No verified cached catalog was available. Using the catalog included with this version of LumenSource.",
};

function progressPercent(progress?: InstallProgress): number {
  if (!progress) return 0;
  if (progress.phase === "cancelled") return 0;
  if (progress.phase === "complete") return 100;
  if (progress.phase === "preparing") return 5;
  if (progress.phase === "verifying") return 88;
  if (progress.phase === "installing") return 95;
  if (progress.totalBytes <= 0) return 12;
  return Math.max(8, Math.min(85, (progress.completedBytes / progress.totalBytes) * 85));
}

function lifecycleLog(message: string): string {
  return `[${new Date().toISOString()}] ${message}`;
}

function curlExample(details: EndpointDetails): string {
  const body = details.chatAvailable
    ? { model: details.model, messages: [{ role: "user", content: "Hello" }] }
    : details.embeddingsAvailable
      ? { model: details.model, input: "Hello from LumenSource" }
      : { model: details.model, prompt: "Hello from LumenSource" };
  const url = inferenceUrl(details);
  return `curl '${url}' \\\n+  --header 'Content-Type: application/json' \\\n+  --data-binary @- <<'JSON'\n${JSON.stringify(body, null, 2)}\nJSON`;
}

function inferenceLabel(details: EndpointDetails): string {
  if (details.chatAvailable) return "Chat completions";
  return details.embeddingsAvailable ? "Embeddings" : "Completions";
}

function inferenceUrl(details: EndpointDetails): string {
  if (details.chatAvailable) return details.chatCompletionsUrl;
  return details.embeddingsAvailable ? details.embeddingsUrl : details.completionsUrl;
}

interface PerformanceHistoryPoint {
  sampledAtUnixMs: number;
  totalMemoryBytes: number;
  systemMemoryBytes: number;
  vramBytes: number;
}

function PerformanceChart({ history }: { history: PerformanceHistoryPoint[] }) {
  const width = 800;
  const height = 270;
  const padding = { top: 18, right: 18, bottom: 38, left: 46 };
  const plotWidth = width - padding.left - padding.right;
  const plotHeight = height - padding.top - padding.bottom;
  const series = [
    { key: "total", label: "Total model memory", color: "#d98938", values: history.map((point) => point.totalMemoryBytes) },
    { key: "system", label: "Model allocation in system RAM", color: "#65a9d7", values: history.map((point) => point.systemMemoryBytes) },
    { key: "vram", label: "GPU VRAM", color: "#b978d1", values: history.map((point) => point.vramBytes) },
  ];
  const gibibyte = 1024 ** 3;
  const maximumObserved = Math.max(gibibyte, ...series.flatMap((item) => item.values));
  const maximumBytes = Math.ceil(maximumObserved / gibibyte) * gibibyte;
  const x = (index: number) => padding.left + (history.length <= 1 ? 0 : (index / (history.length - 1)) * plotWidth);
  const y = (value: number) => padding.top + ((maximumBytes - Math.max(0, Math.min(maximumBytes, value))) / maximumBytes) * plotHeight;
  const firstSample = history[0]?.sampledAtUnixMs;
  const lastSample = history.at(-1)?.sampledAtUnixMs;

  return (
    <section className="performance-chart-card" aria-labelledby="performance-history-title">
      <div className="chart-header">
        <div>
          <h3 id="performance-history-title">Model memory history</h3>
          <p>Per-model Ollama allocation · rolling two-minute window</p>
        </div>
        <div className="chart-legend" aria-label="Chart series">
          {series.map((item) => {
            const current = item.values.at(-1) ?? 0;
            return (
              <span key={item.key}><i style={{ background: item.color }} />{item.label}<b>{formatBytes(current)}</b></span>
            );
          })}
        </div>
      </div>
      {history.length < 2 ? (
        <div className="chart-collecting"><span className="model-control-spinner" /> Collecting enough samples to draw the graph…</div>
      ) : (
        <svg className="performance-chart" viewBox={`0 0 ${width} ${height}`} role="img" aria-label="Per-model total memory, system memory, and GPU memory allocation history">
          {[0, 0.25, 0.5, 0.75, 1].map((ratio) => {
            const value = maximumBytes * ratio;
            return (
            <g key={ratio}>
              <line x1={padding.left} x2={width - padding.right} y1={y(value)} y2={y(value)} className="chart-grid-line" />
              <text x={padding.left - 10} y={y(value) + 4} textAnchor="end" className="chart-axis-label">{formatBytes(value)}</text>
            </g>
            );
          })}
          {series.map((item) => {
            const points = item.values.map((value, index) => `${x(index)},${y(value)}`).join(" ");
            return <polyline key={item.key} points={points} fill="none" stroke={item.color} className="chart-series-line" />;
          })}
          <text x={padding.left} y={height - 10} className="chart-time-label">{firstSample ? new Date(firstSample).toLocaleTimeString() : ""}</text>
          <text x={width - padding.right} y={height - 10} textAnchor="end" className="chart-time-label">{lastSample ? new Date(lastSample).toLocaleTimeString() : "Now"}</text>
        </svg>
      )}
    </section>
  );
}

function App() {
  const [catalog, setCatalog] = useState<CatalogSummary>();
  const [runningModels, setRunningModels] = useState<RunningModelEntry[]>([]);
  const [wizardOpen, setWizardOpen] = useState(false);
  const [wizardStep, setWizardStep] = useState<WizardStep>("location");
  const [wizardLocation, setWizardLocation] = useState<"local" | "remote">("local");
  const [remoteTargets, setRemoteTargets] = useState<RemoteTargetProfile[]>([]);
  const [selectedRemoteTargetId, setSelectedRemoteTargetId] = useState("");
  const [remoteTargetDialogOpen, setRemoteTargetDialogOpen] = useState(false);
  const [remoteTargetSaveLoading, setRemoteTargetSaveLoading] = useState(false);
  const [remoteTargetDialogError, setRemoteTargetDialogError] = useState<string>();
  const [remoteConfig, setRemoteConfig] = useState<RemoteTargetConfig>(emptyRemoteTarget);
  const [remotePassword, setRemotePassword] = useState("");
  const [remoteReport, setRemoteReport] = useState<RemoteConnectionReport>();
  const [remoteCheckLoading, setRemoteCheckLoading] = useState(false);
  const [downloadBusy, setDownloadBusy] = useState(false);
  const [installCancellable, setInstallCancellable] = useState(false);
  const [cancelBusy, setCancelBusy] = useState(false);
  const [startAfterInstall, setStartAfterInstall] = useState(true);
  const [useCase, setUseCase] = useState<UseIntent>("chat");
  const [selectedModelId, setSelectedModelId] = useState("");
  const [recommendations, setRecommendations] = useState<Recommendation[]>([]);
  const [modelPickerOpen, setModelPickerOpen] = useState(false);
  const [modelPickerPosition, setModelPickerPosition] = useState<{ top: number; left: number; width: number }>();
  const [preflight, setPreflight] = useState<PreflightReport>();
  const [preflightLoading, setPreflightLoading] = useState(false);
  const [licenseBasis, setLicenseBasis] = useState<"catalog" | "separate">("catalog");
  const [licenseAcknowledged, setLicenseAcknowledged] = useState(false);
  const [separateLicenseReference, setSeparateLicenseReference] = useState("");
  const [installProgress, setInstallProgress] = useState<InstallProgress>();
  const [endpoint, setEndpoint] = useState<EndpointDetails>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const [menuOpenId, setMenuOpenId] = useState<string>();
  const [menuPosition, setMenuPosition] = useState<{ top: number; left: number } | null>(null);
  const [detailModelId, setDetailModelId] = useState<string>();
  const [detailTab, setDetailTab] = useState<"chat" | "logs" | "performance" | "api">("logs");
  const [chatSessions, setChatSessions] = useState<Record<string, ChatMessage[]>>({});
  const [chatDraft, setChatDraft] = useState("");
  const [chatBusyModelId, setChatBusyModelId] = useState<string>();
  const [chatCancelBusy, setChatCancelBusy] = useState(false);
  const [chatError, setChatError] = useState<string>();
  const [modelAction, setModelAction] = useState<{ id: string; action: "starting" | "stopping" }>();
  const [modelEndpoint, setModelEndpoint] = useState<EndpointDetails>();
  const [modelEndpointLoading, setModelEndpointLoading] = useState(false);
  const [modelEndpointError, setModelEndpointError] = useState<string>();
  const [performanceSnapshot, setPerformanceSnapshot] = useState<PerformanceSnapshot>();
  const [performanceHistory, setPerformanceHistory] = useState<PerformanceHistoryPoint[]>([]);
  const [performanceLoading, setPerformanceLoading] = useState(false);
  const [performanceError, setPerformanceError] = useState<string>();
  const [renameId, setRenameId] = useState<string>();
  const [renameValue, setRenameValue] = useState("");
  const [pendingRemovalId, setPendingRemovalId] = useState<string>();
  const [removalBusy, setRemovalBusy] = useState(false);
  const [confirmCloseWizard, setConfirmCloseWizard] = useState(false);
  const [hardwareInfo, setHardwareInfo] = useState<HardwareProfile>();
  const [hardwareLoading, setHardwareLoading] = useState(false);
  const [hardwareError, setHardwareError] = useState<string>();
  const [modelsLoaded, setModelsLoaded] = useState(false);
  const [copiedField, setCopiedField] = useState<string>();
  const [telemetryEnabled, setTelemetryEnabled] = useState<boolean>();
  const [telemetryDialogOpen, setTelemetryDialogOpen] = useState(false);
  const [telemetrySaving, setTelemetrySaving] = useState(false);
  const renameInputRef = useRef<HTMLInputElement>(null);
  const contentPanelRef = useRef<HTMLDivElement>(null);
  const chatTranscriptRef = useRef<HTMLDivElement>(null);
  const copyFeedbackTimerRef = useRef<number | undefined>(undefined);
  const pendingModelSaveRef = useRef<RunningModelEntry[] | null>(null);
  const modelSaveInFlightRef = useRef(false);
  const selectedRemoteTarget = remoteTargets.find((target) => target.targetId === selectedRemoteTargetId);
  const detailModel = runningModels.find((model) => model.id === detailModelId);
  const detailChatMessages = detailModelId ? chatSessions[detailModelId] ?? [] : [];
  const wizardTargetId = wizardLocation === "remote" ? remoteReport?.targetId : "local";

  useEffect(() => {
    void desktopCommands.refreshCatalog().then((value) => {
      setCatalog(value);
    }).catch((catalogError: unknown) => setError(messageFromError(catalogError)));

    void desktopCommands.loadModels().then((saved) => {
      setRunningModels(saved);
    }).catch((loadError: unknown) => {
      setError(messageFromError(loadError));
    }).finally(() => setModelsLoaded(true));

    void desktopCommands.loadRemoteTargets().then(setRemoteTargets).catch((loadError: unknown) => {
      setError(messageFromError(loadError));
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
    if (!modelsLoaded) return;
    pendingModelSaveRef.current = runningModels;
    if (modelSaveInFlightRef.current) return;

    modelSaveInFlightRef.current = true;
    void (async () => {
      try {
        while (pendingModelSaveRef.current) {
          const models = pendingModelSaveRef.current;
          pendingModelSaveRef.current = null;
          await desktopCommands.saveModels(models);
        }
      } catch (saveError) {
        setError(messageFromError(saveError));
      } finally {
        modelSaveInFlightRef.current = false;
      }
    })();
  }, [modelsLoaded, runningModels]);

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

  useEffect(() => {
    if (!modelPickerOpen) return;

    const dismissPicker = (event: PointerEvent) => {
      const target = event.target;
      if (target instanceof Element && target.closest("[data-catalog-model-picker]")) return;
      setModelPickerOpen(false);
      setModelPickerPosition(undefined);
    };
    const dismissPickerWithKeyboard = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "Escape") return;
      setModelPickerOpen(false);
      setModelPickerPosition(undefined);
    };
    const dismissPickerAfterResize = () => {
      setModelPickerOpen(false);
      setModelPickerPosition(undefined);
    };
    const dismissPickerAfterOutsideScroll = (event: Event) => {
      const target = event.target;
      if (target instanceof Element && target.closest("[data-catalog-model-picker]")) return;
      setModelPickerOpen(false);
      setModelPickerPosition(undefined);
    };

    document.addEventListener("pointerdown", dismissPicker, true);
    document.addEventListener("keydown", dismissPickerWithKeyboard);
    window.addEventListener("resize", dismissPickerAfterResize);
    document.addEventListener("scroll", dismissPickerAfterOutsideScroll, true);
    return () => {
      document.removeEventListener("pointerdown", dismissPicker, true);
      document.removeEventListener("keydown", dismissPickerWithKeyboard);
      window.removeEventListener("resize", dismissPickerAfterResize);
      document.removeEventListener("scroll", dismissPickerAfterOutsideScroll, true);
    };
  }, [modelPickerOpen]);

  useEffect(() => {
    if (!detailModel?.runtimeModelId) return;

    let disposed = false;
    let refreshInFlight = false;
    const modelId = detailModel.modelId;
    const runtimeModelId = detailModel.runtimeModelId;
    const refresh = async () => {
      if (refreshInFlight) return;
      refreshInFlight = true;
      try {
        const snapshot = await desktopCommands.performance(modelId, runtimeModelId, detailModel.targetId);
        if (!disposed) {
          setPerformanceSnapshot(snapshot);
          const historyPoint: PerformanceHistoryPoint = {
            sampledAtUnixMs: snapshot.sampledAtUnixMs,
            totalMemoryBytes: snapshot.allocatedMemoryBytes,
            systemMemoryBytes: snapshot.allocatedSystemMemoryBytes,
            vramBytes: snapshot.allocatedVramBytes,
          };
          setPerformanceHistory((current) => {
            const withoutDuplicate = current.filter((point) => point.sampledAtUnixMs !== historyPoint.sampledAtUnixMs);
            return [...withoutDuplicate, historyPoint].slice(-60);
          });
          setPerformanceError(undefined);
        }
      } catch (performanceFailure) {
        if (!disposed) setPerformanceError(messageFromError(performanceFailure));
      } finally {
        refreshInFlight = false;
        if (!disposed) setPerformanceLoading(false);
      }
    };

    setPerformanceSnapshot(undefined);
    setPerformanceHistory([]);
    setPerformanceError(undefined);
    setPerformanceLoading(true);
    void refresh();
    const interval = window.setInterval(() => void refresh(), 2_000);
    return () => {
      disposed = true;
      window.clearInterval(interval);
    };
  }, [detailModel?.modelId, detailModel?.runtimeModelId, detailModel?.targetId]);

  useEffect(() => {
    if ((detailTab !== "api" && detailTab !== "chat") || !detailModel) return;
    if (!detailModel.runtimeModelId) {
      setModelEndpoint(undefined);
      setModelEndpointError("This entry does not expose a runtime model identifier.");
      setModelEndpointLoading(false);
      return;
    }

    let disposed = false;
    setModelEndpoint(undefined);
    setModelEndpointError(undefined);
    setModelEndpointLoading(true);
    void desktopCommands.modelEndpoint(detailModel.modelId, detailModel.runtimeModelId, detailModel.targetId)
      .then((details) => {
        if (!disposed) setModelEndpoint(details);
      })
      .catch((endpointError: unknown) => {
        if (!disposed) setModelEndpointError(messageFromError(endpointError));
      })
      .finally(() => {
        if (!disposed) setModelEndpointLoading(false);
      });
    return () => {
      disposed = true;
    };
  }, [detailTab, detailModel?.modelId, detailModel?.runtimeModelId, detailModel?.targetId]);

  useEffect(() => {
    setChatDraft("");
    setChatError(undefined);
  }, [detailModelId]);

  useEffect(() => {
    if (detailTab !== "chat") return;
    const frame = window.requestAnimationFrame(() => {
      chatTranscriptRef.current?.scrollTo({
        top: chatTranscriptRef.current.scrollHeight,
        behavior: "smooth",
      });
    });
    return () => window.cancelAnimationFrame(frame);
  }, [detailTab, detailModelId, detailChatMessages]);

  // Detect hardware on the selected local or connected remote target.
  useEffect(() => {
    if (wizardStep === "hardware" && wizardTargetId && !hardwareInfo && !hardwareLoading && !hardwareError) {
      setHardwareLoading(true);
      setHardwareError(undefined);
      void detectHardware(wizardTargetId)
        .then((info) => {
          setHardwareInfo(info);
          setHardwareLoading(false);
        })
        .catch((detectionError: unknown) => {
          setHardwareError(messageFromError(detectionError));
          setHardwareLoading(false);
        });
    }
  }, [wizardStep, wizardTargetId, hardwareInfo, hardwareLoading, hardwareError]);

  useEffect(() => {
    if (wizardStep !== "suggestion" || !selectedModelId) return;
    let disposed = false;
    setPreflight(undefined);
    setPreflightLoading(true);
    setError(undefined);
    if (!wizardTargetId) return;
    void desktopCommands.preflight(selectedModelId, wizardTargetId)
      .then((report) => {
        if (!disposed) setPreflight(report);
      })
      .catch((preflightError: unknown) => {
        if (!disposed) setError(messageFromError(preflightError));
      })
      .finally(() => {
        if (!disposed) setPreflightLoading(false);
      });
    return () => {
      disposed = true;
    };
  }, [wizardStep, selectedModelId, wizardTargetId]);

  useEffect(() => {
    setLicenseBasis("catalog");
    setLicenseAcknowledged(false);
    setSeparateLicenseReference("");
  }, [selectedModelId]);

  const selectedRecommendation = recommendations.find(
    (recommendation) => recommendation.modelId === selectedModelId,
  );
  const recommendedModel = recommendations.find((recommendation) => recommendation.recommended);
  const selectedIsDummy = selectedRecommendation?.runtimeId === "dummy-runtime";
  const licenseReviewComplete = Boolean(selectedRecommendation) && (licenseBasis === "catalog"
    ? !selectedRecommendation?.license.requiresUserAcceptance || licenseAcknowledged
    : licenseAcknowledged && separateLicenseReference.trim().length > 0);

  const addInstalledModel = (
    selected: Recommendation,
    runtimeModelId: string,
    running: boolean,
  ) => {
    setRunningModels((current) => {
      const sharedRuntimeAlreadyRunning = current.some((model) => (
        model.targetId === (wizardTargetId ?? "local")
        && model.runtimeModelId === runtimeModelId
        && model.running
      ));
      const effectiveRunning = running || sharedRuntimeAlreadyRunning;
      const message = running
        ? "Model installed and started successfully."
        : sharedRuntimeAlreadyRunning
          ? "Model installed successfully. Its shared runtime was already running."
          : "Model installed successfully. Automatic start was skipped.";
      const entry: RunningModelEntry = {
        id: `${selected.modelId}-${crypto.randomUUID()}`,
        name: selected.name,
        modelId: selected.modelId,
        modelName: selected.name,
        runtimeModelId,
        version: selected.version,
        location: wizardLocation,
        targetId: wizardTargetId ?? "local",
        targetName: wizardLocation === "remote" ? remoteReport?.targetName : undefined,
        running: effectiveRunning,
        managed: true,
        digest: selected.runtimeDigest,
        licenseBasis,
        licenseReference: licenseBasis === "separate" ? separateLicenseReference.trim() : undefined,
        licenseAcknowledgedAt: licenseAcknowledged ? new Date().toISOString() : undefined,
        licenseProfileId: selected.license.profileId,
        licenseName: selected.license.name,
        licenseUrl: selected.license.url,
        licenseReviewedAt: selected.license.reviewedAt,
        licenseCatalogVersion: catalog?.revision,
        logs: [lifecycleLog(message)],
      };
      return [entry, ...current.map((model) => (
        model.targetId === (wizardTargetId ?? "local") && model.runtimeModelId === runtimeModelId
          ? {
              ...model,
              running: effectiveRunning,
              logs: !model.running && effectiveRunning
                ? [...model.logs, lifecycleLog("Shared runtime started by another installation.")]
                : model.logs,
            }
          : model
      ))];
    });
  };

  const toggleRunning = async (id: string) => {
    const selected = runningModels.find((model) => model.id === id);
    if (!selected || modelAction) return;
    setModelAction({ id, action: selected.running ? "stopping" : "starting" });
    setError(undefined);
    try {
      if (selected.running) {
        await desktopCommands.stop(selected.modelId, selected.targetId);
        setRunningModels((current) => current.map((model) => (
          model.id === id || (
            selected.runtimeModelId !== undefined
            && model.targetId === selected.targetId
            && model.runtimeModelId === selected.runtimeModelId
          )
            ? { ...model, running: false, logs: [...model.logs, lifecycleLog("Model stopped.")] }
            : model
        )));
      } else {
        await desktopCommands.start(selected.modelId, selected.targetId);
        setRunningModels((current) => current.map((model) => (
          model.id === id || (
            selected.runtimeModelId !== undefined
            && model.targetId === selected.targetId
            && model.runtimeModelId === selected.runtimeModelId
          )
            ? { ...model, running: true, logs: [...model.logs, lifecycleLog("Model started.")] }
            : model
        )));
      }
    } catch (runtimeError) {
      setError(messageFromError(runtimeError));
    } finally {
      setModelAction(undefined);
    }
  };

  const openRename = (model: RunningModelEntry) => {
    setRenameId(model.id);
    setRenameValue(model.name);
    setMenuOpenId(undefined);
    setMenuPosition(null);
  };

  const saveRename = () => {
    if (!renameId) return;
    setRunningModels((current) => current.map((model) => {
      if (model.id !== renameId) return model;
      const name = renameValue.trim() || model.name;
      return name === model.name
        ? model
        : { ...model, name, logs: [...model.logs, lifecycleLog(`Renamed from ${model.name} to ${name}.`)] };
    }));
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

  const openLogs = (model: RunningModelEntry) => {
    setDetailModelId(model.id);
    setDetailTab("logs");
    setMenuOpenId(undefined);
    setMenuPosition(null);
  };

  const openPerformance = (model: RunningModelEntry) => {
    setDetailModelId(model.id);
    setDetailTab("performance");
    setMenuOpenId(undefined);
    setMenuPosition(null);
  };

  const openApi = (model: RunningModelEntry) => {
    setDetailModelId(model.id);
    setDetailTab("api");
    setMenuOpenId(undefined);
    setMenuPosition(null);
  };

  const clearChat = () => {
    if (!detailModelId || chatBusyModelId === detailModelId) return;
    setChatSessions((current) => ({ ...current, [detailModelId]: [] }));
    setChatError(undefined);
  };

  const sendChatMessage = async () => {
    const model = detailModel;
    const content = chatDraft.trim();
    if (!model || !content || chatBusyModelId) return;
    if (!model.running) {
      setChatError("Start this model before sending a message.");
      return;
    }
    if (model.managed === false || !model.runtimeModelId || !modelEndpoint?.apiAvailable || !modelEndpoint.chatAvailable) {
      setChatError("This model entry does not expose an available chat API.");
      return;
    }

    const userMessage: ChatMessage = { role: "user", content };
    const requestMessages = [...(chatSessions[model.id] ?? []), userMessage];
    setChatSessions((current) => ({
      ...current,
      [model.id]: [...requestMessages, { role: "assistant", content: "" }],
    }));
    setChatDraft("");
    setChatError(undefined);
    setChatBusyModelId(model.id);

    try {
      await desktopCommands.chat(
        model.modelId,
        model.runtimeModelId,
        model.targetId,
        requestMessages,
        (event) => {
          setChatSessions((current) => {
            const messages = [...(current[model.id] ?? [])];
            const last = messages.at(-1);
            if (!last || last.role !== "assistant") return current;
            if (event.event === "delta") {
              messages[messages.length - 1] = { ...last, content: last.content + event.content };
            } else if (!last.content) {
              messages.pop();
            }
            return { ...current, [model.id]: messages };
          });
        },
      );
    } catch (chatFailure) {
      const message = messageFromError(chatFailure);
      setChatSessions((current) => {
        const messages = [...(current[model.id] ?? [])];
        if (messages.at(-1)?.role === "assistant" && !messages.at(-1)?.content) messages.pop();
        return { ...current, [model.id]: messages };
      });
      if (!message.toLowerCase().includes("chat cancelled")) setChatError(message);
    } finally {
      setChatBusyModelId(undefined);
      setChatCancelBusy(false);
    }
  };

  const stopChat = async () => {
    if (!chatBusyModelId || chatCancelBusy) return;
    setChatCancelBusy(true);
    try {
      await desktopCommands.cancelChat();
    } catch (cancelError) {
      setChatError(messageFromError(cancelError));
      setChatCancelBusy(false);
    }
  };

  const handleChatKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key !== "Enter" || event.shiftKey) return;
    event.preventDefault();
    void sendChatMessage();
  };

  const clearLogs = () => {
    if (!detailModelId) return;
    setRunningModels((current) => current.map((model) => (
      model.id === detailModelId ? { ...model, logs: [] } : model
    )));
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
      const remainingModels = await desktopCommands.removeModel(modelId);
      setRunningModels(remainingModels);
      setPendingRemovalId(undefined);
    } catch (removalError) {
      setError(messageFromError(removalError));
    } finally {
      setRemovalBusy(false);
    }
  };

  const openRemoteTargetDialog = () => {
    setRemoteConfig(emptyRemoteTarget());
    setRemoteTargetDialogError(undefined);
    setRemoteTargetDialogOpen(true);
  };

  const closeRemoteTargetDialog = () => {
    if (remoteTargetSaveLoading) return;
    setRemoteTargetDialogOpen(false);
    setRemoteTargetDialogError(undefined);
    setRemoteConfig(emptyRemoteTarget());
  };

  const saveRemoteTarget = async () => {
    setRemoteTargetSaveLoading(true);
    setRemoteTargetDialogError(undefined);
    try {
      const saved = await desktopCommands.saveRemoteTarget({
        ...remoteConfig,
        name: remoteConfig.name.trim(),
        host: remoteConfig.host.trim(),
        username: remoteConfig.username.trim(),
        identityFile: remoteConfig.authentication === "key"
          ? remoteConfig.identityFile?.trim() || undefined
          : undefined,
      });
      setRemoteTargets((current) => [...current.filter((target) => target.targetId !== saved.targetId), saved]
        .sort((left, right) => left.targetName.localeCompare(right.targetName)));
      setSelectedRemoteTargetId(saved.targetId);
      setRemoteReport(undefined);
      setRemotePassword("");
      setRemoteTargetDialogOpen(false);
      setRemoteConfig(emptyRemoteTarget());
    } catch (saveError) {
      setRemoteTargetDialogError(messageFromError(saveError));
    } finally {
      setRemoteTargetSaveLoading(false);
    }
  };

  const checkRemoteTarget = async () => {
    if (!selectedRemoteTarget) return;
    setRemoteCheckLoading(true);
    setRemoteReport(undefined);
    setError(undefined);
    try {
      const report = await desktopCommands.checkRemoteTarget(
        selectedRemoteTarget.config,
        selectedRemoteTarget.config.authentication === "password" ? remotePassword : undefined,
      );
      setRemoteReport(report);
    } catch (connectionError) {
      setError(messageFromError(connectionError));
    } finally {
      setRemotePassword("");
      setRemoteCheckLoading(false);
    }
  };

  const openWizard = () => {
    setDetailModelId(undefined);
    setWizardOpen(true);
    setWizardStep("location");
    setWizardLocation("local");
    setSelectedRemoteTargetId("");
    setRemoteReport(undefined);
    setRemoteCheckLoading(false);
    setRemotePassword("");
    setRemoteTargetDialogOpen(false);
    setRemoteTargetDialogError(undefined);
    setRemoteConfig(emptyRemoteTarget());
    setUseCase("chat");
    setDownloadBusy(false);
    setInstallCancellable(false);
    setCancelBusy(false);
    setStartAfterInstall(true);
    setSelectedModelId("");
    setRecommendations([]);
    setModelPickerOpen(false);
    setModelPickerPosition(undefined);
    setPreflight(undefined);
    setPreflightLoading(false);
    setLicenseBasis("catalog");
    setLicenseAcknowledged(false);
    setSeparateLicenseReference("");
    setInstallProgress(undefined);
    setEndpoint(undefined);
    setError(undefined);
    setHardwareInfo(undefined);
    setHardwareLoading(false);
    setHardwareError(undefined);
  };

  const requestInstallCancellation = async () => {
    if (!downloadBusy || !installCancellable || cancelBusy) return;
    setCancelBusy(true);
    setInstallProgress((current) => current && ({
      ...current,
      message: "Cancelling installation…",
    }));
    try {
      const accepted = await desktopCommands.cancelInstall();
      if (!accepted) {
        setCancelBusy(false);
        setInstallCancellable(false);
        setError("The installation has already finished and can no longer be cancelled.");
      }
    } catch (cancelError) {
      setCancelBusy(false);
      setError(messageFromError(cancelError));
    }
  };

  const cancelWizard = () => {
    if (downloadBusy) {
      void requestInstallCancellation();
      return;
    }
    if (wizardStep === "location" || wizardStep === "ready") {
      confirmWizardClose();
      return;
    }
    setConfirmCloseWizard(true);
  };

  const confirmWizardClose = () => {
    setWizardOpen(false);
    setWizardStep("location");
    setWizardLocation("local");
    setSelectedRemoteTargetId("");
    setRemoteReport(undefined);
    setRemoteCheckLoading(false);
    setRemotePassword("");
    setRemoteTargetDialogOpen(false);
    setRemoteTargetDialogError(undefined);
    setRemoteConfig(emptyRemoteTarget());
    setUseCase("chat");
    setDownloadBusy(false);
    setInstallCancellable(false);
    setCancelBusy(false);
    setStartAfterInstall(true);
    setSelectedModelId("");
    setRecommendations([]);
    setPreflight(undefined);
    setPreflightLoading(false);
    setLicenseBasis("catalog");
    setLicenseAcknowledged(false);
    setSeparateLicenseReference("");
    setInstallProgress(undefined);
    setEndpoint(undefined);
    setError(undefined);
    setConfirmCloseWizard(false);
    setHardwareInfo(undefined);
    setHardwareLoading(false);
    setHardwareError(undefined);
  };

  const dismissWizardClose = () => {
    setConfirmCloseWizard(false);
  };

  const openPublisherUrl = async (url: string) => {
    try {
      const parsed = new URL(url);
      if (parsed.protocol !== "https:") {
        throw new Error("Only secure publisher links can be opened.");
      }
      await openUrl(url);
    } catch (linkError) {
      setError(`Could not open the publisher page. ${messageFromError(linkError)}`);
    }
  };

  const goToNextStep = async () => {
    setError(undefined);
    if (wizardStep === "location") {
      if (wizardLocation === "remote" && !remoteReport?.canContinue) return;
      setWizardStep("hardware");
    } else if (wizardStep === "hardware") {
      setWizardStep("intent");
    } else if (wizardStep === "intent") {
      setBusy(true);
      try {
        const available = await desktopCommands.recommendations(useCase, wizardTargetId ?? "local");
        const result = wizardLocation === "remote"
          ? available.filter((recommendation) => recommendation.runtimeId === "ollama")
          : available;
        if (!result.length) {
          throw new Error("No supported model was found in the active catalog.");
        }
        setRecommendations(result);
        setSelectedModelId(result.find((recommendation) => recommendation.recommended)?.modelId ?? result[0].modelId);
        setPreflight(undefined);
        setWizardStep("suggestion");
      } catch (recommendationError) {
        setError(messageFromError(recommendationError));
      } finally {
        setBusy(false);
      }
    } else if (wizardStep === "suggestion") {
      if (!selectedModelId || !preflight?.canInstall || preflightLoading) return;
      setWizardStep("license");
    } else if (wizardStep === "license" && licenseReviewComplete) {
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
    } else if (wizardStep === "license") {
      setWizardStep("suggestion");
    } else if (wizardStep === "download" && !downloadBusy) {
      setWizardStep("license");
    }
  };

  const startInstall = async () => {
    if (!selectedRecommendation || !preflight?.canInstall || downloadBusy) return;
    setDownloadBusy(true);
    setInstallCancellable(true);
    setCancelBusy(false);
    setBusy(true);
    setError(undefined);
    setInstallProgress({
      modelId: selectedRecommendation.modelId,
      phase: "preparing",
      completedBytes: 0,
      totalBytes: selectedRecommendation.sizeBytes,
      message: "Preparing installation…",
    });

    let unlisten: (() => void) | undefined;
    try {
      unlisten = await desktopCommands.onInstallProgress(setInstallProgress);
      await desktopCommands.install({
        modelId: selectedRecommendation.modelId,
        targetId: wizardTargetId ?? "local",
        licenseBasis,
        licenseReference: licenseBasis === "separate" ? separateLicenseReference.trim() : undefined,
        licenseAcknowledged,
      });
      setInstallCancellable(false);
      if (startAfterInstall) {
        setInstallProgress((current) => current && ({
          ...current,
          phase: "installing",
          message: "Starting the selected model…",
        }));
        await desktopCommands.start(selectedRecommendation.modelId, wizardTargetId ?? "local");
      }
      const details = await desktopCommands.endpoint(wizardTargetId ?? "local");
      setEndpoint(details);
      addInstalledModel(selectedRecommendation, details.model, startAfterInstall);
      setWizardStep("ready");
    } catch (installError) {
      const message = messageFromError(installError);
      if (message.toLowerCase() === "installation cancelled") {
        setInstallProgress((current) => ({
          modelId: current?.modelId ?? selectedRecommendation.modelId,
          phase: "cancelled",
          completedBytes: 0,
          totalBytes: current?.totalBytes ?? selectedRecommendation.sizeBytes,
          message: "Installation cancelled. You can retry when ready.",
        }));
      } else {
        setError(message);
      }
    } finally {
      unlisten?.();
      setDownloadBusy(false);
      setInstallCancellable(false);
      setCancelBusy(false);
      setBusy(false);
    }
  };

  const copyText = async (value: string, feedbackKey: string) => {
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
      setError(messageFromError(copyError));
    }
  };

  const saveTelemetryPreference = async (enabled: boolean) => {
    setTelemetrySaving(true);
    try {
      await desktopCommands.setTelemetryEnabled(enabled);
      setTelemetryEnabled(enabled);
      setTelemetryDialogOpen(false);
    } catch {
      // Telemetry preference and upload failures never enter the application's
      // user-facing error flow. Collection stays disabled unless persistence
      // succeeds.
      setTelemetryEnabled(false);
      setTelemetryDialogOpen(false);
    } finally {
      setTelemetrySaving(false);
    }
  };

  return (
    <div className="app-shell">
      <header className="app-header">
        <a className="brand" href="#" aria-label="Lumen Source home">
          <img className="brand-mark" src={appIcon} alt="" />
          <span>Lumen Source</span>
        </a>
        <span className="local-badge"><i /> Private by design</span>
      </header>

      <main className="main-panel" aria-busy={busy || modelAction !== undefined}>
        {error && (
          <div className="error-banner" role="alert">
            <span>!</span>
            <div><strong>We hit a snag</strong><p>{error}</p></div>
            <button onClick={() => setError(undefined)} aria-label="Dismiss error">×</button>
          </div>
        )}

        <section className={`models-page ${wizardOpen ? "wizard-open" : ""}`}>
          <aside className="sidebar" aria-label="Navigation panel" />

          <div ref={contentPanelRef} className={`content-panel ${detailModel ? "detail-open" : ""}`}>
            {wizardOpen ? (
              <section className="wizard-card" aria-label="Model setup wizard">
                <div className="wizard-header">
                  <div>
                    <span className="eyebrow">Add model</span>
                    <h2>{WIZARD_TITLES[wizardStep]}</h2>
                  </div>
                  <button className="icon-button" type="button" onClick={cancelWizard} aria-label={downloadBusy ? "Cancel installation" : "Cancel wizard"} disabled={downloadBusy && (!installCancellable || cancelBusy)}>
                    ✕
                  </button>
                </div>

                <div className="wizard-steps" aria-label="Wizard steps">
                  {WIZARD_STEPS.map((step, index) => (
                    <span key={step} className={`wizard-step ${wizardStep === step ? "active" : index < WIZARD_STEPS.indexOf(wizardStep) ? "complete" : ""}`}>
                      {index + 1}
                    </span>
                  ))}
                </div>

                {wizardStep === "location" ? (
                  <div className="wizard-body">
                    <p>Run Ollama locally or connect to an existing Ollama service on a remote Linux machine.</p>
                    <div className="choice-list">
                      <button type="button" className={`choice ${wizardLocation === "local" ? "selected" : ""}`} onClick={() => { setWizardLocation("local"); setRemoteReport(undefined); setHardwareInfo(undefined); setHardwareError(undefined); }}>
                        <span className="choice-icon">L</span>
                        <div>
                          <strong>Local runtime</strong>
                          <small>Run the model on this machine.</small>
                        </div>
                        <i />
                      </button>
                      <button type="button" className={`choice ${wizardLocation === "remote" ? "selected" : ""}`} onClick={() => { setWizardLocation("remote"); setHardwareInfo(undefined); setHardwareError(undefined); }}>
                        <span className="choice-icon">R</span>
                        <div>
                          <strong>Remote runtime</strong>
                          <small>Connect Linux to Linux through your existing SSH access.</small>
                        </div>
                        <i />
                      </button>
                    </div>
                    <p className="runtime-availability-note">
                      <strong>Ollama:</strong> LumenSource includes Ollama for local deployments. For remote deployments, Ollama must already be installed and running on the target machine.
                    </p>
                    {wizardLocation === "remote" && (
                      <div className="remote-target-panel">
                        <div className="remote-scope-note">
                          <strong>Linux remote preview</strong>
                          <span>LumenSource checks SSH, target hardware and storage, and the remote Ollama service. It does not install remote services.</span>
                        </div>
                        <div className="remote-target-picker">
                          <label>
                            <span>Target machine</span>
                            <select value={selectedRemoteTargetId} onChange={(event) => {
                              setSelectedRemoteTargetId(event.target.value);
                              setRemoteReport(undefined);
                              setRemotePassword("");
                              setHardwareInfo(undefined);
                              setHardwareError(undefined);
                            }}>
                              <option value="">Select a target machine</option>
                              {remoteTargets.map((target) => (
                                <option value={target.targetId} key={target.targetId}>
                                  {target.config.name.trim()
                                    ? `${target.targetName} — ${target.config.host}`
                                    : target.targetName}
                                </option>
                              ))}
                            </select>
                          </label>
                          <button className="secondary-button" type="button" onClick={openRemoteTargetDialog}>
                            Add target machine
                          </button>
                        </div>
                        {!remoteTargets.length && (
                          <p className="remote-target-empty">No target machines have been saved yet.</p>
                        )}
                        {selectedRemoteTarget && (
                          <>
                            <div className="remote-target-summary">
                              <div>
                                <strong>{selectedRemoteTarget.targetName}</strong>
                                <span>{selectedRemoteTarget.config.username}@{selectedRemoteTarget.config.host}:{selectedRemoteTarget.config.port}</span>
                              </div>
                              <small>{selectedRemoteTarget.config.authentication === "key"
                                ? "SSH key or agent"
                                : "Password authentication"}</small>
                            </div>
                            {selectedRemoteTarget.config.authentication === "password" && (
                              <label className="remote-password-field">
                                <span>SSH password</span>
                                <input type="password" value={remotePassword} onChange={(event) => { setRemotePassword(event.target.value); setRemoteReport(undefined); }} autoComplete="current-password" />
                                <small>Used only for this connection attempt. LumenSource never saves it.</small>
                              </label>
                            )}
                            <button className="secondary-button remote-check-button" type="button" onClick={() => void checkRemoteTarget()} disabled={remoteCheckLoading || (selectedRemoteTarget.config.authentication === "password" && !remotePassword)}>
                              {remoteCheckLoading ? "Checking connection…" : "Check remote connection"}
                            </button>
                          </>
                        )}
                        {remoteReport && (
                          <div className="remote-checks" aria-live="polite">
                            {remoteReport.checks.map((check) => (
                              <div className={`remote-check ${check.status}`} key={check.id}>
                                <span>{check.status === "pass" ? "✓" : "!"}</span>
                                <div><strong>{check.label}</strong><p>{check.detail}</p>{check.guidance && <small>{check.guidance}</small>}</div>
                              </div>
                            ))}
                          </div>
                        )}
                      </div>
                    )}
                    <div className="actions">
                      <button className="primary-button" type="button" onClick={goToNextStep} disabled={wizardLocation === "remote" && !remoteReport?.canContinue}>
                        Continue
                      </button>
                    </div>
                  </div>
                ) : wizardStep === "hardware" ? (
                  <div className="wizard-body">
                    <p>{wizardLocation === "remote"
                      ? `Here is the hardware detected on ${remoteReport?.targetName ?? "the remote target"} before the model is prepared.`
                      : "Here is a detailed hardware summary before the model is prepared."}</p>
                    <div className="hardware-summary">
                      <div className="scan-card">
                        <div className="scan-visual"><span>⌘</span><i /><i /></div>
                        <div>
                          <strong>
                            {hardwareLoading
                              ? "Detecting hardware..."
                              : hardwareError
                                ? "Hardware detection failed"
                                : wizardLocation === "remote" ? "Remote hardware detected" : "Hardware detected"}
                          </strong>
                          <p>
                            {hardwareLoading
                              ? wizardLocation === "remote" ? "Scanning the target system..." : "Scanning your system..."
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
                              <li><span>Frequency</span><strong>{hardwareInfo.cpuFrequencyMhz ? `${hardwareInfo.cpuFrequencyMhz} MHz` : "Unavailable"}</strong></li>
                            </ul>
                          </div>
                          <div className="hardware-card">
                            <h3>Memory</h3>
                            <ul>
                              <li><span>Capacity</span><strong>{formatBytes(hardwareInfo.memoryBytes)}</strong></li>
                              <li><span>Type</span><strong>{hardwareInfo.memoryKind ?? "Unavailable"}</strong></li>
                              <li><span>Speed</span><strong>{hardwareInfo.memorySpeedMts ? `${hardwareInfo.memorySpeedMts} MT/s` : "Unavailable"}</strong></li>
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
                      {hardwareError && (
                        <button className="secondary-button" type="button" onClick={() => {
                          setHardwareError(undefined);
                          setHardwareInfo(undefined);
                        }}>
                          Try detection again
                        </button>
                      )}
                    </div>
                    <div className="actions">
                      <button className="back-button" type="button" onClick={goToPreviousStep}>
                        ← Back
                      </button>
                      <button className="primary-button" type="button" onClick={goToNextStep} disabled={hardwareLoading || !hardwareInfo}>
                        Continue
                      </button>
                    </div>
                  </div>
                ) : wizardStep === "intent" ? (
                  <div className="wizard-body">
                    <p>Select the main way you plan to use the model. This helps tailor the recommendation.</p>
                    <div className="choice-list">
                      <button type="button" className={`choice ${useCase === "code" ? "selected" : ""}`} onClick={() => setUseCase("code")}>
                        <span className="choice-icon">⌨</span>
                        <div>
                          <strong>Coding assistant</strong>
                          <small>Good for code generation, debugging, and refactors.</small>
                        </div>
                        <i />
                      </button>
                      <button type="button" className={`choice ${useCase === "chat" ? "selected" : ""}`} onClick={() => setUseCase("chat")}>
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
                      <button className="primary-button" type="button" onClick={goToNextStep} disabled={busy}>
                        {busy ? "Finding compatible models…" : "Continue"}
                      </button>
                    </div>
                  </div>
                ) : wizardStep === "suggestion" ? (
                  <div className="wizard-body">
                    <p>{wizardLocation === "remote"
                      ? "Use the Ollama model recommended for the remote target hardware and selected use case, or choose another compatible model."
                      : "Use the model recommended for your hardware and selected use case, or choose any model from the active catalog."}</p>
                    <div className="recommendation-picker">
                      {recommendedModel && (
                        <button
                          type="button"
                          className={`recommended-choice ${selectedModelId === recommendedModel.modelId ? "selected" : ""}`}
                          onClick={() => {
                            setSelectedModelId(recommendedModel.modelId);
                            setModelPickerOpen(false);
                            setModelPickerPosition(undefined);
                          }}
                        >
                          <span className="recommended-choice-copy">
                            <b>Recommended</b>
                            <strong>{recommendedModel.name}</strong>
                            <em>{recommendedModel.provider}</em>
                          </span>
                          <small>{wizardLocation === "remote" ? "Best match for the remote target and selected use case" : "Best match from the compatibility and use-case ranking"}</small>
                          <i className="radio-dot" />
                        </button>
                      )}
                      <div className="catalog-model-picker" data-catalog-model-picker>
                        <span>Choose from all available models</span>
                        <button
                          className={`catalog-picker-trigger ${modelPickerOpen ? "open" : ""}`}
                          type="button"
                          aria-haspopup="listbox"
                          aria-expanded={modelPickerOpen}
                          onClick={(event) => {
                            if (modelPickerOpen) {
                              setModelPickerOpen(false);
                              setModelPickerPosition(undefined);
                              return;
                            }
                            const rect = event.currentTarget.getBoundingClientRect();
                            const estimatedHeight = Math.min(280, recommendations.length * 56 + 12);
                            const opensUpward = window.innerHeight - rect.bottom < estimatedHeight + 12 && rect.top > estimatedHeight;
                            setModelPickerPosition({
                              top: Math.max(8, opensUpward ? rect.top - estimatedHeight - 6 : rect.bottom + 6),
                              left: Math.max(8, Math.min(rect.left, window.innerWidth - rect.width - 8)),
                              width: rect.width,
                            });
                            setModelPickerOpen(true);
                          }}
                        >
                          <div className="catalog-trigger-model">
                            <span>
                              <strong>{selectedRecommendation?.name ?? "Select a model"}</strong>
                              {selectedRecommendation && (
                                <small>{selectedRecommendation.provider} · {selectedRecommendation.recommended ? "Recommended" : selectedRecommendation.compatible ? selectedRecommendation.fit : "Incompatible"}</small>
                              )}
                            </span>
                          </div>
                          <i aria-hidden="true" />
                        </button>
                        {modelPickerOpen && modelPickerPosition && createPortal(
                          <div
                            className="catalog-model-menu"
                            data-catalog-model-picker
                            role="listbox"
                            aria-label="Available models"
                            style={{ top: `${modelPickerPosition.top}px`, left: `${modelPickerPosition.left}px`, width: `${modelPickerPosition.width}px` }}
                          >
                            {recommendations.map((recommendation) => (
                              <button
                                key={recommendation.modelId}
                                type="button"
                                role="option"
                                aria-selected={selectedModelId === recommendation.modelId}
                                className={`catalog-model-option ${selectedModelId === recommendation.modelId ? "selected" : ""} ${recommendation.compatible ? "" : "incompatible"}`}
                                onClick={() => {
                                  setSelectedModelId(recommendation.modelId);
                                  setModelPickerOpen(false);
                                  setModelPickerPosition(undefined);
                                }}
                              >
                                <div className="catalog-model-identity">
                                  <span><strong>{recommendation.name}</strong><small>{recommendation.provider} · {recommendation.modelId}</small></span>
                                </div>
                                <em>{recommendation.recommended ? "Recommended" : recommendation.compatible ? recommendation.fit : "Incompatible"}</em>
                              </button>
                            ))}
                          </div>,
                          document.body,
                        )}
                      </div>
                    </div>
                    {selectedRecommendation && (
                      <article className={`model-card model-detail-card ${selectedRecommendation.compatible ? "" : "incompatible"}`}>
                        <div className="model-top">
                          <span className={`fit ${selectedRecommendation.fit}`}>
                            {selectedRecommendation.recommended ? "Recommended" : selectedRecommendation.fit}
                          </span>
                        </div>
                        <div className="publisher-identity">
                          <small>Publisher</small>
                          <strong>{selectedRecommendation.provider}</strong>
                        </div>
                        <h3>{selectedRecommendation.name}</h3>
                        <div className="model-version">
                          <span>Version</span>
                          <code>{selectedRecommendation.modelId}</code>
                        </div>
                        <p>{selectedRecommendation.description}</p>
                        <div className="model-stats">
                          <span><b>{formatBytes(selectedRecommendation.sizeBytes)}</b>Storage</span>
                          <span><b>{selectedRecommendation.contextWindow.toLocaleString()}</b>Context</span>
                          <span><b>{selectedRecommendation.version}</b>Runtime</span>
                        </div>
                        <h4>{selectedRecommendation.compatible ? "Why this model" : "Compatibility issues"}</h4>
                        <ul className="ranking-reasons">
                          {selectedRecommendation.reasons.map((reason) => (
                            <li key={reason}>{selectedRecommendation.compatible ? "✓" : "!"} {reason}</li>
                          ))}
                        </ul>
                        <div className="inline-requirements" aria-live="polite">
                          <h5>Installation checks</h5>
                          {preflightLoading ? (
                            <div className="requirements-loading"><span className="model-control-spinner" /> Checking this machine…</div>
                          ) : (
                            <div className="check-list compact">
                              {preflight?.checks.map((item) => (
                                <div className={`check ${item.status}`} key={item.id}>
                                  <span className="check-status" role="img" aria-label={`${item.label}: ${item.status}`}>
                                    {item.status === "pass" ? "✓" : item.status === "warning" ? "~" : "!"}
                                  </span>
                                  <div>
                                    <strong>{item.label}</strong>
                                    <small>{item.id === "storage" && preflight
                                      ? `Required: ${formatBytes(preflight.requiredBytes)} free space. Available: ${formatBytes(preflight.availableBytes)}.`
                                      : item.detail}</small>
                                  </div>
                                </div>
                              ))}
                            </div>
                          )}
                        </div>
                      </article>
                    )}
                    <div className="actions">
                      <button className="back-button" type="button" onClick={goToPreviousStep}>
                        ← Back
                      </button>
                      <button className="primary-button" type="button" onClick={goToNextStep} disabled={!selectedModelId || preflightLoading || !preflight?.canInstall}>
                        {preflightLoading ? "Checking requirements…" : preflight?.canInstall ? "Continue to license" : "Requirements not met"}
                      </button>
                    </div>
                  </div>
                ) : wizardStep === "license" ? (
                  <div className="wizard-body">
                    <p>Review the permissions and conditions that apply to {selectedRecommendation?.name}. The linked license controls if this summary differs from it.</p>
                    <p className="license-legal-notice">LumenSource is not the model licensor and does not grant rights to use or redistribute this model. Catalog information is a summary only; the publisher’s current terms control.</p>
                    {selectedRecommendation && (
                      <>
                        <article className={`license-card ${selectedRecommendation.license.commercialUse === "not-permitted" ? "restricted" : ""}`}>
                          <div className="license-heading">
                            <div>
                              <div className="publisher-identity">
                                <small>Publisher</small>
                                <strong>{selectedRecommendation.provider}</strong>
                              </div>
                              <span className="license-classification">{formatLegalValue(selectedRecommendation.license.classification)}</span>
                              <h3>{selectedRecommendation.license.name}</h3>
                            </div>
                            {selectedRecommendation.license.reviewedAt && <small>Reviewed {selectedRecommendation.license.reviewedAt}</small>}
                          </div>
                          <p>{selectedRecommendation.license.summary || "Review the complete license before using or distributing this model."}</p>
                          <div className="license-permissions">
                            <span className={selectedRecommendation.license.commercialUse === "not-permitted" ? "denied" : ""}>
                              <small>Commercial use</small><b>{formatLegalValue(selectedRecommendation.license.commercialUse)}</b>
                            </span>
                            <span><small>Redistribution</small><b>{formatLegalValue(selectedRecommendation.license.redistribution)}</b></span>
                            <span><small>Derivatives</small><b>{formatLegalValue(selectedRecommendation.license.derivatives)}</b></span>
                          </div>
                          <div className="license-compliance">
                            <span><small>Attribution</small><b>{formatLegalValue(selectedRecommendation.license.attribution)}</b></span>
                            <span><small>License copy</small><b>{formatLegalValue(selectedRecommendation.license.licenseText)}</b></span>
                            <span><small>NOTICE file</small><b>{formatLegalValue(selectedRecommendation.license.notice)}</b></span>
                          </div>
                          {selectedRecommendation.license.geographicRestrictions.length > 0 && (
                            <section className="license-alert">
                              <h4>Geographic restrictions</h4>
                              <ul>{selectedRecommendation.license.geographicRestrictions.map((item) => <li key={item}>{item}</li>)}</ul>
                            </section>
                          )}
                          <div className="license-conditions">
                            <section>
                              <h4>Conditions and obligations</h4>
                              {selectedRecommendation.license.obligations.length > 0
                                ? <ul>{selectedRecommendation.license.obligations.map((item) => <li key={item}>{item}</li>)}</ul>
                                : <p>No additional obligations are summarized in the catalog.</p>}
                            </section>
                            <section>
                              <h4>Restrictions</h4>
                              {selectedRecommendation.license.restrictions.length > 0
                                ? <ul>{selectedRecommendation.license.restrictions.map((item) => <li key={item}>{item}</li>)}</ul>
                                : <p>No additional restrictions are summarized in the catalog.</p>}
                            </section>
                          </div>
                          <div className="license-links">
                            {selectedRecommendation.license.url && <button type="button" onClick={() => void openPublisherUrl(selectedRecommendation.license.url!)}>Read the full license ↗</button>}
                            {selectedRecommendation.license.usagePolicyUrl && <button type="button" onClick={() => void openPublisherUrl(selectedRecommendation.license.usagePolicyUrl!)}>Read the usage policy ↗</button>}
                          </div>
                        </article>

                        <fieldset className="license-basis">
                          <legend>Which permission are you relying on?</legend>
                          <label className={licenseBasis === "catalog" ? "selected" : ""}>
                            <input type="radio" name="license-basis" checked={licenseBasis === "catalog"} onChange={() => { setLicenseBasis("catalog"); setLicenseAcknowledged(false); }} />
                            <span><strong>Use the cataloged license</strong><small>The terms and restrictions shown above apply.</small></span>
                          </label>
                          <label className={licenseBasis === "separate" ? "selected" : ""}>
                            <input type="radio" name="license-basis" checked={licenseBasis === "separate"} onChange={() => { setLicenseBasis("separate"); setLicenseAcknowledged(false); }} />
                            <span><strong>I have a separate license or authorization</strong><small>For example, a commercial agreement issued by the model publisher.</small></span>
                          </label>
                        </fieldset>

                        {licenseBasis === "separate" ? (
                          <div className="separate-license-form">
                            <label>
                              <span>Agreement reference or authorization note</span>
                              <textarea
                                rows={3}
                                value={separateLicenseReference}
                                onChange={(event) => setSeparateLicenseReference(event.target.value)}
                                placeholder="Example: Commercial agreement — internal reference LS-2026-014"
                              />
                              <small>Stored locally with the installed model. Do not paste license keys, confidential contract text, or personal data.</small>
                            </label>
                            <label className="license-confirmation">
                              <input type="checkbox" checked={licenseAcknowledged} onChange={(event) => setLicenseAcknowledged(event.target.checked)} />
                              <span>I confirm that this separate agreement authorizes my intended use. LumenSource has not validated it.</span>
                            </label>
                          </div>
                        ) : selectedRecommendation.license.requiresUserAcceptance ? (
                          <label className="license-confirmation required">
                            <input type="checkbox" checked={licenseAcknowledged} onChange={(event) => setLicenseAcknowledged(event.target.checked)} />
                            <span>I have read and agree to the linked license, restrictions, and usage policy.</span>
                          </label>
                        ) : (
                          <p className="license-information-note">This license is informational and does not require an additional acknowledgment in LumenSource. Redistribution obligations may still apply.</p>
                        )}
                      </>
                    )}
                    <div className="actions">
                      <button className="back-button" type="button" onClick={goToPreviousStep}>
                        ← Back
                      </button>
                      <button className="primary-button" type="button" onClick={goToNextStep} disabled={!licenseReviewComplete}>
                        {licenseReviewComplete ? "Continue to installation" : "Review and confirm"}
                      </button>
                    </div>
                  </div>
                ) : wizardStep === "download" ? (
                  <div className="wizard-body">
                    <p>{wizardLocation === "remote"
                      ? `Lumen Source will ask the existing Ollama service on ${remoteReport?.targetName ?? "the remote target"} to pull the model, then optionally preload it.`
                      : "Lumen Source will prepare the selected runtime, install the model, verify any direct downloads, and optionally start the model."}</p>
                    <label className="install-option">
                      <input
                        type="checkbox"
                        checked={startAfterInstall}
                        onChange={(event) => setStartAfterInstall(event.target.checked)}
                        disabled={downloadBusy}
                      />
                      <span>
                        <strong>Start model after installation</strong>
                        <small>When disabled, the model is downloaded but is not preloaded into memory. You can start it later from the models page.</small>
                      </span>
                    </label>
                    <div className="install-stage">
                      <div className="model-orb"><span>⬇</span><i /></div>
                      <h3>{downloadBusy ? "Installation in progress" : `Install ${selectedRecommendation?.name ?? "model"}`}</h3>
                      <p>{installProgress?.message ?? "Start the verified installation when you are ready."}</p>
                      {installProgress && (
                        <>
                          <div className="progress-track" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(progressPercent(installProgress))}>
                            <i style={{ width: `${progressPercent(installProgress)}%` }} />
                          </div>
                          <div className="progress-meta">
                            <span>{installProgress.phase}</span>
                            {installProgress.currentItem && installProgress.totalItems && (
                              <span className="progress-items">Download item {installProgress.currentItem} of {installProgress.totalItems}</span>
                            )}
                            <b>{Math.round(progressPercent(installProgress))}%</b>
                          </div>
                        </>
                      )}
                    </div>
                    <div className="actions">
                      {downloadBusy ? (
                        <button className="secondary-button" type="button" onClick={() => void requestInstallCancellation()} disabled={!installCancellable || cancelBusy}>
                          {cancelBusy ? "Cancelling…" : "Cancel installation"}
                        </button>
                      ) : (
                        <button className="back-button" type="button" onClick={goToPreviousStep}>
                          ← Back
                        </button>
                      )}
                      <button className="primary-button" type="button" onClick={startInstall} disabled={downloadBusy || !preflight?.canInstall}>
                        {downloadBusy ? "Installing…" : startAfterInstall ? "Install and start" : "Install"}
                      </button>
                    </div>
                  </div>
                ) : (
                  <div className="wizard-body">
                    <p>{selectedIsDummy
                      ? "The dummy model lifecycle completed. It simulates runtime state and does not provide inference."
                      : startAfterInstall
                      ? wizardLocation === "remote"
                        ? `The model is installed on ${remoteReport?.targetName ?? "the remote target"} and available through the SSH tunnel.`
                        : "The model is installed, running locally, and ready for external tools."
                      : "The model is installed but has not been loaded into memory."}</p>
                    <div className="success-mark">✓</div>
                    <div className={`runtime-hero ${startAfterInstall ? "" : "stopped"}`}>
                      <div><span className="pulse" /> {selectedIsDummy
                        ? startAfterInstall ? "Test state active" : "Test model installed"
                        : startAfterInstall ? "Model running" : "Model not loaded"}</div>
                      <h3>{selectedRecommendation?.name}</h3>
                      <p>{selectedIsDummy
                        ? "These connection values are placeholders for testing the Ready screen and copy controls."
                        : startAfterInstall
                        ? "Use these OpenAI-compatible connection details in your preferred application."
                        : "The runtime uses this endpoint; start the model for an explicit readiness check."}</p>
                    </div>
                    <div className="endpoint-card">
                      <div>
                        <span>Base URL</span>
                        <div className="copy-value"><code>{endpoint?.baseUrl}</code><button className={copiedField === "ready-base" ? "copied" : ""} type="button" aria-live="polite" onClick={() => endpoint && void copyText(endpoint.baseUrl, "ready-base")}>{copiedField === "ready-base" ? "✓ Copied" : "Copy"}</button></div>
                      </div>
                      <div>
                        <span>{endpoint ? inferenceLabel(endpoint) : "Inference endpoint"}</span>
                        <div className="copy-value"><code>{endpoint ? inferenceUrl(endpoint) : undefined}</code><button className={copiedField === "ready-inference" ? "copied" : ""} type="button" aria-live="polite" onClick={() => endpoint && void copyText(inferenceUrl(endpoint), "ready-inference")}>{copiedField === "ready-inference" ? "✓ Copied" : "Copy"}</button></div>
                      </div>
                      <div>
                        <span>Model</span>
                        <div className="copy-value"><code>{endpoint?.model}</code><button className={copiedField === "ready-model" ? "copied" : ""} type="button" aria-live="polite" onClick={() => endpoint && void copyText(endpoint.model, "ready-model")}>{copiedField === "ready-model" ? "✓ Copied" : "Copy"}</button></div>
                      </div>
                      <div>
                        <span>API key</span>
                        <div className="copy-value"><code>{endpoint?.apiKeyRequired ? "Required" : "Not required"}</code></div>
                      </div>
                    </div>
                    <div className="actions">
                      <button className="primary-button" type="button" onClick={confirmWizardClose}>Finish</button>
                    </div>
                  </div>
                )}
              </section>
            ) : detailModel ? (
              <section className="model-detail-page" aria-label={`${detailModel.name} details`}>
                <div className="models-toolbar detail-toolbar">
                  <div>
                    <span className="eyebrow">Model details</span>
                    <div className="model-title-line">
                      <h1>{detailModel.name}</h1>
                      <button className="back-to-models" type="button" onClick={() => setDetailModelId(undefined)}>← Back to model list</button>
                    </div>
                  </div>
                  <div className="detail-runtime-actions">
                    <span className={`runtime-state ${detailModel.running ? "running" : "stopped"}`}><i /> {detailModel.running ? "Running" : "Stopped"}</span>
                    <button
                      className="secondary-button detail-runtime-button"
                      type="button"
                      onClick={() => void toggleRunning(detailModel.id)}
                      disabled={detailModel.managed === false || modelAction !== undefined}
                    >
                      {modelAction?.id === detailModel.id ? (
                        <><span className="model-control-spinner" /> {modelAction.action === "starting" ? "Starting…" : "Stopping…"}</>
                      ) : detailModel.running ? (
                        <><span className="model-control-icon stop" /> Stop model</>
                      ) : (
                        <><span className="model-control-icon start" /> Start model</>
                      )}
                    </button>
                  </div>
                </div>

                <nav className="detail-tabs" aria-label="Model detail sections">
                  <button type="button" className={detailTab === "chat" ? "active" : ""} onClick={() => setDetailTab("chat")}>Chat</button>
                  <button type="button" className={detailTab === "logs" ? "active" : ""} onClick={() => setDetailTab("logs")}>Logs</button>
                  <button type="button" className={detailTab === "performance" ? "active" : ""} onClick={() => setDetailTab("performance")}>Performance</button>
                  <button type="button" className={detailTab === "api" ? "active" : ""} onClick={() => setDetailTab("api")}>API</button>
                </nav>

                {detailTab === "logs" ? (
                  <section className="detail-section" aria-labelledby="model-logs-title">
                    <div className="detail-section-header">
                      <div>
                        <h2 id="model-logs-title">Lifecycle logs</h2>
                        <p>Installation and model-state activity recorded by Lumen Source. Prompts, generated responses, and Ollama server stdout are intentionally not captured here.</p>
                      </div>
                      <div className="detail-section-actions">
                        <button className="secondary-button" type="button" onClick={clearLogs} disabled={detailModel.logs.length === 0}>Clear</button>
                        <button className="secondary-button" type="button" aria-live="polite" onClick={() => void copyText(detailModel.logs.join("\n"), `logs-${detailModel.id}`)} disabled={detailModel.logs.length === 0}>{copiedField === `logs-${detailModel.id}` ? "✓ Copied" : "Copy logs"}</button>
                      </div>
                    </div>
                    {detailModel.logs.length > 0 ? (
                      <ol className="log-viewer full-log-viewer">
                        {detailModel.logs.map((log, index) => <li key={`${index}-${log}`}>{log}</li>)}
                      </ol>
                    ) : (
                      <div className="details-empty full-detail-empty">No Lumen Source lifecycle events have been recorded for this entry. A running model does not generate log entries simply by serving inference requests.</div>
                    )}
                  </section>
                ) : detailTab === "performance" ? (
                  <section className="detail-section" aria-labelledby="model-performance-title">
                    <div className="detail-section-header">
                      <div>
                        <h2 id="model-performance-title">Model resource allocation</h2>
                        <p>Updates every two seconds from Ollama's <code>/api/ps</code> data for <code>{detailModel.runtimeModelId ?? detailModel.modelId}</code>. Ollama does not expose reliable per-model CPU or GPU processor utilization.</p>
                      </div>
                    </div>
                    {performanceError && <div className="inline-error" role="alert">{performanceError}</div>}
                    {performanceLoading && !performanceSnapshot ? (
                      <div className="details-empty full-detail-empty">Sampling utilization…</div>
                    ) : performanceSnapshot ? (
                      <>
                        <PerformanceChart history={performanceHistory} />
                        <div className="performance-status detail-performance-status">
                          <span className={`led ${performanceSnapshot.state === "running" ? "on" : "off"}`} />
                          <strong>{performanceSnapshot.state === "running" ? "Model loaded" : performanceSnapshot.state === "unavailable" ? "Runtime unavailable" : "Model stopped"}</strong>
                          <small>Sampled at {new Date(performanceSnapshot.sampledAtUnixMs).toLocaleTimeString()}</small>
                        </div>
                        <div className="performance-grid full-performance-grid">
                          <div className="performance-card">
                            <span>Total model memory</span>
                            <strong>{formatBytes(performanceSnapshot.allocatedMemoryBytes)}</strong>
                            <small>Memory reserved for this loaded model</small>
                          </div>
                          <div className="performance-card">
                            <span>Model allocation in system RAM</span>
                            <strong>{formatBytes(performanceSnapshot.allocatedSystemMemoryBytes)}</strong>
                            <small>Model memory kept in ordinary RAM instead of GPU VRAM; this is not CPU utilization</small>
                          </div>
                          <div className="performance-card">
                            <span>GPU VRAM</span>
                            <strong>{formatBytes(performanceSnapshot.allocatedVramBytes)}</strong>
                            <small>Model allocation placed on the GPU</small>
                          </div>
                          <div className="performance-card">
                            <span>Context capacity</span>
                            <strong>{performanceSnapshot.contextLength?.toLocaleString() ?? "Unavailable"}</strong>
                            <small>{performanceSnapshot.contextLength === undefined ? "Not reported by this Ollama version" : "Tokens configured for the loaded model"}</small>
                          </div>
                        </div>
                      </>
                    ) : null}
                  </section>
                ) : detailTab === "chat" ? (
                  <section className="detail-section chat-section" aria-labelledby="model-chat-title">
                    <div className="detail-section-header">
                      <div>
                        <h2 id="model-chat-title">Chat with {detailModel.name}</h2>
                        <p>This conversation stays in application memory and is not added to lifecycle logs or saved when LumenSource closes.</p>
                      </div>
                      <div className="detail-section-actions">
                        <button className="secondary-button" type="button" onClick={clearChat} disabled={detailChatMessages.length === 0 || chatBusyModelId === detailModel.id}>Clear</button>
                      </div>
                    </div>
                    {chatError && <div className="inline-error" role="alert">{chatError}</div>}
                    {modelEndpointError && <div className="inline-error" role="alert">{modelEndpointError}</div>}
                    {!detailModel.running && (
                      <div className="api-notice warning"><strong>Model stopped</strong><span>Start this model before sending a message.</span></div>
                    )}
                    {modelEndpointLoading ? (
                      <div className="details-empty full-detail-empty">Preparing the local chat connection…</div>
                    ) : detailModel.managed === false ? (
                      <div className="details-empty full-detail-empty">Chat is unavailable for models that are not managed by this catalog.</div>
                    ) : modelEndpoint && !modelEndpoint.apiAvailable ? (
                      <div className="details-empty full-detail-empty">This test runtime does not expose an inference API.</div>
                    ) : modelEndpoint && !modelEndpoint.chatAvailable ? (
                      <div className="details-empty full-detail-empty">{modelEndpoint.embeddingsAvailable
                        ? "This model produces embeddings for search and retrieval; it does not generate conversational text."
                        : "This model supports completion-style inference rather than multi-turn chat. See the API tab for an example request."}</div>
                    ) : modelEndpoint ? (
                      <>
                        <div className="chat-transcript" ref={chatTranscriptRef} role="log" aria-live="polite" aria-label={`Conversation with ${detailModel.name}`}>
                          {detailChatMessages.length === 0 ? (
                            <div className="chat-empty">
                              <strong>Start a private conversation</strong>
                              <span>Messages are sent only to <code>{detailModel.runtimeModelId}</code> through {detailModel.location === "remote" ? `the encrypted tunnel to ${detailModel.targetName ?? "the remote target"}` : "the local Ollama runtime"}.</span>
                            </div>
                          ) : detailChatMessages.map((message, index) => (
                            <article className={`chat-message ${message.role}`} key={`${message.role}-${index}`}>
                              <span>{message.role === "user" ? "You" : detailModel.name}</span>
                              {message.content ? <p>{message.content}</p> : <div className="chat-generating"><span className="model-control-spinner" /> Generating…</div>}
                            </article>
                          ))}
                        </div>
                        <div className="chat-composer">
                          <textarea
                            rows={3}
                            maxLength={65_536}
                            value={chatDraft}
                            onChange={(event) => setChatDraft(event.target.value)}
                            onKeyDown={handleChatKeyDown}
                            placeholder={detailModel.running ? "Message this model…" : "Start the model to chat"}
                            aria-label="Chat message"
                            disabled={!detailModel.running || chatBusyModelId === detailModel.id}
                          />
                          <div>
                            <small>Enter to send · Shift+Enter for a new line</small>
                            {chatBusyModelId === detailModel.id ? (
                              <button className="secondary-button" type="button" onClick={() => void stopChat()} disabled={chatCancelBusy}>{chatCancelBusy ? "Stopping…" : "Stop"}</button>
                            ) : (
                              <button className="primary-button" type="button" onClick={() => void sendChatMessage()} disabled={!detailModel.running || !chatDraft.trim()}>Send</button>
                            )}
                          </div>
                        </div>
                      </>
                    ) : null}
                  </section>
                ) : (
                  <section className="detail-section" aria-labelledby="model-api-title">
                    <div className="detail-section-header">
                      <div>
                        <h2 id="model-api-title">OpenAI-compatible API</h2>
                        <p>Use this model identifier with the {detailModel.location === "remote" ? "local end of the SSH tunnel" : "local Ollama endpoint"}. The endpoint belongs to the runtime, while the <code>model</code> value selects this specific model.</p>
                      </div>
                    </div>
                    {modelEndpointError && <div className="inline-error" role="alert">{modelEndpointError}</div>}
                    {modelEndpointLoading ? (
                      <div className="details-empty full-detail-empty">Preparing API connection details…</div>
                    ) : modelEndpoint ? (
                      <>
                        {!modelEndpoint.apiAvailable && (
                          <div className="api-notice warning"><strong>No inference API</strong><span>The dummy model exposes placeholder values for UI testing only.</span></div>
                        )}
                        <div className="endpoint-card api-endpoint-card">
                          <div>
                            <span>Base URL</span>
                            <div className="copy-value"><code>{modelEndpoint.baseUrl}</code><button className={copiedField === `api-base-${detailModel.id}` ? "copied" : ""} type="button" aria-live="polite" onClick={() => void copyText(modelEndpoint.baseUrl, `api-base-${detailModel.id}`)}>{copiedField === `api-base-${detailModel.id}` ? "✓ Copied" : "Copy"}</button></div>
                          </div>
                          <div>
                            <span>{inferenceLabel(modelEndpoint)}</span>
                            <div className="copy-value"><code>{inferenceUrl(modelEndpoint)}</code><button className={copiedField === `api-inference-${detailModel.id}` ? "copied" : ""} type="button" aria-live="polite" onClick={() => void copyText(inferenceUrl(modelEndpoint), `api-inference-${detailModel.id}`)}>{copiedField === `api-inference-${detailModel.id}` ? "✓ Copied" : "Copy"}</button></div>
                          </div>
                          <div>
                            <span>Model</span>
                            <div className="copy-value"><code>{modelEndpoint.model}</code><button className={copiedField === `api-model-${detailModel.id}` ? "copied" : ""} type="button" aria-live="polite" onClick={() => void copyText(modelEndpoint.model, `api-model-${detailModel.id}`)}>{copiedField === `api-model-${detailModel.id}` ? "✓ Copied" : "Copy"}</button></div>
                          </div>
                          <div>
                            <span>API key</span>
                            <div className="copy-value"><code>{modelEndpoint.apiKeyRequired ? "Required" : "Not required for loopback access"}</code></div>
                          </div>
                        </div>
                        {modelEndpoint.apiAvailable && (
                          <div className="api-example-card">
                            <div>
                              <h3>Example request</h3>
                              <button className="secondary-button" type="button" aria-live="polite" onClick={() => void copyText(curlExample(modelEndpoint), `api-example-${detailModel.id}`)}>{copiedField === `api-example-${detailModel.id}` ? "✓ Copied" : "Copy cURL"}</button>
                            </div>
                            <pre tabIndex={0} aria-label="Example cURL request"><code>{curlExample(modelEndpoint)}</code></pre>
                          </div>
                        )}
                        <div className="api-notice">
                          <strong>{detailModel.running ? "Model is preloaded" : "Model is not preloaded"}</strong>
                          <span>{detailModel.running ? "Requests can use the currently loaded model." : "Ollama may load an installed model when the first request arrives, which can make that request slower."} {detailModel.location === "remote" ? "Lumen Source must remain open to maintain this SSH tunnel." : "Lumen Source does not need to remain open while the independent Ollama service is available."}</span>
                        </div>
                      </>
                    ) : null}
                  </section>
                )}
              </section>
            ) : (
              <>
                <div className="models-toolbar">
                  <div>
                    <span className="eyebrow">Models</span>
                    <h1>Models</h1>
                  </div>
                  <button className="primary-button" type="button" onClick={openWizard}>
                    Add model
                  </button>
                </div>

                <div className="models-table" role="table" aria-label="Installed models">
                  {runningModels.length > 0 ? runningModels.map((model) => (
                    <div className="models-table-row" role="row" key={model.id}>
                      <button className="model-row-button" type="button" onClick={() => openLogs(model)} aria-label={`Open details for ${model.name}`}>
                        <div className="model-main">
                          <span className={`led ${model.running ? "on" : "off"}`} aria-label={model.running ? "Running" : "Stopped"} />
                          <div>
                            <strong role="cell">{model.name}</strong>
                            <p>{model.modelName} · {model.version} · {model.location === "remote" ? model.targetName ?? "Remote Linux" : "This machine"}{model.managed === false ? " · Discovered" : ""}</p>
                            {modelAction?.id === model.id && (
                              <span className="model-transition"><i className="model-control-spinner" /> {modelAction.action === "starting" ? "Starting model…" : "Stopping model…"}</span>
                            )}
                          </div>
                        </div>
                      </button>
                      <div className="model-actions">
                        <button
                          className="icon-button"
                          type="button"
                          onClick={() => void toggleRunning(model.id)}
                          disabled={model.managed === false || modelAction !== undefined}
                          title={model.managed === false ? "This model is not present in the active catalog" : model.running ? "Stop model" : "Start model"}
                          aria-label={modelAction?.id === model.id ? modelAction.action === "starting" ? "Starting model" : "Stopping model" : model.running ? "Stop model" : "Start model"}
                        >
                          {modelAction?.id === model.id
                            ? <span className="model-control-spinner" />
                            : <span className={`model-control-icon ${model.running ? "stop" : "start"}`} />}
                        </button>
                        <div className="menu-wrap" data-model-menu>
                          <button
                            className="icon-button"
                            type="button"
                            onClick={(event: MouseEvent<HTMLButtonElement>) => toggleMenu(model.id, event.currentTarget)}
                            aria-haspopup="menu"
                            aria-expanded={menuOpenId === model.id}
                          >
                            ⋯
                          </button>
                          {menuOpenId === model.id && menuPosition && createPortal(
                            <div className="menu-popover" data-model-menu role="menu" style={{ top: `${menuPosition.top}px`, left: `${menuPosition.left}px` }}>
                              <button type="button" role="menuitem" onClick={() => openRename(model)}>Rename</button>
                              <button type="button" role="menuitem" onClick={() => openLogs(model)}>Logs</button>
                              <button type="button" role="menuitem" onClick={() => openPerformance(model)}>Performance</button>
                              <button type="button" role="menuitem" onClick={() => openApi(model)}>API</button>
                              <button type="button" role="menuitem" onClick={() => requestRemoval(model.id)}>Remove</button>
                            </div>,
                            document.body,
                          )}
                        </div>
                      </div>
                    </div>
                  )) : (
                    <div className="empty-state" role="row">
                      <strong>No installed models yet</strong>
                      <p>Add one from the catalog or start Ollama to discover its local model store.</p>
                    </div>
                  )}
                </div>
              </>
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
          Usage statistics: {telemetryEnabled ? "on" : "off"}
        </button>
        {catalog && (
          <div
            className={`catalog-status ${catalog.source}`}
            role="status"
            title={CATALOG_SOURCE_DETAILS[catalog.source]}
          >
            <i aria-hidden="true" />
            <strong>{CATALOG_SOURCE_LABELS[catalog.source]}</strong>
            <span>Catalog {catalog.revision} · {catalog.modelCount} models</span>
          </div>
        )}
      </footer>

      {telemetryDialogOpen && (
        <div
          className="modal-backdrop"
          onClick={() => {
            if (telemetryEnabled !== undefined) setTelemetryDialogOpen(false);
          }}
        >
          <div
            className="modal-card telemetry-dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="telemetry-dialog-title"
            onClick={(event) => event.stopPropagation()}
          >
            <span className="eyebrow">Privacy choice</span>
            <h3 id="telemetry-dialog-title">Help improve LumenSource</h3>
            <p>
              If you choose to share usage statistics, LumenSource sends weekly
              aggregate counts for catalog loads, model installs, starts, and
              built-in chat outcomes, plus coarse operating-system and hardware
              tiers.
            </p>
            <p>
              Prompts, responses, files, paths, hostnames, remote addresses,
              account details, and raw error messages are never collected.
            </p>
            <div className="modal-actions">
              <button
                className="secondary-button"
                type="button"
                disabled={telemetrySaving}
                onClick={() => void saveTelemetryPreference(false)}
              >
                Do not share
              </button>
              <button
                className="primary-button"
                type="button"
                disabled={telemetrySaving}
                onClick={() => void saveTelemetryPreference(true)}
              >
                {telemetrySaving ? "Saving…" : "Share statistics"}
              </button>
            </div>
          </div>
        </div>
      )}

      {remoteTargetDialogOpen && (
        <div className="modal-backdrop" onClick={closeRemoteTargetDialog}>
          <form className="modal-card remote-target-dialog" role="dialog" aria-modal="true" aria-labelledby="remote-target-dialog-title" onClick={(event) => event.stopPropagation()} onSubmit={(event) => {
            event.preventDefault();
            void saveRemoteTarget();
          }}>
            <div className="remote-target-dialog-heading">
              <div>
                <span className="eyebrow">Remote deployment</span>
                <h3 id="remote-target-dialog-title">Add target machine</h3>
              </div>
              <button className="icon-button" type="button" onClick={closeRemoteTargetDialog} disabled={remoteTargetSaveLoading} aria-label="Close target machine dialog">✕</button>
            </div>
            <p>Save the SSH connection settings for this machine. LumenSource will check the connection after you select it in the wizard.</p>
            <div className="wizard-form remote-target-form">
              <label>
                <span>Target name (optional)</span>
                <input value={remoteConfig.name} maxLength={80} onChange={(event) => setRemoteConfig((current) => ({ ...current, name: event.target.value }))} placeholder="GPU workstation" />
                <small>The host or IP address is used when this is blank.</small>
              </label>
              <label>
                <span>Host or IP address</span>
                <input value={remoteConfig.host} maxLength={255} required onChange={(event) => setRemoteConfig((current) => ({ ...current, host: event.target.value }))} placeholder="model-host.example" autoCapitalize="none" spellCheck={false} />
              </label>
              <label>
                <span>SSH username</span>
                <input value={remoteConfig.username} maxLength={64} required onChange={(event) => setRemoteConfig((current) => ({ ...current, username: event.target.value }))} placeholder="user" autoCapitalize="none" spellCheck={false} />
              </label>
              <label>
                <span>SSH port</span>
                <input type="number" min={1} max={65535} required value={remoteConfig.port} onChange={(event) => setRemoteConfig((current) => ({ ...current, port: Number(event.target.value) }))} />
              </label>
              <fieldset className="remote-authentication">
                <legend>Authentication</legend>
                <label className={remoteConfig.authentication === "key" ? "selected" : ""}>
                  <input type="radio" name="new-remote-authentication" value="key" checked={remoteConfig.authentication === "key"} onChange={() => setRemoteConfig((current) => ({ ...current, authentication: "key" }))} />
                  <span><strong>SSH key or agent</strong><small>Recommended for repeat connections and automatic reconnects.</small></span>
                </label>
                <label className={remoteConfig.authentication === "password" ? "selected" : ""}>
                  <input type="radio" name="new-remote-authentication" value="password" checked={remoteConfig.authentication === "password"} onChange={() => setRemoteConfig((current) => ({ ...current, authentication: "password", identityFile: undefined }))} />
                  <span><strong>Password</strong><small>Non-preferred fallback. You will enter it when connecting; it is never saved.</small></span>
                </label>
              </fieldset>
              {remoteConfig.authentication === "key" && (
                <label className="remote-key-field">
                  <span>Identity file (optional)</span>
                  <input value={remoteConfig.identityFile ?? ""} onChange={(event) => setRemoteConfig((current) => ({ ...current, identityFile: event.target.value || undefined }))} placeholder="/home/user/.ssh/id_ed25519" autoCapitalize="none" spellCheck={false} />
                  <small>Leave blank to use your SSH agent and configuration. Encrypted keys must already be unlocked in the agent.</small>
                </label>
              )}
            </div>
            {remoteTargetDialogError && <div className="inline-error" role="alert">{remoteTargetDialogError}</div>}
            <div className="modal-actions">
              <button className="secondary-button" type="button" onClick={closeRemoteTargetDialog} disabled={remoteTargetSaveLoading}>Cancel</button>
              <button className="primary-button" type="submit" disabled={remoteTargetSaveLoading || !remoteConfig.host.trim() || !remoteConfig.username.trim() || !Number.isInteger(remoteConfig.port) || remoteConfig.port < 1 || remoteConfig.port > 65535}>
                {remoteTargetSaveLoading ? "Saving…" : "Save"}
              </button>
            </div>
          </form>
        </div>
      )}

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
        <div className="modal-backdrop" onClick={() => {
          if (!removalBusy) setPendingRemovalId(undefined);
        }}>
          <div className="modal-card" onClick={(event) => event.stopPropagation()}>
            <h3>Remove model?</h3>
            <p>This will stop the model if needed and permanently delete its downloaded files from Ollama. Continue?</p>
            <div className="modal-actions">
              <button className="secondary-button" type="button" disabled={removalBusy} onClick={() => setPendingRemovalId(undefined)}>Cancel</button>
              <button className="primary-button" type="button" disabled={removalBusy} onClick={() => void confirmRemoval()}>{removalBusy ? "Removing…" : "Remove model"}</button>
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
