import type { KeyboardEvent } from "react";
import { useModelChat } from "../hooks/useModelChat";
import { usePerformanceMonitor } from "../hooks/usePerformanceMonitor";
import { browserMessages } from "../i18n";
import { curlExample, formatBytes, inferenceLabel, inferenceUrl } from "../modelUi";
import type { RunningModelEntry } from "../types";
import type { ApplicationSettings } from "../types";
import { IntegrationPanel } from "./IntegrationPanel";
import { LumenChatWorkspace } from "./LumenChatWorkspace";
import { ModelSettingsPanel } from "./ModelSettingsPanel";
import { PerformanceChart } from "./PerformanceChart";
import { VllmModelSettings } from "./VllmModelSettings";

const text = browserMessages();

export type DetailTab = "chat" | "logs" | "performance" | "api" | "integration" | "settings";

interface ModelDetailsProps {
  model: RunningModelEntry;
  models: RunningModelEntry[];
  applicationSettings?: ApplicationSettings;
  tab: DetailTab;
  modelAction?: { id: string; action: "starting" | "stopping" };
  copiedField?: string;
  errorFrom: (error: unknown) => string;
  onTab: (tab: DetailTab) => void;
  onBack: () => void;
  onToggleRunning: () => void;
  onClearLogs: () => void;
  onCopy: (value: string, feedbackKey: string) => void;
  onSettingsChanged: (settings: ApplicationSettings) => void;
  onStartModel: (modelId: string) => void;
  onModelSaved: (model: RunningModelEntry) => void;
  onInventoryChanged: (models: RunningModelEntry[]) => void;
}

export function ModelDetails({
  model,
  models,
  applicationSettings,
  tab,
  modelAction,
  copiedField,
  errorFrom,
  onTab,
  onBack,
  onToggleRunning,
  onClearLogs,
  onCopy,
  onSettingsChanged,
  onStartModel,
  onModelSaved,
  onInventoryChanged,
}: ModelDetailsProps) {
  const performance = usePerformanceMonitor(model, errorFrom);
  const chat = useModelChat(model, tab === "api", errorFrom);

  return (
    <section className="model-detail-page" aria-label={text.details.aria(model.name)}>
      <div className="models-toolbar detail-toolbar">
        <div>
          <span className="eyebrow">{text.details.label}</span>
          <div className="model-title-line">
            <h1>{model.name}</h1>
            <button className="back-to-models" type="button" onClick={onBack}>{text.details.back}</button>
          </div>
        </div>
        <div className="detail-runtime-actions">
          <span className={`runtime-state ${model.running ? "running" : "stopped"}`}><i /> {model.running ? text.details.running : text.details.stopped}</span>
          <button
            className="secondary-button detail-runtime-button"
            type="button"
            onClick={onToggleRunning}
            disabled={!model.runtimeCapabilities.modelStartStop || modelAction !== undefined}
          >
            {modelAction?.id === model.id ? (
              <><span className="model-control-spinner" /> {modelAction.action === "starting" ? text.details.starting : text.details.stopping}</>
            ) : model.running ? (
              <><span className="model-control-icon stop" /> {text.details.stop}</>
            ) : (
              <><span className="model-control-icon start" /> {text.details.start}</>
            )}
          </button>
        </div>
      </div>

      <nav className="detail-tabs" aria-label={text.details.tabsLabel}>
        <button type="button" className={tab === "chat" ? "active" : ""} onClick={() => onTab("chat")}>{text.chat.tab}</button>
        <button type="button" className={tab === "logs" ? "active" : ""} onClick={() => onTab("logs")}>{text.common.logs}</button>
        <button type="button" className={tab === "performance" ? "active" : ""} onClick={() => onTab("performance")}>{text.common.performance}</button>
        <button type="button" className={tab === "api" ? "active" : ""} onClick={() => onTab("api")}>{text.common.api}</button>
        <button type="button" className={tab === "integration" ? "active" : ""} onClick={() => onTab("integration")}>Integration</button>
        <button type="button" className={tab === "settings" ? "active" : ""} onClick={() => onTab("settings")}>{text.navigation.settings}</button>
      </nav>

      {tab === "logs" ? (
        <LogsPanel model={model} copiedField={copiedField} onClear={onClearLogs} onCopy={onCopy} />
      ) : tab === "performance" ? (
        <PerformancePanel model={model} performance={performance} />
      ) : tab === "chat" ? (
        <LumenChatWorkspace
          models={models}
          settings={applicationSettings}
          initialModelId={model.id}
          copiedField={copiedField}
          errorFrom={errorFrom}
          onCopy={onCopy}
          onSettingsChanged={onSettingsChanged}
          onStartModel={onStartModel}
        />
      ) : tab === "api" ? (
        <ApiPanel model={model} chat={chat} copiedField={copiedField} onCopy={onCopy} />
      ) : tab === "integration" ? (
        <IntegrationPanel model={model} copiedField={copiedField} errorFrom={errorFrom} onCopy={onCopy} />
      ) : (
        <div className="model-settings-stack">
          {applicationSettings ? (
            <ModelSettingsPanel
              model={model}
              applicationSettings={applicationSettings}
              onSaved={onModelSaved}
              onInventoryChanged={onInventoryChanged}
            />
          ) : (
            <section className="detail-section">
              <div className="details-empty full-detail-empty">{text.common.loading}</div>
            </section>
          )}
          {model.runtimeId === "vllm" && model.runtimeCapabilities.lifecycle === "external" && (
            <VllmModelSettings model={model} onSaved={onModelSaved} />
          )}
        </div>
      )}
    </section>
  );
}

function LogsPanel({
  model,
  copiedField,
  onClear,
  onCopy,
}: {
  model: RunningModelEntry;
  copiedField?: string;
  onClear: () => void;
  onCopy: (value: string, key: string) => void;
}) {
  const copyKey = `logs-${model.id}`;
  return (
    <section className="detail-section" aria-labelledby="model-logs-title">
      <div className="detail-section-header">
        <div>
          <h2 id="model-logs-title">{text.logs.title}</h2>
          <p>{text.logs.description}</p>
        </div>
        <div className="detail-section-actions">
          <button className="secondary-button" type="button" onClick={onClear} disabled={model.logs.length === 0}>{text.common.clear}</button>
          <button className="secondary-button" type="button" aria-live="polite" onClick={() => onCopy(model.logs.join("\n"), copyKey)} disabled={model.logs.length === 0}>{copiedField === copyKey ? text.common.copied : text.logs.copy}</button>
        </div>
      </div>
      {model.logs.length > 0 ? (
        <ol className="log-viewer full-log-viewer">
          {model.logs.map((log, index) => <li key={`${index}-${log}`}>{log}</li>)}
        </ol>
      ) : (
        <div className="details-empty full-detail-empty">{text.logs.empty}</div>
      )}
    </section>
  );
}

type PerformanceState = ReturnType<typeof usePerformanceMonitor>;

function PerformancePanel({ model, performance }: { model: RunningModelEntry; performance: PerformanceState }) {
  if (model.runtimeCapabilities.lifecycle === "external") {
    return (
      <section className="detail-section" aria-labelledby="model-performance-title">
        <div className="detail-section-header">
          <div>
            <h2 id="model-performance-title">{text.performance.title}</h2>
            <p>{text.performance.externalDescription}</p>
          </div>
        </div>
        <div className="details-empty full-detail-empty">{text.performance.externalUnavailable}</div>
      </section>
    );
  }
  return (
    <section className="detail-section" aria-labelledby="model-performance-title">
      <div className="detail-section-header">
        <div>
          <h2 id="model-performance-title">{text.performance.title}</h2>
          <p>{text.performance.descriptionBeforeApi} <code>/api/ps</code> {text.performance.descriptionBeforeModel} <code>{model.runtimeModelId ?? model.modelId}</code>. {text.performance.descriptionAfterModel}</p>
        </div>
      </div>
      {performance.error && <div className="inline-error" role="alert">{performance.error}</div>}
      {performance.loading && !performance.snapshot ? (
        <div className="details-empty full-detail-empty">{text.performance.sampling}</div>
      ) : performance.snapshot ? (
        <>
          <PerformanceChart history={performance.history} />
          <div className="performance-status detail-performance-status">
            <span className={`led ${performance.snapshot.state === "running" ? "on" : "off"}`} />
            <strong>{performance.snapshot.state === "running" ? text.performance.loaded : performance.snapshot.state === "unavailable" ? text.performance.runtimeUnavailable : text.performance.stopped}</strong>
            <small>{text.performance.sampledAt(new Date(performance.snapshot.sampledAtUnixMs).toLocaleTimeString(text.locale))}</small>
          </div>
          <div className="performance-grid full-performance-grid">
            <div className="performance-card">
              <span>{text.performance.totalMemory}</span>
              <strong>{formatBytes(performance.snapshot.allocatedMemoryBytes)}</strong>
              <small>{text.performance.totalMemoryHint}</small>
            </div>
            <div className="performance-card">
              <span>{text.performance.systemMemory}</span>
              <strong>{formatBytes(performance.snapshot.allocatedSystemMemoryBytes)}</strong>
              <small>{text.performance.systemMemoryHint}</small>
            </div>
            <div className="performance-card">
              <span>{text.performance.vram}</span>
              <strong>{formatBytes(performance.snapshot.allocatedVramBytes)}</strong>
              <small>{text.performance.vramHint}</small>
            </div>
            <div className="performance-card">
              <span>{text.performance.contextCapacity}</span>
              <strong>{performance.snapshot.contextLength?.toLocaleString(text.locale) ?? text.common.unavailable}</strong>
              <small>{performance.snapshot.contextLength === undefined ? text.performance.contextUnavailable : text.performance.contextTokens}</small>
            </div>
          </div>
        </>
      ) : null}
    </section>
  );
}

type ChatState = ReturnType<typeof useModelChat>;

function ChatPanel({
  model,
  chat,
  transcriptRef,
  onKeyDown,
}: {
  model: RunningModelEntry;
  chat: ChatState;
  transcriptRef: React.RefObject<HTMLDivElement | null>;
  onKeyDown: (event: KeyboardEvent<HTMLTextAreaElement>) => void;
}) {
  return (
    <section className="detail-section chat-section" aria-labelledby="model-chat-title">
      <div className="detail-section-header">
        <div>
          <h2 id="model-chat-title">{text.chat.title(model.name)}</h2>
          <p>{text.chat.privacy}</p>
        </div>
        <div className="detail-section-actions">
          <button className="secondary-button" type="button" onClick={chat.clear} disabled={chat.messages.length === 0 || chat.busy}>{text.common.clear}</button>
        </div>
      </div>
      {chat.error && <div className="inline-error" role="alert">{chat.error}</div>}
      {chat.endpointError && <div className="inline-error" role="alert">{chat.endpointError}</div>}
      {!model.running && (
        <div className="api-notice warning"><strong>{text.chat.stoppedTitle}</strong><span>{text.chat.stoppedDescription}</span></div>
      )}
      {chat.endpointLoading ? (
        <div className="details-empty full-detail-empty">{text.chat.preparing}</div>
      ) : chat.endpoint && !chat.endpoint.apiAvailable ? (
        <div className="details-empty full-detail-empty">{text.chat.testRuntime}</div>
      ) : chat.endpoint && !chat.endpoint.chatAvailable ? (
        <div className="details-empty full-detail-empty">{chat.endpoint.embeddingsAvailable ? text.chat.embeddingsOnly : text.chat.completionsOnly}</div>
      ) : chat.endpoint ? (
        <>
          <div className="chat-transcript" ref={transcriptRef} role="log" aria-live="polite" aria-label={text.chat.conversationAria(model.name)}>
            {chat.messages.length === 0 ? (
              <div className="chat-empty">
                <strong>{text.chat.startPrivate}</strong>
                <span>{text.chat.privateDescription(
                  model.runtimeModelId ?? model.modelId,
                  model.location === "remote"
                    ? text.chat.remoteTransport(model.targetName ?? text.wizard.hardware.remoteTargetFallback)
                    : model.runtimeId === "vllm"
                      ? text.chat.vllmTransport
                      : text.chat.localTransport,
                )}</span>
              </div>
            ) : chat.messages.map((message, index) => (
              <article className={`chat-message ${message.role}`} key={`${message.role}-${index}`}>
                <span>{message.role === "user" ? text.chat.you : model.name}</span>
                {message.content ? <p>{message.content}</p> : <div className="chat-generating"><span className="model-control-spinner" /> {text.chat.generating}</div>}
              </article>
            ))}
          </div>
          <div className="chat-composer">
            <textarea
              rows={3}
              maxLength={65_536}
              value={chat.draft}
              onChange={(event) => chat.setDraft(event.target.value)}
              onKeyDown={onKeyDown}
              placeholder={model.running ? text.chat.messagePlaceholder : text.chat.stoppedPlaceholder}
              aria-label={text.chat.messageAria}
              disabled={!model.running || chat.busy}
            />
            <div>
              <small>{text.chat.keyboardHint}</small>
              {chat.busy ? (
                <button className="secondary-button" type="button" onClick={() => void chat.stop()} disabled={chat.cancelBusy}>{chat.cancelBusy ? text.chat.stopping : text.chat.stop}</button>
              ) : (
                <button className="primary-button" type="button" onClick={() => void chat.send()} disabled={!model.running || !chat.draft.trim()}>{text.chat.send}</button>
              )}
            </div>
          </div>
        </>
      ) : null}
    </section>
  );
}

function ApiPanel({
  model,
  chat,
  copiedField,
  onCopy,
}: {
  model: RunningModelEntry;
  chat: ChatState;
  copiedField?: string;
  onCopy: (value: string, key: string) => void;
}) {
  const endpoint = chat.endpoint;
  return (
    <section className="detail-section" aria-labelledby="model-api-title">
      <div className="detail-section-header">
        <div>
          <h2 id="model-api-title">{text.api.title}</h2>
          <p>{text.api.description(model.location === "remote" ? text.api.remoteEndpoint : model.runtimeId === "vllm" ? "configured vLLM endpoint" : text.api.localEndpoint)}</p>
        </div>
      </div>
      {chat.endpointError && <div className="inline-error" role="alert">{chat.endpointError}</div>}
      {chat.endpointLoading ? (
        <div className="details-empty full-detail-empty">{text.api.preparing}</div>
      ) : endpoint ? (
        <>
          {!endpoint.apiAvailable && (
            <div className="api-notice warning"><strong>{text.api.noApi}</strong><span>{text.api.noApiDescription}</span></div>
          )}
          <div className="endpoint-card api-endpoint-card">
            <EndpointValue label={text.wizard.ready.baseUrl} value={endpoint.baseUrl} copyKey={`api-base-${model.id}`} copiedField={copiedField} onCopy={onCopy} />
            <EndpointValue label={inferenceLabel(endpoint)} value={inferenceUrl(endpoint)} copyKey={`api-inference-${model.id}`} copiedField={copiedField} onCopy={onCopy} />
            <EndpointValue label={text.common.model} value={endpoint.model} copyKey={`api-model-${model.id}`} copiedField={copiedField} onCopy={onCopy} />
            <div>
              <span>{text.api.apiKey}</span>
              <div className="copy-value"><code>{endpoint.apiKeyRequired ? text.common.required : text.api.notRequiredLoopback}</code></div>
            </div>
          </div>
          {endpoint.apiAvailable && (
            <div className="api-example-card">
              <div>
                <h3>{text.api.example}</h3>
                <button className="secondary-button" type="button" aria-live="polite" onClick={() => onCopy(curlExample(endpoint), `api-example-${model.id}`)}>{copiedField === `api-example-${model.id}` ? text.common.copied : text.api.copyCurl}</button>
              </div>
              <pre tabIndex={0} aria-label={text.api.exampleAria}><code>{curlExample(endpoint)}</code></pre>
            </div>
          )}
          <div className="api-notice">
            <strong>{model.running ? text.api.preloaded : text.api.notPreloaded}</strong>
            <span>{model.running ? text.api.preloadedDescription : text.api.lazyLoadDescription} {model.location === "remote" ? text.api.remoteAvailability : text.api.localAvailability}</span>
          </div>
        </>
      ) : null}
    </section>
  );
}

function EndpointValue({
  label,
  value,
  copyKey,
  copiedField,
  onCopy,
}: {
  label: string;
  value: string;
  copyKey: string;
  copiedField?: string;
  onCopy: (value: string, key: string) => void;
}) {
  return (
    <div>
      <span>{label}</span>
      <div className="copy-value">
        <code>{value}</code>
        <button className={copiedField === copyKey ? "copied" : ""} type="button" aria-live="polite" onClick={() => onCopy(value, copyKey)}>
          {copiedField === copyKey ? text.common.copied : text.common.copy}
        </button>
      </div>
    </div>
  );
}
