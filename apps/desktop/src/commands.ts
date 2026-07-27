import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  CatalogSummary,
  ChatEvent,
  ChatMessage,
  EndpointDetails,
  HardwareProfile,
  InstallProgress,
  InstallRequest,
  PerformanceSnapshot,
  PreflightReport,
  Recommendation,
  RemoteConnectionReport,
  RemoteTargetConfig,
  RemoteTargetProfile,
  RunningModelEntry,
  RuntimeStatus,
  UseIntent,
} from "./types";

export const desktopCommands = {
  telemetryPreference: () => invoke<boolean | null>("telemetry_preference"),
  setTelemetryEnabled: (enabled: boolean) =>
    invoke<void>("set_telemetry_enabled", { enabled }),
  detectHardware: (targetId: string) =>
    invoke<HardwareProfile>("detect_hardware", { targetId }),
  loadRemoteTargets: () => invoke<RemoteTargetProfile[]>("load_remote_targets"),
  saveRemoteTarget: (config: RemoteTargetConfig) =>
    invoke<RemoteTargetProfile>("save_remote_target", { config }),
  checkRemoteTarget: (config: RemoteTargetConfig, password?: string) =>
    invoke<RemoteConnectionReport>("check_remote_target", { config, password }),
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
  start: (modelId: string, targetId: string) =>
    invoke<RuntimeStatus>("start_runtime", { modelId, targetId }),
  stop: (modelId: string, targetId: string) => invoke<RuntimeStatus>("stop_runtime", { modelId, targetId }),
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

export function messageFromError(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "Something unexpected happened. Please try again.";
}
