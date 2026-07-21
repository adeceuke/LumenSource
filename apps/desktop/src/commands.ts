import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  CatalogSummary,
  EndpointDetails,
  HardwareProfile,
  InstallProgress,
  InstallRequest,
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
  onInstallProgress: (
    handler: (progress: InstallProgress) => void,
  ): Promise<UnlistenFn> =>
    listen<InstallProgress>("install-progress", ({ payload }) => handler(payload)),
  start: (modelId: string) =>
    invoke<RuntimeStatus>("start_runtime", { modelId }),
  stop: () => invoke<RuntimeStatus>("stop_runtime"),
  status: () => invoke<RuntimeStatus>("runtime_status"),
  endpoint: () => invoke<EndpointDetails>("endpoint_details"),
  loadModels: () => invoke<RunningModelEntry[]>("load_models"),
  saveModels: (models: RunningModelEntry[]) => invoke<void>("save_models", { models }),
};

export function messageFromError(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "Something unexpected happened. Please try again.";
}
