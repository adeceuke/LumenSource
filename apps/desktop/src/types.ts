export type WizardStep =
  | "location"
  | "hardware"
  | "intent"
  | "suggestion"
  | "license"
  | "download"
  | "ready";

export interface HardwareProfile {
  cpu: string;
  cpuCores: number;
  cpuFrequencyMhz?: number;
  memoryBytes: number;
  memoryKind?: string;
  memorySpeedMts?: number;
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

export type UseIntent = "chat" | "code" | "creative" | "research";

export interface Recommendation {
  modelId: string;
  runtimeId: string;
  name: string;
  provider: string;
  description: string;
  version: string;
  sizeBytes: number;
  contextWindow: number;
  runtimeDigest?: string;
  fit: "ideal" | "good" | "limited" | "incompatible";
  reasons: string[];
  recommended: boolean;
  compatible: boolean;
  license: ModelLicense;
}

export interface ModelLicense {
  profileId?: string;
  name: string;
  url?: string;
  classification: string;
  commercialUse: "permitted" | "permitted-with-conditions" | "not-permitted" | "unknown";
  redistribution: "permitted" | "permitted-with-conditions" | "not-permitted" | "unknown";
  derivatives: "permitted" | "permitted-with-conditions" | "not-permitted" | "unknown";
  requiresUserAcceptance: boolean;
  attribution: "required" | "not-required" | "unknown";
  licenseText: string;
  notice: string;
  uiNotice: "informational" | "acknowledgement-required";
  summary: string;
  obligations: string[];
  restrictions: string[];
  geographicRestrictions: string[];
  usagePolicyUrl?: string;
  reviewedAt?: string;
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

export interface RemoteTargetConfig {
  name: string;
  host: string;
  port: number;
  username: string;
  authentication: "key" | "password";
  identityFile?: string;
}

export interface RemoteTargetProfile {
  targetId: string;
  targetName: string;
  config: RemoteTargetConfig;
}

export interface RemoteConnectionCheck {
  id: string;
  label: string;
  status: "pass" | "fail";
  detail: string;
  guidance?: string;
}

export interface RemoteConnectionReport {
  targetId: string;
  targetName: string;
  canContinue: boolean;
  checks: RemoteConnectionCheck[];
}

export interface InstallRequest {
  modelId: string;
  targetId: string;
  licenseBasis: "catalog" | "separate";
  licenseReference?: string;
  licenseAcknowledged: boolean;
}

export interface InstallProgress {
  modelId: string;
  phase: "preparing" | "downloading" | "verifying" | "installing" | "complete" | "cancelled";
  completedBytes: number;
  totalBytes: number;
  currentItem?: number;
  totalItems?: number;
  message: string;
}

export interface RuntimeStatus {
  state: "stopped" | "starting" | "running" | "stopping" | "error";
  modelId?: string;
  message?: string;
}

export interface PerformanceSnapshot {
  modelId: string;
  state: "running" | "stopped" | "unavailable";
  sampledAtUnixMs: number;
  allocatedMemoryBytes: number;
  allocatedVramBytes: number;
  allocatedSystemMemoryBytes: number;
  contextLength?: number;
}

export interface EndpointDetails {
  baseUrl: string;
  chatCompletionsUrl: string;
  completionsUrl: string;
  embeddingsUrl: string;
  model: string;
  apiKeyRequired: boolean;
  apiAvailable: boolean;
  chatAvailable: boolean;
  embeddingsAvailable: boolean;
}

export interface ChatMessage {
  role: "user" | "assistant";
  content: string;
}

export type ChatEvent =
  | { event: "delta"; content: string }
  | { event: "done" };

export interface RunningModelEntry {
  id: string;
  name: string;
  modelId: string;
  modelName: string;
  runtimeModelId?: string;
  version: string;
  location: "local" | "remote";
  targetId: string;
  targetName?: string;
  running: boolean;
  managed?: boolean;
  digest?: string;
  sizeBytes?: number;
  licenseBasis?: "catalog" | "separate";
  licenseReference?: string;
  licenseAcknowledgedAt?: string;
  licenseProfileId?: string;
  licenseName?: string;
  licenseUrl?: string;
  licenseReviewedAt?: string;
  licenseCatalogVersion?: string;
  logs: string[];
}
