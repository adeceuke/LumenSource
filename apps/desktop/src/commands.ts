import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ApplicationSettings,
  CatalogSummary,
  ChatEvent,
  ChatMessage,
  EndpointDetails,
  HardwareProfile,
  InstallProgress,
  InstallRequest,
  MachineUsageSnapshot,
  OllamaConnectionReport,
  PerformanceSnapshot,
  PreflightReport,
  Recommendation,
  RemoteConnectionReport,
  RemoteCredentialStatus,
  RemoteTargetConfig,
  RemoteTargetProfile,
  RunningModelEntry,
  RuntimeSecretKind,
  RuntimeStatus,
  SettingsSaveReport,
  SettingsValidationError,
  UseIntent,
} from "./types";

export const desktopCommands = {
  telemetryPreference: () => invoke<boolean | null>("telemetry_preference"),
  setTelemetryEnabled: (enabled: boolean) =>
    invoke<void>("set_telemetry_enabled", { enabled }),
  loadSettings: () => invoke<ApplicationSettings>("load_settings"),
  validateSettings: (settings: ApplicationSettings) =>
    invoke<SettingsValidationError[]>("validate_settings", { settings }),
  saveSettings: (settings: ApplicationSettings, confirmNetworkExposure: boolean) =>
    invoke<SettingsSaveReport>("save_settings", { settings, confirmNetworkExposure }),
  resetSettings: () => invoke<SettingsSaveReport>("reset_settings"),
  testOllamaConnection: (settings: ApplicationSettings) =>
    invoke<OllamaConnectionReport>("test_ollama_connection", { settings }),
  restartManagedOllama: () =>
    invoke<OllamaConnectionReport>("restart_managed_ollama"),
  runtimeSecretStatus: (kind: RuntimeSecretKind) =>
    invoke<boolean>("runtime_secret_status", { kind }),
  saveRuntimeSecret: (kind: RuntimeSecretKind, secret: string) =>
    invoke<void>("save_runtime_secret", { kind, secret }),
  deleteRuntimeSecret: (kind: RuntimeSecretKind) =>
    invoke<void>("delete_runtime_secret", { kind }),
  detectHardware: (targetId: string, password?: string) =>
    invoke<HardwareProfile>("detect_hardware", { targetId, password }),
  machineUsage: (targetId: string, password?: string) =>
    invoke<MachineUsageSnapshot>("machine_usage", { targetId, password }),
  loadRemoteTargets: () => invoke<RemoteTargetProfile[]>("load_remote_targets"),
  saveRemoteTarget: (config: RemoteTargetConfig) =>
    invoke<RemoteTargetProfile>("save_remote_target", { config }),
  checkRemoteTarget: (config: RemoteTargetConfig, password?: string) =>
    invoke<RemoteConnectionReport>("check_remote_target", { config, password }),
  remoteCredentialStatus: (targetId: string) =>
    invoke<RemoteCredentialStatus>("remote_credential_status", { targetId }),
  saveRemotePassword: (targetId: string, password: string) =>
    invoke<void>("save_remote_password", { targetId, password }),
  deleteRemotePassword: (targetId: string) =>
    invoke<void>("delete_remote_password", { targetId }),
  loadCatalog: () => invoke<CatalogSummary>("load_catalog"),
  refreshCatalog: () => invoke<CatalogSummary>("refresh_catalog"),
  recommendations: (intent: UseIntent, targetId: string) =>
    invoke<Recommendation[]>("get_recommendations", { intent, targetId }),
  preflight: (modelId: string, targetId: string) =>
    invoke<PreflightReport>("run_preflight", { modelId, targetId }),
  install: (request: InstallRequest) =>
    invoke<void>("install_model", { request }),
  cancelInstall: () => invoke<boolean>("cancel_install"),
  onInstallProgress: (
    handler: (progress: InstallProgress) => void,
  ): Promise<UnlistenFn> =>
    listen<InstallProgress>("install-progress", ({ payload }) => handler(payload)),
  start: (modelId: string, targetId: string, password?: string) =>
    invoke<RuntimeStatus>("start_runtime", { modelId, targetId, password }),
  stop: (modelId: string, targetId: string, password?: string) =>
    invoke<RuntimeStatus>("stop_runtime", { modelId, targetId, password }),
  status: () => invoke<RuntimeStatus>("runtime_status"),
  performance: (modelId: string, runtimeModelId: string, targetId: string) =>
    invoke<PerformanceSnapshot>("model_performance", { modelId, runtimeModelId, targetId }),
  endpoint: (targetId: string) => invoke<EndpointDetails>("endpoint_details", { targetId }),
  modelEndpoint: (modelId: string, runtimeModelId: string, targetId: string) =>
    invoke<EndpointDetails>("model_endpoint_details", { modelId, runtimeModelId, targetId }),
  chat: (
    modelId: string,
    runtimeModelId: string,
    targetId: string,
    messages: ChatMessage[],
    handler: (event: ChatEvent) => void,
  ) => {
    const onEvent = new Channel<ChatEvent>();
    onEvent.onmessage = handler;
    return invoke<void>("chat_with_model", { modelId, runtimeModelId, targetId, messages, onEvent });
  },
  cancelChat: () => invoke<boolean>("cancel_chat"),
  loadModels: () => invoke<RunningModelEntry[]>("load_models"),
  saveModels: (models: RunningModelEntry[]) => invoke<void>("save_models", { models }),
  removeModel: (modelId: string) => invoke<RunningModelEntry[]>("remove_model", { modelId }),
};

export function messageFromError(error: unknown, fallback: string): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return fallback;
}

export function isChatCancellationMessage(message: string): boolean {
  return message.toLowerCase().includes("chat cancelled");
}

export function isInstallCancellationMessage(message: string): boolean {
  return message.toLowerCase() === "installation cancelled";
}

export function isRemotePasswordRequiredMessage(message: string): boolean {
  const normalized = message.toLowerCase();
  return normalized.includes("ssh_password_required")
    || normalized.includes("password authentication was rejected")
    || normalized.includes("enter the ssh password");
}
