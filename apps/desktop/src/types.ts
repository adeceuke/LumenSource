export type WizardStep =
  | "location"
  | "hardware"
  | "intent"
  | "suggestion"
  | "requirements"
  | "download";

export interface HardwareProfile {
  cpu: string;
  cpuCores: number;
  memoryBytes: number;
  gpu?: {
    name: string;
    memoryBytes?: number;
    backend: string;
  };
  platform: string;
  storage: {
    mountPoint: string;
    totalBytes: number;
    availableBytes: number;
  };
}

export interface CatalogModelSummary {
  id: string;
  displayName: string;
  version: string;
  description: string;
}

export interface CatalogSummary {
  revision: string;
  updatedAt: string;
  modelCount: number;
  source: "cache" | "network" | "bundled";
  models: CatalogModelSummary[];
}

export type UseIntent = "chat" | "code" | "creative";

export interface Recommendation {
  modelId: string;
  name: string;
  description: string;
  sizeBytes: number;
  contextWindow: number;
  fit: "ideal" | "good" | "limited";
  reasons: string[];
}

export interface PreflightCheck {
  id: string;
  label: string;
  status: "pass" | "warning" | "fail";
  detail: string;
}

export interface PreflightReport {
  canInstall: boolean;
  requiredBytes: number;
  availableBytes: number;
  checks: PreflightCheck[];
}

export interface InstallRequest {
  modelId: string;
}

export interface InstallProgress {
  modelId: string;
  phase: "preparing" | "downloading" | "verifying" | "installing" | "complete";
  completedBytes: number;
  totalBytes: number;
  message: string;
}

export interface RuntimeStatus {
  state: "stopped" | "starting" | "running" | "stopping" | "error";
  modelId?: string;
  message?: string;
}

export interface EndpointDetails {
  baseUrl: string;
  chatCompletionsUrl: string;
  model: string;
  apiKeyRequired: boolean;
}

export interface RunningModelEntry {
  id: string;
  name: string;
  modelId: string;
  modelName: string;
  version: string;
  location: "local" | "remote";
  running: boolean;
  logs: string[];
}
