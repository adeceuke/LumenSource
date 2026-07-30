export type WizardStep =
  | "location"
  | "hardware"
  | "intent"
  | "suggestion"
  | "profile"
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

export interface MachineUsageSnapshot {
  targetId: string;
  sampledAtUnixMs: number;
  cpuUtilizationPercent: number;
  usedMemoryBytes: number;
  availableMemoryBytes: number;
  accelerators: Array<{
    name: string;
    backend: string;
    utilizationPercent: number | null;
    usedVramBytes: number | null;
  }>;
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
  labels: string[];
  estimatedLoadedMemoryMinBytes: number;
  estimatedLoadedMemoryMaxBytes: number;
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
  status: "pass" | "warning" | "fail";
  messageKey: string;
  detail?: string;
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
  status: "pass" | "fail";
  messageKey: string;
  detail: string;
  guidance?: string;
}

export interface RemoteConnectionReport {
  targetId: string;
  targetName: string;
  canContinue: boolean;
  checks: RemoteConnectionCheck[];
}

export interface RemoteCredentialStatus {
  passwordRequired: boolean;
  passwordSaved: boolean;
}

export interface InstallRequest {
  modelId: string;
  targetId: string;
  performanceProfile: PerformanceProfile;
  licenseBasis: "catalog" | "separate";
  licenseReference?: string;
  licenseAcknowledged: boolean;
  installRuntime: boolean;
}

export interface StorageReport {
  scannedAt: string;
  totalBytes: number;
  reclaimableBytes: number;
  entries: StorageEntry[];
}

export interface StorageEntry {
  id: string;
  category: string;
  label: string;
  path?: string;
  sizeBytes: number;
  exact: boolean;
  shared: boolean;
  owners: string[];
  cleanupEligible: boolean;
  cleanupEffect: string;
}

export interface CleanupReport {
  removedBytes: number;
  scope: string;
  effect: string;
}

export interface InterruptedInstall {
  modelId: string;
  targetId: string;
  performanceProfile: PerformanceProfile;
  licenseBasis: string;
  licenseReference?: string;
  licenseAcknowledged: boolean;
  installRuntime: boolean;
  startedAt: string;
  recovery: "resume" | "restart";
}

export interface PerformanceProfileReport {
  profile: PerformanceProfile;
  settings: ModelSettings;
  summary: string;
  accelerator: string;
  contextLength?: number;
  concurrentRequests: number;
  minimumMemoryBytes: number;
  availableMemoryBytes: number;
  fitsDetectedMemory: boolean;
  warnings: string[];
  hardwareSummary: string;
}

export interface InstallationValidationCheck {
  id: string;
  status: "pass" | "warning" | "fail";
  detail: string;
}

export interface InstallationValidationReport {
  passed: boolean;
  capability: string;
  runtimeId: string;
  runtimeModelId: string;
  message: string;
  accelerator?: string;
  hardwareSummary?: string;
  effectiveContextLength?: number;
  validatedAt: string;
  running: boolean;
  settings: ModelSettings;
  checks: InstallationValidationCheck[];
}

export interface InstallProgress {
  modelId: string;
  phase: "preparing" | "downloading" | "verifying" | "installing" | "complete" | "cancelled";
  completedBytes: number;
  totalBytes: number;
  currentItem?: number;
  totalItems?: number;
  messageKey: string;
  detail?: string;
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
  runtimeId: "ollama" | "vllm" | "dummy-runtime";
  runtimeModelId?: string;
  runtimeCapabilities: RuntimeCapabilities;
  modelSettings?: ModelSettings;
  installationValidation?: InstallationValidationReport;
  inventoryStatus?: "available" | "missing" | "needsReconnect";
  discovered?: boolean;
  pinned?: boolean;
  lastSeenAt?: string;
  rollback?: ModelRevisionSnapshot;
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

export type RuntimeId = "ollama" | "vllm";
export type PerformanceProfile = "safe" | "balanced" | "fast" | "custom";
export type RuntimeSecretKind = "vllmApiKey" | "huggingFaceToken";

export interface ModelRevisionSnapshot {
  runtimeModelId: string;
  rollbackArtifactId?: string;
  runtimeExecutable?: string;
  digest?: string;
  version: string;
  modelSettings: ModelSettings;
  installationValidation?: InstallationValidationReport;
  savedAt: string;
}

export interface ModelUpdatePlan {
  available: boolean;
  classification: "optional" | "recommended" | "compatibility" | "security";
  kinds: Array<"application" | "runtime" | "containerImage" | "modelWeights" | "tokenizer">;
  currentRevision: string;
  candidateRevision: string;
  downloadBytes: number;
  additionalDiskBytes: number;
  licenseChanged: boolean;
  requiresLicenseAcknowledgement: boolean;
  compatibility: string;
  message: string;
}

export interface ResourceStartPlan {
  canStart: boolean;
  requiredMemoryBytes: number;
  availableMemoryBytes: number;
  requiredVramBytes: number;
  availableVramBytes?: number;
  consumers: ResourceConsumer[];
  stoppableModelIds: string[];
  pinnedModelIds: string[];
  waitingReason?: string;
}

export interface ResourceConsumer {
  modelId: string;
  name: string;
  memoryBytes: number;
  vramBytes: number;
  pinned: boolean;
}

export interface QueuedOperation {
  modelId: string;
  action: string;
  queuedAt: string;
  needsRetry: boolean;
}

export interface ApplicationSettings {
  schemaVersion: number;
  defaultRuntime: RuntimeId;
  defaultTargetId: string;
  defaultUseIntent: UseIntent;
  defaultPerformanceProfile: PerformanceProfile;
  startAfterInstall: boolean;
  autoStartManagedRuntimes: boolean;
  storage: {
    modelDirectory?: string;
    cacheDirectory?: string;
  };
  privacy: {
    telemetryEnabled: boolean;
    lifecycleLogRetention: number;
    confirmModelDeletion: boolean;
  };
  sharing: {
    enabled: boolean;
    allowOtherDevices: boolean;
    port: number;
    exposedModelIds: string[];
  };
  ollama: {
    endpoint: string;
    executablePath?: string;
    contextLength?: number;
    keepAlive: string;
    maxLoadedModels: number;
    parallelRequests: number;
    maxQueuedRequests: number;
    gpuSelection?: string;
    experimentalVulkan: boolean;
    bindAddress: string;
    allowedOrigins: string[];
    debugLogging: boolean;
  };
  vllm: {
    huggingFaceCacheDirectory?: string;
    gpuSelection?: string;
    gpuMemoryUtilization: number;
    maxContextLength?: number;
    maxConcurrentSequences: number;
    prefixCaching: boolean;
    weightDtype: string;
    quantization?: string;
    kvCacheDtype: string;
    cpuOffloadGib: number;
    tensorParallelSize: number;
    pipelineParallelSize: number;
    bindAddress: string;
    managedPortStart: number;
    managedPortEnd: number;
    pinnedRuntimeVersion: string;
  };
}

export interface SharingStatus {
  enabled: boolean;
  running: boolean;
  allowOtherDevices: boolean;
  tokenSaved: boolean;
  address?: string;
  exposedModels: string[];
  transportWarning?: string;
}

export interface RuntimeCapabilities {
  managedModelStorage: boolean;
  multipleModels: boolean;
  chat: boolean;
  embeddings: boolean;
  pooling: boolean;
  modelStartStop: boolean;
  globalConfiguration: boolean;
  perModelConfiguration: boolean;
  artifactAcquisition: boolean;
  remoteConnection: boolean;
  lifecycle?: "managed" | "external" | "simulated";
}

export interface ModelSettings {
  performanceProfile?: PerformanceProfile;
  runtimeManagementMode?: "managed" | "external";
  inferenceTask?: "chat" | "embeddings";
  endpoint?: string;
  verifyTls: boolean;
  connectionTimeoutSeconds: number;
  requestTimeoutSeconds: number;
  systemPrompt?: string;
  temperature?: number;
  maxOutputTokens?: number;
  topP?: number;
  topK?: number;
  minP?: number;
  repetitionPenalty?: number;
  seed?: number;
  stopSequences: string[];
  structuredOutput?: boolean;
  reasoningLevel?: "low" | "medium" | "high";
  contextLength?: number;
  keepAlive?: string;
  loadOnStartup?: boolean;
  preferredAccelerator?: string;
  ollamaDerivedModelName?: string;
  ollamaPersistentParameters: boolean;
  vllmModelRevision?: string;
  vllmTokenizerRevision?: string;
  vllmServedModelName?: string;
  vllmTask?: string;
  vllmRunner?: string;
  vllmWeightDtype?: string;
  vllmQuantization?: string;
  vllmGpuMemoryUtilization?: number;
  vllmMaxConcurrentSequences?: number;
  vllmPrefixCaching?: boolean;
  vllmKvCacheDtype?: string;
  vllmCpuOffloadGib?: number;
  vllmTensorParallelSize?: number;
  vllmPipelineParallelSize?: number;
  managedContainerEngine?: string;
  managedContainerName?: string;
  managedPort?: number;
}

export interface ModelSettingsSaveReport {
  model: RunningModelEntry;
  restartRequired: boolean;
  restarted: boolean;
  message: string;
}

export interface ManagedVllmSupport {
  supported: boolean;
  platform: string;
  containerEngine?: string;
  nvidiaAvailable: boolean;
  message: string;
}

export interface RuntimeMigrationOption {
  runtimeId: RuntimeId;
  variantId?: string;
  available: boolean;
  reason: string;
  requiresHuggingFaceToken: boolean;
  tokenSaved: boolean;
}

export interface RuntimeMigrationReport {
  replacement: RunningModelEntry;
  sourceEntryId: string;
  sourceCanBeRemoved: boolean;
  message: string;
}

export interface RuntimeDiagnostics {
  runtimeId: string;
  version: string;
  health: string;
  lifecycle: string;
  endpoint?: string;
  effectiveContextLength?: number;
  effectiveKeepAlive?: string;
  managedContainerEngine?: string;
  managedContainerName?: string;
  managedPort?: number;
  recentLogs: string[];
}

export interface ExternalVllmConfig {
  endpoint: string;
  servedModel: string;
  inferenceTask: "chat" | "embeddings";
  verifyTls: boolean;
  connectionTimeoutSeconds: number;
  requestTimeoutSeconds: number;
}

export interface VllmConnectionReport {
  healthy: boolean;
  authenticated: boolean;
  endpoint: string;
  models: string[];
  message: string;
}

export interface SettingsValidationError {
  field: string;
  message: string;
}

export interface SettingsSaveReport {
  settings: ApplicationSettings;
  runtimeRestartRequired: boolean;
}

export interface OllamaConnectionReport {
  healthy: boolean;
  endpoint: string;
  version?: string;
  message: string;
}
