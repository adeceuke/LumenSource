import { browserMessages } from "./i18n";
import type {
  EndpointDetails,
  InstallProgress,
  PreflightCheck,
  RemoteConnectionCheck,
  RemoteTargetConfig,
  RuntimeCapabilities,
} from "./types";

const text = browserMessages();

export function emptyRemoteTarget(): RemoteTargetConfig {
  return {
    name: "",
    host: "",
    port: 22,
    username: "",
    authentication: "key",
  };
}

export function formatBytes(bytes: number): string {
  const gibibytes = bytes / (1024 ** 3);
  return text.format.gibibytes(gibibytes.toFixed(gibibytes >= 10 ? 0 : 1));
}

export function formatLegalValue(value: string): string {
  return text.format.legalValue(value);
}

export function progressPercent(progress?: InstallProgress): number {
  if (!progress) return 0;
  if (progress.phase === "cancelled") return 0;
  if (progress.phase === "failed") return 98;
  if (progress.phase === "complete") return 100;
  if (progress.phase === "preparing") return 5;
  if (progress.phase === "verifying") return 88;
  if (progress.phase === "installing") return 95;
  if (progress.phase === "validating") return 98;
  if (progress.totalBytes <= 0) return 12;
  return Math.max(8, Math.min(85, (progress.completedBytes / progress.totalBytes) * 85));
}

export function installProgressMessage(progress?: InstallProgress): string {
  if (!progress) return text.wizard.install.ready;
  const messages: Record<string, string> = text.wizard.install.progressMessages;
  return messages[progress.messageKey] ?? progress.detail ?? text.wizard.install.ready;
}

export function preflightCheckCopy(check: PreflightCheck): { label: string; detail: string } {
  const labels: Record<string, string> = text.wizard.suggestion.checkLabels;
  const messages: Record<string, string> = text.wizard.suggestion.checkMessages;
  const remote = check.messageKey.startsWith("remote.");
  const labelKey = check.id === "hardware" && remote
    ? "remoteHardware"
    : check.id === "storage" && remote
      ? "remoteStorage"
      : check.id === "runtime" && check.messageKey === "local.runtimeDummy"
        ? "dummyRuntime"
        : check.id === "runtime" && remote
          ? "remoteRuntime"
          : check.id;
  let detail = check.detail ?? messages[check.messageKey.replace(/^(local|remote)\./, "")] ?? check.messageKey;
  if (check.messageKey === "local.runtimeInstallable" && check.detail) {
    const [url, sha256] = check.detail.split("\t", 2);
    detail = text.wizard.suggestion.runtimeInstallable(url, sha256);
  } else if (check.messageKey === "remote.source") {
    detail = text.wizard.suggestion.remoteSource(check.detail ?? "");
  } else if (check.messageKey === "local.sourceDummy") {
    detail = text.wizard.suggestion.dummySource(check.detail ?? "");
  } else if (check.messageKey === "local.sourceOllama") {
    detail = text.wizard.suggestion.ollamaSource(check.detail ?? "");
  }
  return {
    label: labels[labelKey] ?? check.id,
    detail,
  };
}

export function remoteCheckCopy(check: RemoteConnectionCheck): { label: string; detail: string } {
  const labels: Record<string, string> = text.wizard.location.checkLabels;
  const messages: Record<string, string> = text.wizard.location.checkMessages;
  return {
    label: labels[check.id] ?? check.id,
    detail: messages[check.messageKey] ?? check.detail,
  };
}

export function lifecycleLog(message: string): string {
  return `[${new Date().toISOString()}] ${message}`;
}

export function runtimeCapabilities(runtimeId: string): RuntimeCapabilities {
  if (runtimeId === "vllm") {
    return {
      managedModelStorage: false,
      multipleModels: false,
      chat: true,
      embeddings: true,
      pooling: true,
      modelStartStop: false,
      globalConfiguration: false,
      perModelConfiguration: true,
      artifactAcquisition: false,
      remoteConnection: false,
      lifecycle: "external",
    };
  }
  const dummy = runtimeId === "dummy-runtime";
  return {
    managedModelStorage: true,
    multipleModels: true,
    chat: !dummy,
    embeddings: !dummy,
    pooling: false,
    modelStartStop: true,
    globalConfiguration: !dummy,
    perModelConfiguration: !dummy,
    artifactAcquisition: !dummy,
    remoteConnection: !dummy,
    lifecycle: dummy ? "simulated" : "managed",
  };
}

export function inferenceLabel(details: EndpointDetails): string {
  if (details.chatAvailable) return text.api.chatCompletions;
  return details.embeddingsAvailable ? text.api.embeddings : text.api.completions;
}

export function inferenceUrl(details: EndpointDetails): string {
  if (details.chatAvailable) return details.chatCompletionsUrl;
  return details.embeddingsAvailable ? details.embeddingsUrl : details.completionsUrl;
}

export function curlExample(details: EndpointDetails): string {
  return text.curlExample(details, inferenceUrl(details));
}
