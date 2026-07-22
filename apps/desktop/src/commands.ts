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
  RunningModelEntry,
  RuntimeStatus,
  UseIntent,
} from "./types";

export const desktopCommands = {
  detectHardware: () => invoke<HardwareProfile>("detect_hardware"),
  loadCatalog: () => invoke<CatalogSummary>("load_catalog"),
  refreshCatalog: () => invoke<CatalogSummary>("refresh_catalog"),
  recommendations: (intent: UseIntent) =>
    invoke<Recommendation[]>("get_recommendations", { intent }),
  preflight: (modelId: string) =>
    invoke<PreflightReport>("run_preflight", { modelId }),
  install: (request: InstallRequest) =>
    invoke<void>("install_model", { request }),
  cancelInstall: () => invoke<boolean>("cancel_install"),
  onInstallProgress: (
    handler: (progress: InstallProgress) => void,
  ): Promise<UnlistenFn> =>
    listen<InstallProgress>("install-progress", ({ payload }) => handler(payload)),
  start: (modelId: string) =>
    invoke<RuntimeStatus>("start_runtime", { modelId }),
  stop: (modelId: string) => invoke<RuntimeStatus>("stop_runtime", { modelId }),
  status: () => invoke<RuntimeStatus>("runtime_status"),
  performance: (modelId: string, runtimeModelId: string) =>
    invoke<PerformanceSnapshot>("model_performance", { modelId, runtimeModelId }),
  endpoint: () => invoke<EndpointDetails>("endpoint_details"),
  modelEndpoint: (modelId: string, runtimeModelId: string) =>
    invoke<EndpointDetails>("model_endpoint_details", { modelId, runtimeModelId }),
  chat: (
    modelId: string,
    runtimeModelId: string,
    messages: ChatMessage[],
    handler: (event: ChatEvent) => void,
  ) => {
    const onEvent = new Channel<ChatEvent>();
    onEvent.onmessage = handler;
    return invoke<void>("chat_with_model", { modelId, runtimeModelId, messages, onEvent });
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
