import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useState, type Dispatch, type SetStateAction } from "react";
import type { ModelSetupWizardProps } from "../components/ModelSetupWizard";
import {
  desktopCommands,
  isInstallCancellationMessage,
  messageFromError,
} from "../commands";
import { browserMessages } from "../i18n";
import { emptyRemoteTarget, lifecycleLog } from "../modelUi";
import type {
  CatalogSummary,
  EndpointDetails,
  HardwareProfile,
  InstallProgress,
  PreflightReport,
  Recommendation,
  RemoteConnectionReport,
  RemoteTargetConfig,
  RemoteTargetProfile,
  RunningModelEntry,
  UseIntent,
  WizardStep,
} from "../types";

const text = browserMessages();

interface UseModelWizardOptions {
  catalog?: CatalogSummary;
  setModels: Dispatch<SetStateAction<RunningModelEntry[]>>;
  setError: Dispatch<SetStateAction<string | undefined>>;
  onOpen: () => void;
  copiedField?: string;
  copyText: (value: string, key: string) => void | Promise<void>;
}

interface RemoteTargetDialogState {
  open: boolean;
  config: RemoteTargetConfig;
  saving: boolean;
  error?: string;
  setConfig: Dispatch<SetStateAction<RemoteTargetConfig>>;
  close: () => void;
  save: () => Promise<void>;
}

interface ModelWizardState {
  open: boolean;
  busy: boolean;
  openWizard: () => void;
  closeConfirmationOpen: boolean;
  dismissCloseConfirmation: () => void;
  confirmClose: () => void;
  remoteTargetDialog: RemoteTargetDialogState;
  props: ModelSetupWizardProps;
}

function localizedError(error: unknown): string {
  return messageFromError(error, text.errors.unexpected);
}

export function useModelWizard({
  catalog,
  setModels,
  setError,
  onOpen,
  copiedField,
  copyText,
}: UseModelWizardOptions): ModelWizardState {
  const [open, setOpen] = useState(false);
  const [wizardStep, setWizardStep] = useState<WizardStep>("location");
  const [wizardLocation, setWizardLocation] = useState<"local" | "remote">("local");
  const [remoteTargets, setRemoteTargets] = useState<RemoteTargetProfile[]>([]);
  const [selectedRemoteTargetId, setSelectedRemoteTargetId] = useState("");
  const [remoteTargetDialogOpen, setRemoteTargetDialogOpen] = useState(false);
  const [remoteTargetSaveLoading, setRemoteTargetSaveLoading] = useState(false);
  const [remoteTargetDialogError, setRemoteTargetDialogError] = useState<string>();
  const [remoteConfig, setRemoteConfig] = useState<RemoteTargetConfig>(emptyRemoteTarget);
  const [remotePassword, setRemotePassword] = useState("");
  const [rememberRemotePassword, setRememberRemotePassword] = useState(true);
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
  const [modelPickerPosition, setModelPickerPosition] = useState<{
    top: number;
    left: number;
    width: number;
  }>();
  const [preflight, setPreflight] = useState<PreflightReport>();
  const [preflightLoading, setPreflightLoading] = useState(false);
  const [licenseBasis, setLicenseBasis] = useState<"catalog" | "separate">("catalog");
  const [licenseAcknowledged, setLicenseAcknowledged] = useState(false);
  const [separateLicenseReference, setSeparateLicenseReference] = useState("");
  const [installProgress, setInstallProgress] = useState<InstallProgress>();
  const [endpoint, setEndpoint] = useState<EndpointDetails>();
  const [busy, setBusy] = useState(false);
  const [confirmClose, setConfirmClose] = useState(false);
  const [hardwareInfo, setHardwareInfo] = useState<HardwareProfile>();
  const [hardwareLoading, setHardwareLoading] = useState(false);
  const [hardwareError, setHardwareError] = useState<string>();

  const selectedRemoteTarget = remoteTargets.find(
    (target) => target.targetId === selectedRemoteTargetId,
  );
  const wizardTargetId = wizardLocation === "remote" ? remoteReport?.targetId : "local";
  const selectedRecommendation = recommendations.find(
    (recommendation) => recommendation.modelId === selectedModelId,
  );
  const recommendedModel = recommendations.find((recommendation) => recommendation.recommended);
  const selectedIsDummy = selectedRecommendation?.runtimeId === "dummy-runtime";
  const licenseReviewComplete = Boolean(selectedRecommendation) && (licenseBasis === "catalog"
    ? !selectedRecommendation?.license.requiresUserAcceptance || licenseAcknowledged
    : licenseAcknowledged && separateLicenseReference.trim().length > 0);

  useEffect(() => {
    void desktopCommands.loadRemoteTargets().then(setRemoteTargets).catch((loadError: unknown) => {
      setError(localizedError(loadError));
    });
  }, [setError]);

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
    if (
      wizardStep !== "hardware"
      || !wizardTargetId
      || hardwareInfo
      || hardwareLoading
      || hardwareError
    ) {
      return;
    }
    setHardwareLoading(true);
    setHardwareError(undefined);
    void desktopCommands.detectHardware(wizardTargetId)
      .then((info) => {
        setHardwareInfo(info);
        setHardwareLoading(false);
      })
      .catch((detectionError: unknown) => {
        setHardwareError(localizedError(detectionError));
        setHardwareLoading(false);
      });
  }, [wizardStep, wizardTargetId, hardwareInfo, hardwareLoading, hardwareError]);

  useEffect(() => {
    if (wizardStep !== "suggestion" || !selectedModelId || !wizardTargetId) return;
    let disposed = false;
    setPreflight(undefined);
    setPreflightLoading(true);
    setError(undefined);
    void desktopCommands.preflight(selectedModelId, wizardTargetId)
      .then((report) => {
        if (!disposed) setPreflight(report);
      })
      .catch((preflightError: unknown) => {
        if (!disposed) setError(localizedError(preflightError));
      })
      .finally(() => {
        if (!disposed) setPreflightLoading(false);
      });
    return () => {
      disposed = true;
    };
  }, [wizardStep, selectedModelId, wizardTargetId, setError]);

  useEffect(() => {
    setLicenseBasis("catalog");
    setLicenseAcknowledged(false);
    setSeparateLicenseReference("");
  }, [selectedModelId]);

  const reset = () => {
    setWizardStep("location");
    setWizardLocation("local");
    setSelectedRemoteTargetId("");
    setRemoteReport(undefined);
    setRemoteCheckLoading(false);
    setRemotePassword("");
    setRememberRemotePassword(true);
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
    setConfirmClose(false);
    setHardwareInfo(undefined);
    setHardwareLoading(false);
    setHardwareError(undefined);
  };

  const openWizard = () => {
    onOpen();
    reset();
    void desktopCommands.loadRemoteTargets().then(setRemoteTargets).catch((loadError: unknown) => {
      setError(localizedError(loadError));
    });
    setOpen(true);
  };

  const confirmWizardClose = () => {
    setOpen(false);
    reset();
  };

  const requestInstallCancellation = async () => {
    if (!downloadBusy || !installCancellable || cancelBusy) return;
    setCancelBusy(true);
    setInstallProgress((current) => current && ({
      ...current,
      messageKey: "cancelling",
    }));
    try {
      const accepted = await desktopCommands.cancelInstall();
      if (!accepted) {
        setCancelBusy(false);
        setInstallCancellable(false);
        setError(text.errors.cancellationTooLate);
      }
    } catch (cancelError) {
      setCancelBusy(false);
      setError(localizedError(cancelError));
    }
  };

  const cancelWizard = () => {
    if (downloadBusy) {
      void requestInstallCancellation();
    } else if (wizardStep === "location" || wizardStep === "ready") {
      confirmWizardClose();
    } else {
      setConfirmClose(true);
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
      setRemoteTargets((current) => [...current.filter(
        (target) => target.targetId !== saved.targetId,
      ), saved].sort((left, right) => left.targetName.localeCompare(right.targetName)));
      setSelectedRemoteTargetId(saved.targetId);
      setRemoteReport(undefined);
      setRemotePassword("");
      setRemoteTargetDialogOpen(false);
      setRemoteConfig(emptyRemoteTarget());
    } catch (saveError) {
      setRemoteTargetDialogError(localizedError(saveError));
    } finally {
      setRemoteTargetSaveLoading(false);
    }
  };

  const checkRemoteTarget = async () => {
    if (!selectedRemoteTarget) return;
    const connectionPassword = remotePassword;
    setRemoteCheckLoading(true);
    setRemoteReport(undefined);
    setError(undefined);
    try {
      const report = await desktopCommands.checkRemoteTarget(
        selectedRemoteTarget.config,
        selectedRemoteTarget.config.authentication === "password" && connectionPassword
          ? connectionPassword
          : undefined,
      );
      if (
        report.canContinue
        && selectedRemoteTarget.config.authentication === "password"
        && connectionPassword
        && rememberRemotePassword
      ) {
        await desktopCommands.saveRemotePassword(report.targetId, connectionPassword);
      }
      setRemoteReport(report);
    } catch (connectionError) {
      setError(localizedError(connectionError));
    } finally {
      setRemotePassword("");
      setRemoteCheckLoading(false);
    }
  };

  const openPublisherUrl = async (url: string) => {
    try {
      const parsed = new URL(url);
      if (parsed.protocol !== "https:") {
        throw new Error(text.errors.insecurePublisherUrl);
      }
      await openUrl(url);
    } catch (linkError) {
      setError(text.errors.publisherUrl(localizedError(linkError)));
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
        const available = await desktopCommands.recommendations(
          useCase,
          wizardTargetId ?? "local",
        );
        const result = wizardLocation === "remote"
          ? available.filter((recommendation) => recommendation.runtimeId === "ollama")
          : available;
        if (!result.length) {
          throw new Error(text.errors.noSupportedModel);
        }
        setRecommendations(result);
        setSelectedModelId(
          result.find((recommendation) => recommendation.recommended)?.modelId
            ?? result[0].modelId,
        );
        setPreflight(undefined);
        setWizardStep("suggestion");
      } catch (recommendationError) {
        setError(localizedError(recommendationError));
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

  const addInstalledModel = (
    selected: Recommendation,
    runtimeModelId: string,
    running: boolean,
  ) => {
    setModels((current) => {
      const targetId = wizardTargetId ?? "local";
      const sharedRuntimeAlreadyRunning = current.some((model) => (
        model.targetId === targetId
        && model.runtimeModelId === runtimeModelId
        && model.running
      ));
      const effectiveRunning = running || sharedRuntimeAlreadyRunning;
      const message = running
        ? text.lifecycle.installedAndStarted
        : sharedRuntimeAlreadyRunning
          ? text.lifecycle.installedSharedRuntime
          : text.lifecycle.installedNotStarted;
      const entry: RunningModelEntry = {
        id: `${selected.modelId}-${crypto.randomUUID()}`,
        name: selected.name,
        modelId: selected.modelId,
        modelName: selected.name,
        runtimeModelId,
        version: selected.version,
        location: wizardLocation,
        targetId,
        targetName: wizardLocation === "remote" ? remoteReport?.targetName : undefined,
        running: effectiveRunning,
        managed: true,
        digest: selected.runtimeDigest,
        licenseBasis,
        licenseReference: licenseBasis === "separate"
          ? separateLicenseReference.trim()
          : undefined,
        licenseAcknowledgedAt: licenseAcknowledged ? new Date().toISOString() : undefined,
        licenseProfileId: selected.license.profileId,
        licenseName: selected.license.name,
        licenseUrl: selected.license.url,
        licenseReviewedAt: selected.license.reviewedAt,
        licenseCatalogVersion: catalog?.revision,
        logs: [lifecycleLog(message)],
      };
      return [entry, ...current.map((model) => (
        model.targetId === targetId && model.runtimeModelId === runtimeModelId
          ? {
              ...model,
              running: effectiveRunning,
              logs: !model.running && effectiveRunning
                ? [...model.logs, lifecycleLog(text.lifecycle.sharedRuntimeStarted)]
                : model.logs,
            }
          : model
      ))];
    });
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
      messageKey: "preparing",
    });

    let unlisten: (() => void) | undefined;
    try {
      unlisten = await desktopCommands.onInstallProgress(setInstallProgress);
      await desktopCommands.install({
        modelId: selectedRecommendation.modelId,
        targetId: wizardTargetId ?? "local",
        licenseBasis,
        licenseReference: licenseBasis === "separate"
          ? separateLicenseReference.trim()
          : undefined,
        licenseAcknowledged,
      });
      setInstallCancellable(false);
      if (startAfterInstall) {
        setInstallProgress((current) => current && ({
          ...current,
          phase: "installing",
          messageKey: "starting",
        }));
        await desktopCommands.start(
          selectedRecommendation.modelId,
          wizardTargetId ?? "local",
        );
      }
      const details = await desktopCommands.endpoint(wizardTargetId ?? "local");
      setEndpoint(details);
      addInstalledModel(selectedRecommendation, details.model, startAfterInstall);
      setWizardStep("ready");
    } catch (installError) {
      const message = localizedError(installError);
      if (isInstallCancellationMessage(message)) {
        setInstallProgress((current) => ({
          modelId: current?.modelId ?? selectedRecommendation.modelId,
          phase: "cancelled",
          completedBytes: 0,
          totalBytes: current?.totalBytes ?? selectedRecommendation.sizeBytes,
          messageKey: "cancelled",
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

  return {
    open,
    busy,
    openWizard,
    closeConfirmationOpen: confirmClose,
    dismissCloseConfirmation: () => setConfirmClose(false),
    confirmClose: confirmWizardClose,
    remoteTargetDialog: {
      open: remoteTargetDialogOpen,
      config: remoteConfig,
      saving: remoteTargetSaveLoading,
      error: remoteTargetDialogError,
      setConfig: setRemoteConfig,
      close: closeRemoteTargetDialog,
      save: saveRemoteTarget,
    },
    props: {
      wizardStep,
      wizardLocation,
      remoteTargets,
      selectedRemoteTargetId,
      selectedRemoteTarget,
      remotePassword,
      rememberRemotePassword,
      remoteReport,
      remoteCheckLoading,
      hardwareInfo,
      hardwareLoading,
      hardwareError,
      useCase,
      busy,
      recommendations,
      recommendedModel,
      selectedModelId,
      selectedRecommendation,
      modelPickerOpen,
      modelPickerPosition,
      preflight,
      preflightLoading,
      licenseBasis,
      licenseAcknowledged,
      separateLicenseReference,
      licenseReviewComplete,
      downloadBusy,
      installCancellable,
      cancelBusy,
      startAfterInstall,
      installProgress,
      selectedIsDummy,
      endpoint,
      copiedField,
      setWizardLocation,
      setRemoteReport,
      setHardwareInfo,
      setHardwareError,
      setSelectedRemoteTargetId,
      setRemotePassword,
      setRememberRemotePassword,
      setUseCase,
      setSelectedModelId,
      setModelPickerOpen,
      setModelPickerPosition,
      setLicenseBasis,
      setLicenseAcknowledged,
      setSeparateLicenseReference,
      setStartAfterInstall,
      cancelWizard,
      openRemoteTargetDialog,
      checkRemoteTarget,
      goToNextStep,
      goToPreviousStep,
      openPublisherUrl,
      requestInstallCancellation,
      startInstall,
      confirmWizardClose,
      copyText,
    },
  };
}
