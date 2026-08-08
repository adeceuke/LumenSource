import { createPortal } from "react-dom";
import type { Dispatch, SetStateAction } from "react";
import { browserMessages } from "../i18n";
import {
  formatBytes,
  formatLegalValue,
  inferenceLabel,
  inferenceUrl,
  installProgressMessage,
  preflightCheckCopy,
  progressPercent,
  remoteCheckCopy,
} from "../modelUi";
import type {
  EndpointDetails,
  HardwareProfile,
  InstallProgress,
  InstallationValidationReport,
  PerformanceProfile,
  PerformanceProfileReport,
  PerformanceTestResult,
  PreflightReport,
  Recommendation,
  RemoteConnectionReport,
  RemoteTargetProfile,
  UseIntent,
  WizardStep,
} from "../types";
import { PerformanceTestResults } from "./PerformanceTestPanel";

const text = browserMessages();
const WIZARD_STEPS: WizardStep[] = [
  "location",
  "hardware",
  "intent",
  "suggestion",
  "profile",
  "license",
  "download",
  "ready",
];
const PERFORMANCE_PROFILES: PerformanceProfile[] = ["safe", "balanced", "fast", "custom"];

export interface ModelSetupWizardProps {
  wizardStep: WizardStep;
  wizardLocation: "local" | "remote";
  remoteTargets: RemoteTargetProfile[];
  selectedRemoteTargetId: string;
  selectedRemoteTarget?: RemoteTargetProfile;
  remotePassword: string;
  remotePasswordSaved: boolean;
  rememberRemotePassword: boolean;
  remoteReport?: RemoteConnectionReport;
  remoteCheckLoading: boolean;
  hardwareInfo?: HardwareProfile;
  hardwareLoading: boolean;
  hardwareError?: string;
  useCase: UseIntent;
  busy: boolean;
  recommendations: Recommendation[];
  recommendedModel?: Recommendation;
  selectedModelId: string;
  selectedRecommendation?: Recommendation;
  modelPickerOpen: boolean;
  modelPickerPosition?: { top: number; left: number; width: number };
  preflight?: PreflightReport;
  preflightLoading: boolean;
  performanceProfile: PerformanceProfile;
  profileReport?: PerformanceProfileReport;
  profileLoading: boolean;
  profileError?: string;
  licenseBasis: "catalog" | "separate";
  licenseAcknowledged: boolean;
  separateLicenseReference: string;
  licenseReviewComplete: boolean;
  downloadBusy: boolean;
  installCancellable: boolean;
  cancelBusy: boolean;
  startAfterInstall: boolean;
  installRuntime: boolean;
  runtimeInstallRequired: boolean;
  installProgress?: InstallProgress;
  validationReport?: InstallationValidationReport;
  validationDetailsOpen: boolean;
  performanceTest?: PerformanceTestResult;
  performanceTestBusy: boolean;
  performanceTestError?: string;
  selectedIsDummy: boolean;
  endpoint?: EndpointDetails;
  copiedField?: string;
  setWizardLocation: Dispatch<SetStateAction<"local" | "remote">>;
  setRemoteReport: Dispatch<SetStateAction<RemoteConnectionReport | undefined>>;
  setHardwareInfo: Dispatch<SetStateAction<HardwareProfile | undefined>>;
  setHardwareError: Dispatch<SetStateAction<string | undefined>>;
  setSelectedRemoteTargetId: Dispatch<SetStateAction<string>>;
  setRemotePassword: Dispatch<SetStateAction<string>>;
  setRememberRemotePassword: Dispatch<SetStateAction<boolean>>;
  setUseCase: Dispatch<SetStateAction<UseIntent>>;
  setSelectedModelId: Dispatch<SetStateAction<string>>;
  setModelPickerOpen: Dispatch<SetStateAction<boolean>>;
  setModelPickerPosition: Dispatch<SetStateAction<{ top: number; left: number; width: number } | undefined>>;
  setPerformanceProfile: Dispatch<SetStateAction<PerformanceProfile>>;
  setLicenseBasis: Dispatch<SetStateAction<"catalog" | "separate">>;
  setLicenseAcknowledged: Dispatch<SetStateAction<boolean>>;
  setSeparateLicenseReference: Dispatch<SetStateAction<string>>;
  setStartAfterInstall: Dispatch<SetStateAction<boolean>>;
  setInstallRuntime: Dispatch<SetStateAction<boolean>>;
  setValidationDetailsOpen: Dispatch<SetStateAction<boolean>>;
  cancelWizard: () => void;
  openRemoteTargetDialog: () => void;
  checkRemoteTarget: () => void | Promise<void>;
  goToNextStep: () => void | Promise<void>;
  goToPreviousStep: () => void;
  openPublisherUrl: (url: string) => void | Promise<void>;
  requestInstallCancellation: () => void | Promise<void>;
  retryValidation: () => void | Promise<void>;
  useSaferSettings: () => void | Promise<void>;
  removeIncompleteInstall: () => void | Promise<void>;
  startInstall: () => void | Promise<void>;
  openInstalledModel: () => void;
  runPerformanceTest: () => void | Promise<void>;
  reviewPerformanceAlternative: (modelId: string) => void;
  copyText: (value: string, key: string) => void | Promise<void>;
}

export function ModelSetupWizard(props: ModelSetupWizardProps) {
  const {
    wizardStep,
    wizardLocation,
    remoteTargets,
    selectedRemoteTargetId,
    selectedRemoteTarget,
    remotePassword,
    remotePasswordSaved,
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
    performanceProfile,
    profileReport,
    profileLoading,
    profileError,
    licenseBasis,
    licenseAcknowledged,
    separateLicenseReference,
    licenseReviewComplete,
    downloadBusy,
    installCancellable,
    cancelBusy,
    startAfterInstall,
    installRuntime,
    runtimeInstallRequired,
    installProgress,
    validationReport,
    validationDetailsOpen,
    performanceTest,
    performanceTestBusy,
    performanceTestError,
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
    setPerformanceProfile,
    setLicenseBasis,
    setLicenseAcknowledged,
    setSeparateLicenseReference,
    setStartAfterInstall,
    setInstallRuntime,
    setValidationDetailsOpen,
    cancelWizard,
    openRemoteTargetDialog,
    checkRemoteTarget,
    goToNextStep,
    goToPreviousStep,
    openPublisherUrl,
    requestInstallCancellation,
    retryValidation,
    useSaferSettings,
    removeIncompleteInstall,
    startInstall,
    openInstalledModel,
    runPerformanceTest,
    reviewPerformanceAlternative,
    copyText,
  } = props;

  return (
                  <section className="wizard-card" aria-label={text.wizard.label}>
                <div className="wizard-header">
                  <div>
                    <span className="eyebrow">{text.common.addModel}</span>
                    <h2>{text.wizard.titles[wizardStep]}</h2>
                  </div>
                  <button className="icon-button" type="button" onClick={cancelWizard} aria-label={downloadBusy ? text.wizard.cancelInstallation : text.wizard.cancel} disabled={downloadBusy && (!installCancellable || cancelBusy)}>
                    ✕
                  </button>
                </div>

                <div className="wizard-steps" aria-label={text.wizard.stepsLabel}>
                  {WIZARD_STEPS.map((step, index) => (
                    <span key={step} className={`wizard-step ${wizardStep === step ? "active" : index < WIZARD_STEPS.indexOf(wizardStep) ? "complete" : ""}`}>
                      {index + 1}
                    </span>
                  ))}
                </div>

                {wizardStep === "location" ? (
                  <div className="wizard-body">
                    <p>{text.wizard.location.intro}</p>
                    <div className="choice-list">
                      <button type="button" className={`choice ${wizardLocation === "local" ? "selected" : ""}`} onClick={() => { setWizardLocation("local"); setRemoteReport(undefined); setHardwareInfo(undefined); setHardwareError(undefined); }}>
                        <span className="choice-icon">{text.wizard.location.localIcon}</span>
                        <div>
                          <strong>{text.wizard.location.localTitle}</strong>
                          <small>{text.wizard.location.localDescription}</small>
                        </div>
                        <i />
                      </button>
                      <button type="button" className={`choice ${wizardLocation === "remote" ? "selected" : ""}`} onClick={() => { setWizardLocation("remote"); setHardwareInfo(undefined); setHardwareError(undefined); }}>
                        <span className="choice-icon">{text.wizard.location.remoteIcon}</span>
                        <div>
                          <strong>{text.wizard.location.remoteTitle}</strong>
                          <small>{text.wizard.location.remoteDescription}</small>
                        </div>
                        <i />
                      </button>
                    </div>
                    <p className="runtime-availability-note">
                      <strong>{text.wizard.location.ollamaLabel}</strong> {text.wizard.location.ollamaDescription}
                    </p>
                    {wizardLocation === "remote" && (
                      <div className="remote-target-panel">
                        <div className="remote-scope-note">
                          <strong>{text.wizard.location.previewTitle}</strong>
                          <span>{text.wizard.location.previewDescription}</span>
                        </div>
                        <div className="remote-target-picker">
                          <label>
                            <span>{text.wizard.location.targetMachine}</span>
                            <select value={selectedRemoteTargetId} onChange={(event) => {
                              setSelectedRemoteTargetId(event.target.value);
                              setRemoteReport(undefined);
                              setRemotePassword("");
                              setHardwareInfo(undefined);
                              setHardwareError(undefined);
                            }}>
                              <option value="">{text.wizard.location.selectTarget}</option>
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
                            {text.wizard.location.addTarget}
                          </button>
                        </div>
                        {!remoteTargets.length && (
                          <p className="remote-target-empty">{text.wizard.location.noTargets}</p>
                        )}
                        {selectedRemoteTarget && (
                          <>
                            <div className="remote-target-summary">
                              <div>
                                <strong>{selectedRemoteTarget.targetName}</strong>
                                <span>{selectedRemoteTarget.config.username}@{selectedRemoteTarget.config.host}:{selectedRemoteTarget.config.port}</span>
                              </div>
                              <small>{selectedRemoteTarget.config.authentication === "key"
                                ? text.wizard.location.keyAuthentication
                                : text.wizard.location.passwordAuthentication}</small>
                            </div>
                            {selectedRemoteTarget.config.authentication === "password" && (
                              <>
                                <label className="remote-password-field">
                                  <span>{text.wizard.location.password}</span>
                                  <input
                                    type="password"
                                    value={remotePassword}
                                    placeholder={remotePasswordSaved && !remotePassword ? "********" : undefined}
                                    onChange={(event) => { setRemotePassword(event.target.value); setRemoteReport(undefined); }}
                                    autoComplete="current-password"
                                  />
                                  <small>{text.wizard.location.passwordHint}</small>
                                </label>
                                <label className="credential-save-choice wizard-credential-choice">
                                  <input
                                    type="checkbox"
                                    checked={rememberRemotePassword}
                                    onChange={(event) => setRememberRemotePassword(event.target.checked)}
                                  />
                                  <span>
                                    <strong>{text.machines.savePassword}</strong>
                                    <small>{text.machines.savePasswordHint}</small>
                                  </span>
                                </label>
                              </>
                            )}
                            <button className="secondary-button remote-check-button" type="button" onClick={() => void checkRemoteTarget()} disabled={remoteCheckLoading}>
                              {remoteCheckLoading ? text.wizard.location.checkingConnection : text.wizard.location.checkConnection}
                            </button>
                          </>
                        )}
                        {remoteReport && (
                          <div className="remote-checks" aria-live="polite">
                            {remoteReport.checks.map((check) => (
                              <div className={`remote-check ${check.status}`} key={check.id}>
                                <span>{check.status === "pass" ? "✓" : "!"}</span>
                                <div><strong>{remoteCheckCopy(check).label}</strong><p>{remoteCheckCopy(check).detail}</p>{check.guidance && <small>{check.guidance}</small>}</div>
                              </div>
                            ))}
                          </div>
                        )}
                      </div>
                    )}
                    <div className="actions">
                      <button className="primary-button" type="button" onClick={goToNextStep} disabled={wizardLocation === "remote" && !remoteReport?.canContinue}>
                        {text.common.continue}
                      </button>
                    </div>
                  </div>
                ) : wizardStep === "hardware" ? (
                  <div className="wizard-body">
                    <p>{wizardLocation === "remote"
                      ? text.wizard.hardware.remoteIntro(remoteReport?.targetName ?? text.wizard.hardware.remoteTargetFallback)
                      : text.wizard.hardware.localIntro}</p>
                    <div className="hardware-summary">
                      <div className="scan-card">
                        <div className="scan-visual"><span>⌘</span><i /><i /></div>
                        <div>
                          <strong>
                            {hardwareLoading
                              ? text.wizard.hardware.detecting
                              : hardwareError
                                ? text.wizard.hardware.failed
                                : wizardLocation === "remote" ? text.wizard.hardware.remoteDetected : text.wizard.hardware.detected}
                          </strong>
                          <p>
                            {hardwareLoading
                              ? wizardLocation === "remote" ? text.wizard.hardware.scanningRemote : text.wizard.hardware.scanningLocal
                              : hardwareError ?? hardwareInfo?.platform}
                          </p>
                        </div>
                      </div>
                      {hardwareInfo && (
                        <div className="hardware-grid">
                          <div className="hardware-card">
                            <h3>{text.wizard.hardware.cpu}</h3>
                            <ul>
                              <li><span>{text.common.model}</span><strong>{hardwareInfo.cpu}</strong></li>
                              <li><span>{text.wizard.hardware.logicalCores}</span><strong>{hardwareInfo.cpuCores}</strong></li>
                              <li><span>{text.wizard.hardware.frequency}</span><strong>{hardwareInfo.cpuFrequencyMhz ? text.format.megahertz(hardwareInfo.cpuFrequencyMhz) : text.common.unavailable}</strong></li>
                            </ul>
                          </div>
                          <div className="hardware-card">
                            <h3>{text.wizard.hardware.memory}</h3>
                            <ul>
                              <li><span>{text.wizard.hardware.capacity}</span><strong>{formatBytes(hardwareInfo.memoryBytes)}</strong></li>
                              <li><span>{text.wizard.hardware.type}</span><strong>{hardwareInfo.memoryKind ?? text.common.unavailable}</strong></li>
                              <li><span>{text.wizard.hardware.speed}</span><strong>{hardwareInfo.memorySpeedMts ? text.format.transfersPerSecond(hardwareInfo.memorySpeedMts) : text.common.unavailable}</strong></li>
                            </ul>
                          </div>
                          <div className="hardware-card">
                            <h3>{text.wizard.hardware.gpu}</h3>
                            {hardwareInfo.gpu ? (
                              <ul>
                                <li><span>{text.common.model}</span><strong>{hardwareInfo.gpu.name}</strong></li>
                                {hardwareInfo.gpu.memoryBytes !== undefined && (
                                  <li><span>{text.wizard.hardware.vram}</span><strong>{formatBytes(hardwareInfo.gpu.memoryBytes)}</strong></li>
                                )}
                                <li><span>{text.wizard.hardware.backend}</span><strong>{hardwareInfo.gpu.backend}</strong></li>
                              </ul>
                            ) : (
                              <p>{text.wizard.hardware.noGpu}</p>
                            )}
                          </div>
                          <div className="hardware-card">
                            <h3>{text.wizard.hardware.storage}</h3>
                            <ul>
                              <li><span>{text.wizard.hardware.mount}</span><strong>{hardwareInfo.storage.mountPoint}</strong></li>
                              <li><span>{text.wizard.hardware.freeSpace}</span><strong>{formatBytes(hardwareInfo.storage.availableBytes)}</strong></li>
                              <li><span>{text.wizard.hardware.capacity}</span><strong>{formatBytes(hardwareInfo.storage.totalBytes)}</strong></li>
                            </ul>
                          </div>
                        </div>
                      )}
                      {hardwareError && (
                        <button className="secondary-button" type="button" onClick={() => {
                          setHardwareError(undefined);
                          setHardwareInfo(undefined);
                        }}>
                          {text.wizard.hardware.retry}
                        </button>
                      )}
                    </div>
                    <div className="actions">
                      <button className="back-button" type="button" onClick={goToPreviousStep}>
                        {text.common.back}
                      </button>
                      <button className="primary-button" type="button" onClick={goToNextStep} disabled={hardwareLoading || !hardwareInfo}>
                        {text.common.continue}
                      </button>
                    </div>
                  </div>
                ) : wizardStep === "intent" ? (
                  <div className="wizard-body">
                    <p>{text.wizard.intent.intro}</p>
                    <div className="choice-list">
                      <button type="button" className={`choice ${useCase === "code" ? "selected" : ""}`} onClick={() => setUseCase("code")}>
                        <span className="choice-icon">⌨</span>
                        <div>
                          <strong>{text.wizard.intent.codeTitle}</strong>
                          <small>{text.wizard.intent.codeDescription}</small>
                        </div>
                        <i />
                      </button>
                      <button type="button" className={`choice ${useCase === "chat" ? "selected" : ""}`} onClick={() => setUseCase("chat")}>
                        <span className="choice-icon">☁</span>
                        <div>
                          <strong>{text.wizard.intent.chatTitle}</strong>
                          <small>{text.wizard.intent.chatDescription}</small>
                        </div>
                        <i />
                      </button>
                      <button type="button" className={`choice ${useCase === "creative" ? "selected" : ""}`} onClick={() => setUseCase("creative")}>
                        <span className="choice-icon">✦</span>
                        <div>
                          <strong>{text.wizard.intent.creativeTitle}</strong>
                          <small>{text.wizard.intent.creativeDescription}</small>
                        </div>
                        <i />
                      </button>
                      <button type="button" className={`choice ${useCase === "research" ? "selected" : ""}`} onClick={() => setUseCase("research")}>
                        <span className="choice-icon">🔎</span>
                        <div>
                          <strong>{text.wizard.intent.researchTitle}</strong>
                          <small>{text.wizard.intent.researchDescription}</small>
                        </div>
                        <i />
                      </button>
                    </div>
                    <div className="actions">
                      <button className="back-button" type="button" onClick={goToPreviousStep}>
                        {text.common.back}
                      </button>
                      <button className="primary-button" type="button" onClick={goToNextStep} disabled={busy}>
                        {busy ? text.wizard.intent.finding : text.common.continue}
                      </button>
                    </div>
                  </div>
                ) : wizardStep === "suggestion" ? (
                  <div className="wizard-body">
                    <p>{wizardLocation === "remote"
                      ? text.wizard.suggestion.remoteIntro
                      : text.wizard.suggestion.localIntro}</p>
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
                            <b>{text.wizard.suggestion.recommended}</b>
                            <strong>{recommendedModel.name}</strong>
                            <em>{recommendedModel.provider}</em>
                          </span>
                          <small>{wizardLocation === "remote" ? text.wizard.suggestion.remoteBestMatch : text.wizard.suggestion.localBestMatch}</small>
                          <i className="radio-dot" />
                        </button>
                      )}
                      <div className="catalog-model-picker" data-catalog-model-picker>
                        <span>{text.wizard.suggestion.chooseAll}</span>
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
                              <strong>{selectedRecommendation?.name ?? text.wizard.suggestion.selectModel}</strong>
                              {selectedRecommendation && (
                                <small>{selectedRecommendation.provider} · {selectedRecommendation.recommended ? text.wizard.suggestion.recommended : selectedRecommendation.compatible ? selectedRecommendation.fit : text.wizard.suggestion.incompatible}</small>
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
                            aria-label={text.wizard.suggestion.availableModels}
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
                                <em>{recommendation.recommended ? text.wizard.suggestion.recommended : recommendation.compatible ? recommendation.fit : text.wizard.suggestion.incompatible}</em>
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
                            {selectedRecommendation.recommended ? text.wizard.suggestion.recommended : selectedRecommendation.fit}
                          </span>
                        </div>
                        <div className="publisher-identity">
                          <small>{text.wizard.suggestion.publisher}</small>
                          <strong>{selectedRecommendation.provider}</strong>
                        </div>
                        <h3>{selectedRecommendation.name}</h3>
                        {selectedRecommendation.labels.length > 0 && (
                          <div className="model-labels">
                            {selectedRecommendation.labels.map((label) => <span key={label}>{label}</span>)}
                          </div>
                        )}
                        <div className="model-version">
                          <span>{text.wizard.suggestion.version}</span>
                          <code>{selectedRecommendation.modelId}</code>
                        </div>
                        <p>{selectedRecommendation.description}</p>
                        <div className="model-stats">
                          <span><b>{formatBytes(selectedRecommendation.sizeBytes)}</b>{text.wizard.suggestion.storage}</span>
                          <span><b>{selectedRecommendation.contextWindow.toLocaleString(text.locale)}</b>{text.wizard.suggestion.context}</span>
                          <span><b>{selectedRecommendation.version}</b>{text.wizard.suggestion.runtime}</span>
                          <span><b>{formatBytes(selectedRecommendation.estimatedLoadedMemoryMinBytes)} - {formatBytes(selectedRecommendation.estimatedLoadedMemoryMaxBytes)}</b>{text.wizard.suggestion.loadedMemory}</span>
                          {selectedRecommendation.externalEvaluations.map((evaluation) => (
                            <span
                              key={`${evaluation.publisher}-${evaluation.leaderboardName}`}
                              title={`${evaluation.sourceModelName}: ${evaluation.notes}`}
                            >
                              <b>{evaluation.overallTier}</b>
                              {text.wizard.suggestion.externalOverallTier(evaluation.publisher, evaluation.leaderboardName)}
                            </span>
                          ))}
                        </div>
                        <h4>{selectedRecommendation.compatible ? text.wizard.suggestion.why : text.wizard.suggestion.compatibilityIssues}</h4>
                        <ul className="ranking-reasons">
                          {selectedRecommendation.reasons.map((reason) => (
                            <li key={reason}>{selectedRecommendation.compatible ? "✓" : "!"} {reason}</li>
                          ))}
                        </ul>
                        <div className="inline-requirements" aria-live="polite">
                          <h5>{text.wizard.suggestion.checks}</h5>
                          {preflightLoading ? (
                            <div className="requirements-loading"><span className="model-control-spinner" /> {text.wizard.suggestion.checkingMachine}</div>
                          ) : (
                            <div className="check-list compact">
                              {preflight?.checks.map((item) => (
                                <div className={`check ${item.status}`} key={item.id}>
                                  <span className="check-status" role="img" aria-label={`${preflightCheckCopy(item).label}: ${item.status}`}>
                                    {item.status === "pass" ? "✓" : item.status === "warning" ? "~" : "!"}
                                  </span>
                                  <div>
                                    <strong>{preflightCheckCopy(item).label}</strong>
                                    <small>{item.id === "storage" && preflight
                                      ? text.wizard.suggestion.requiredSpace(formatBytes(preflight.requiredBytes), formatBytes(preflight.availableBytes))
                                      : preflightCheckCopy(item).detail}</small>
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
                        {text.common.back}
                      </button>
                      <button className="primary-button" type="button" onClick={goToNextStep} disabled={!selectedModelId || preflightLoading || !preflight?.canInstall}>
                        {preflightLoading ? text.wizard.suggestion.checkingRequirements : preflight?.canInstall ? text.wizard.suggestion.continueToLicense : text.wizard.suggestion.requirementsNotMet}
                      </button>
                    </div>
                  </div>
                ) : wizardStep === "profile" ? (
                  <div className="wizard-body">
                    <p>{text.wizard.profile.intro}</p>
                    <div className="profile-picker">
                      {PERFORMANCE_PROFILES.map((profile) => {
                        const title = text.wizard.profile[`${profile}Title`];
                        const description = text.wizard.profile[`${profile}Description`];
                        return (
                          <button
                            className={`profile-option ${performanceProfile === profile ? "selected" : ""}`}
                            type="button"
                            key={profile}
                            aria-pressed={performanceProfile === profile}
                            onClick={() => setPerformanceProfile(profile)}
                          >
                            <span>
                              <strong>{title}</strong>
                              {profile === "balanced" && <em>{text.wizard.profile.recommended}</em>}
                            </span>
                            <small>{description}</small>
                          </button>
                        );
                      })}
                    </div>
                    <section className="profile-report" aria-live="polite">
                      {profileLoading ? (
                        <div className="requirements-loading"><span className="model-control-spinner" /> {text.wizard.profile.calculating}</div>
                      ) : profileError ? (
                        <p className="profile-warning">{profileError}</p>
                      ) : profileReport ? (
                        <>
                          <h3>{text.wizard.profile.calculated}</h3>
                          <p>{profileReport.summary}</p>
                          <dl>
                            <div><dt>{text.wizard.profile.accelerator}</dt><dd>{profileReport.accelerator}</dd></div>
                            <div><dt>{text.wizard.profile.context}</dt><dd>{profileReport.contextLength?.toLocaleString(text.locale) ?? text.wizard.profile.runtimeDefault}</dd></div>
                            <div><dt>{text.wizard.profile.concurrency}</dt><dd>{profileReport.concurrentRequests || text.wizard.profile.runtimeDefault}</dd></div>
                            <div><dt>{text.wizard.profile.availableMemory}</dt><dd>{formatBytes(profileReport.availableMemoryBytes)}</dd></div>
                            <div><dt>{text.wizard.profile.minimumMemory}</dt><dd>{formatBytes(profileReport.minimumMemoryBytes)}</dd></div>
                          </dl>
                          {profileReport.warnings.map((warning) => <p className="profile-warning" key={warning}>{warning}</p>)}
                        </>
                      ) : null}
                    </section>
                    <div className="actions">
                      <button className="back-button" type="button" onClick={goToPreviousStep}>{text.common.back}</button>
                      <button
                        className="primary-button"
                        type="button"
                        onClick={goToNextStep}
                        disabled={profileLoading || !profileReport?.fitsDetectedMemory}
                      >
                        {text.wizard.profile.continue}
                      </button>
                    </div>
                  </div>
                ) : wizardStep === "license" ? (
                  <div className="wizard-body">
                    <p>{text.wizard.license.intro(selectedRecommendation?.name ?? text.wizard.install.modelFallback)}</p>
                    <p className="license-legal-notice">{text.wizard.license.notice}</p>
                    {selectedRecommendation && (
                      <>
                        <article className={`license-card ${selectedRecommendation.license.commercialUse === "not-permitted" ? "restricted" : ""}`}>
                          <div className="license-heading">
                            <div>
                              <div className="publisher-identity">
                                <small>{text.wizard.suggestion.publisher}</small>
                                <strong>{selectedRecommendation.provider}</strong>
                              </div>
                              <span className="license-classification">{formatLegalValue(selectedRecommendation.license.classification)}</span>
                              <h3>{selectedRecommendation.license.name}</h3>
                            </div>
                            {selectedRecommendation.license.reviewedAt && <small>{text.wizard.license.reviewed(selectedRecommendation.license.reviewedAt)}</small>}
                          </div>
                          <p>{selectedRecommendation.license.summary || text.wizard.license.fallbackSummary}</p>
                          <div className="license-permissions">
                            <span className={selectedRecommendation.license.commercialUse === "not-permitted" ? "denied" : ""}>
                              <small>{text.wizard.license.commercialUse}</small><b>{formatLegalValue(selectedRecommendation.license.commercialUse)}</b>
                            </span>
                            <span><small>{text.wizard.license.redistribution}</small><b>{formatLegalValue(selectedRecommendation.license.redistribution)}</b></span>
                            <span><small>{text.wizard.license.derivatives}</small><b>{formatLegalValue(selectedRecommendation.license.derivatives)}</b></span>
                          </div>
                          <div className="license-compliance">
                            <span><small>{text.wizard.license.attribution}</small><b>{formatLegalValue(selectedRecommendation.license.attribution)}</b></span>
                            <span><small>{text.wizard.license.licenseCopy}</small><b>{formatLegalValue(selectedRecommendation.license.licenseText)}</b></span>
                            <span><small>{text.wizard.license.noticeFile}</small><b>{formatLegalValue(selectedRecommendation.license.notice)}</b></span>
                          </div>
                          {selectedRecommendation.license.geographicRestrictions.length > 0 && (
                            <section className="license-alert">
                              <h4>{text.wizard.license.geographicRestrictions}</h4>
                              <ul>{selectedRecommendation.license.geographicRestrictions.map((item) => <li key={item}>{item}</li>)}</ul>
                            </section>
                          )}
                          <div className="license-conditions">
                            <section>
                              <h4>{text.wizard.license.obligations}</h4>
                              {selectedRecommendation.license.obligations.length > 0
                                ? <ul>{selectedRecommendation.license.obligations.map((item) => <li key={item}>{item}</li>)}</ul>
                                : <p>{text.wizard.license.noObligations}</p>}
                            </section>
                            <section>
                              <h4>{text.wizard.license.restrictions}</h4>
                              {selectedRecommendation.license.restrictions.length > 0
                                ? <ul>{selectedRecommendation.license.restrictions.map((item) => <li key={item}>{item}</li>)}</ul>
                                : <p>{text.wizard.license.noRestrictions}</p>}
                            </section>
                          </div>
                          <div className="license-links">
                            {selectedRecommendation.license.url && <button type="button" onClick={() => void openPublisherUrl(selectedRecommendation.license.url!)}>{text.wizard.license.fullLicense}</button>}
                            {selectedRecommendation.license.usagePolicyUrl && <button type="button" onClick={() => void openPublisherUrl(selectedRecommendation.license.usagePolicyUrl!)}>{text.wizard.license.usagePolicy}</button>}
                          </div>
                        </article>

                        <fieldset className="license-basis">
                          <legend>{text.wizard.license.permissionLegend}</legend>
                          <label className={licenseBasis === "catalog" ? "selected" : ""}>
                            <input type="radio" name="license-basis" checked={licenseBasis === "catalog"} onChange={() => { setLicenseBasis("catalog"); setLicenseAcknowledged(false); }} />
                            <span><strong>{text.wizard.license.catalogTitle}</strong><small>{text.wizard.license.catalogDescription}</small></span>
                          </label>
                          <label className={licenseBasis === "separate" ? "selected" : ""}>
                            <input type="radio" name="license-basis" checked={licenseBasis === "separate"} onChange={() => { setLicenseBasis("separate"); setLicenseAcknowledged(false); }} />
                            <span><strong>{text.wizard.license.separateTitle}</strong><small>{text.wizard.license.separateDescription}</small></span>
                          </label>
                        </fieldset>

                        {licenseBasis === "separate" ? (
                          <div className="separate-license-form">
                            <label>
                              <span>{text.wizard.license.agreementReference}</span>
                              <textarea
                                rows={3}
                                value={separateLicenseReference}
                                onChange={(event) => setSeparateLicenseReference(event.target.value)}
                                placeholder={text.wizard.license.agreementPlaceholder}
                              />
                              <small>{text.wizard.license.agreementHint}</small>
                            </label>
                            <label className="license-confirmation">
                              <input type="checkbox" checked={licenseAcknowledged} onChange={(event) => setLicenseAcknowledged(event.target.checked)} />
                              <span>{text.wizard.license.separateConfirmation}</span>
                            </label>
                          </div>
                        ) : selectedRecommendation.license.requiresUserAcceptance ? (
                          <label className="license-confirmation required">
                            <input type="checkbox" checked={licenseAcknowledged} onChange={(event) => setLicenseAcknowledged(event.target.checked)} />
                            <span>{text.wizard.license.catalogConfirmation}</span>
                          </label>
                        ) : (
                          <p className="license-information-note">{text.wizard.license.informational}</p>
                        )}
                      </>
                    )}
                    <div className="actions">
                      <button className="back-button" type="button" onClick={goToPreviousStep}>
                        {text.common.back}
                      </button>
                      <button className="primary-button" type="button" onClick={goToNextStep} disabled={!licenseReviewComplete}>
                        {licenseReviewComplete ? text.wizard.license.continue : text.wizard.license.review}
                      </button>
                    </div>
                  </div>
                ) : wizardStep === "download" ? (
                  <div className="wizard-body">
                    <p>{wizardLocation === "remote"
                      ? text.wizard.install.remoteIntro(remoteReport?.targetName ?? text.wizard.hardware.remoteTargetFallback)
                      : text.wizard.install.localIntro}</p>
                    {runtimeInstallRequired && (
                      <label className="install-option">
                        <input
                          type="checkbox"
                          checked={installRuntime}
                          onChange={(event) => setInstallRuntime(event.target.checked)}
                          disabled={downloadBusy}
                        />
                        <span>
                          <strong>{text.wizard.install.installOllama}</strong>
                          <small>{text.wizard.install.installOllamaHint}</small>
                        </span>
                      </label>
                    )}
                    <label className="install-option">
                      <input
                        type="checkbox"
                        checked={startAfterInstall}
                        onChange={(event) => setStartAfterInstall(event.target.checked)}
                        disabled={downloadBusy}
                      />
                      <span>
                        <strong>{text.wizard.install.startAfter}</strong>
                        <small>{text.wizard.install.startAfterHint}</small>
                      </span>
                    </label>
                    <div className="install-stage">
                      <div className="model-orb"><span>⬇</span><i /></div>
                      <h3>{downloadBusy ? text.wizard.install.inProgress : text.wizard.install.title(selectedRecommendation?.name ?? text.wizard.install.modelFallback)}</h3>
                      <p>{installProgressMessage(installProgress)}</p>
                      {installProgress?.phase === "validating" && (
                        <p className="install-phase-note" role="status">{text.wizard.install.validationWait}</p>
                      )}
                      {installProgress && (
                        <>
                          <div className="progress-track" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(progressPercent(installProgress))}>
                            <i style={{ width: `${progressPercent(installProgress)}%` }} />
                          </div>
                          <div className="progress-meta">
                            <span>{installProgress.phase}</span>
                            {installProgress.currentItem && installProgress.totalItems && (
                              <span className="progress-items">{text.wizard.install.downloadItem(installProgress.currentItem, installProgress.totalItems)}</span>
                            )}
                            <b>{Math.round(progressPercent(installProgress))}%</b>
                          </div>
                        </>
                      )}
                    </div>
                    {validationReport && !validationReport.passed && (
                      <section className="validation-recovery" aria-live="polite">
                        <h3>{text.wizard.validation.failedTitle}</h3>
                        <p>{validationReport.message}</p>
                        <div className="validation-actions">
                          <button className="secondary-button" type="button" disabled={downloadBusy} onClick={() => void retryValidation()}>
                            {text.wizard.validation.retry}
                          </button>
                          <button className="secondary-button" type="button" disabled={downloadBusy || performanceProfile === "safe"} onClick={() => void useSaferSettings()}>
                            {text.wizard.validation.safer}
                          </button>
                          <button className="secondary-button" type="button" onClick={() => setValidationDetailsOpen((open) => !open)}>
                            {validationDetailsOpen ? text.wizard.validation.hideDiagnostics : text.wizard.validation.openDiagnostics}
                          </button>
                          <button className="danger-button" type="button" disabled={downloadBusy} onClick={() => void removeIncompleteInstall()}>
                            {text.wizard.validation.removeIncomplete}
                          </button>
                        </div>
                        {validationDetailsOpen && (
                          <div className="validation-checks">
                            {validationReport.checks.map((check) => (
                              <div className={`validation-check ${check.status}`} key={check.id}>
                                <strong>{check.id}</strong>
                                <span>{check.detail}</span>
                              </div>
                            ))}
                          </div>
                        )}
                      </section>
                    )}
                    <div className="actions">
                      {downloadBusy ? (
                        <button className="secondary-button" type="button" onClick={() => void requestInstallCancellation()} disabled={!installCancellable || cancelBusy}>
                          {cancelBusy ? text.wizard.install.cancelling : text.wizard.cancelInstallation}
                        </button>
                      ) : (
                        <button className="back-button" type="button" onClick={goToPreviousStep}>
                          {text.common.back}
                        </button>
                      )}
                      <button className="primary-button" type="button" onClick={startInstall} disabled={downloadBusy || Boolean(validationReport && !validationReport.passed) || !preflight?.canInstall || (runtimeInstallRequired && !installRuntime)}>
                        {downloadBusy ? text.wizard.install.installing : startAfterInstall ? text.wizard.install.installAndStart : text.wizard.install.install}
                      </button>
                    </div>
                  </div>
                ) : (
                  <div className="wizard-body">
                    <p>{selectedIsDummy
                      ? text.wizard.ready.dummyIntro
                      : startAfterInstall
                      ? wizardLocation === "remote"
                        ? text.wizard.ready.remoteRunning(remoteReport?.targetName ?? text.wizard.hardware.remoteTargetFallback)
                        : text.wizard.ready.localRunning
                      : text.wizard.ready.notLoaded}</p>
                    <div className="success-mark">✓</div>
                    {validationReport?.passed && (
                      <div className="validation-success">
                        <strong>{text.wizard.validation.passedTitle}</strong>
                        <span>{validationReport.message}</span>
                        <small>
                          {validationReport.accelerator
                            ? text.wizard.validation.allocation(validationReport.accelerator)
                            : text.wizard.validation.metricsUnavailable}
                          {validationReport.effectiveContextLength
                            ? ` · ${text.wizard.validation.context(validationReport.effectiveContextLength)}`
                            : ""}
                        </small>
                      </div>
                    )}
                    {!selectedIsDummy && validationReport?.runtimeId === "ollama" && (
                      <section className="wizard-performance-test">
                        <div>
                          <strong>Optional performance test</strong>
                          <span>Measure first-token latency, prompt processing, generation speed, memory, and actual CPU/GPU allocation. Poor performance will not invalidate this installation.</span>
                        </div>
                        {!performanceTest && (
                          <button className="secondary-button" type="button" disabled={performanceTestBusy} onClick={() => void runPerformanceTest()}>
                            {performanceTestBusy ? "Testing…" : "Run performance test"}
                          </button>
                        )}
                        {performanceTestBusy && (
                          <p className="performance-test-note" role="status">Running a warm-up and two measured requests. This can take several minutes on CPU-only models.</p>
                        )}
                        {performanceTestError && <div className="inline-error" role="alert">{performanceTestError}</div>}
                        {performanceTest && (
                          <>
                            <PerformanceTestResults result={performanceTest} compact onReviewAlternative={reviewPerformanceAlternative} />
                            <button className="secondary-button" type="button" disabled={performanceTestBusy} onClick={() => void runPerformanceTest()}>Run again</button>
                          </>
                        )}
                      </section>
                    )}
                    <div className={`runtime-hero ${startAfterInstall ? "" : "stopped"}`}>
                      <div><span className="pulse" /> {selectedIsDummy
                        ? startAfterInstall ? text.wizard.ready.testActive : text.wizard.ready.testInstalled
                        : startAfterInstall ? text.wizard.ready.running : text.wizard.ready.notLoadedState}</div>
                      <h3>{selectedRecommendation?.name}</h3>
                      <p>{selectedIsDummy
                        ? text.wizard.ready.dummyDetails
                        : startAfterInstall
                        ? text.wizard.ready.runningDetails
                        : text.wizard.ready.stoppedDetails}</p>
                    </div>
                    <div className="endpoint-card">
                      <div>
                        <span>{text.wizard.ready.baseUrl}</span>
                        <div className="copy-value"><code>{endpoint?.baseUrl}</code><button className={copiedField === "ready-base" ? "copied" : ""} type="button" aria-live="polite" onClick={() => endpoint && void copyText(endpoint.baseUrl, "ready-base")}>{copiedField === "ready-base" ? text.common.copied : text.common.copy}</button></div>
                      </div>
                      <div>
                        <span>{endpoint ? inferenceLabel(endpoint) : text.wizard.ready.inferenceEndpoint}</span>
                        <div className="copy-value"><code>{endpoint ? inferenceUrl(endpoint) : undefined}</code><button className={copiedField === "ready-inference" ? "copied" : ""} type="button" aria-live="polite" onClick={() => endpoint && void copyText(inferenceUrl(endpoint), "ready-inference")}>{copiedField === "ready-inference" ? text.common.copied : text.common.copy}</button></div>
                      </div>
                      <div>
                        <span>{text.common.model}</span>
                        <div className="copy-value"><code>{endpoint?.model}</code><button className={copiedField === "ready-model" ? "copied" : ""} type="button" aria-live="polite" onClick={() => endpoint && void copyText(endpoint.model, "ready-model")}>{copiedField === "ready-model" ? text.common.copied : text.common.copy}</button></div>
                      </div>
                      <div>
                        <span>{text.wizard.ready.apiKey}</span>
                        <div className="copy-value"><code>{endpoint?.apiKeyRequired ? text.common.required : text.wizard.ready.notRequired}</code></div>
                      </div>
                    </div>
                    <div className="actions">
                      <button className="primary-button" type="button" onClick={openInstalledModel}>{text.wizard.ready.returnToLibrary}</button>
                    </div>
                  </div>
                )}
              </section>

  );
}
