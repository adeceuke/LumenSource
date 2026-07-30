import { createPortal } from "react-dom";
import { browserMessages } from "../i18n";
import type { RunningModelEntry } from "../types";

const text = browserMessages();

interface ModelsListProps {
  models: RunningModelEntry[];
  modelAction?: { id: string; action: "starting" | "stopping" };
  menuOpenId?: string;
  menuPosition: { top: number; left: number } | null;
  onAdd: () => void;
  onConnectVllm: () => void;
  onOpen: (model: RunningModelEntry) => void;
  onToggleRunning: (id: string) => void;
  onToggleMenu: (id: string, trigger: HTMLButtonElement) => void;
  onRename: (model: RunningModelEntry) => void;
  onPerformance: (model: RunningModelEntry) => void;
  onApi: (model: RunningModelEntry) => void;
  onSettings: (model: RunningModelEntry) => void;
  onRemove: (id: string) => void;
}

export function ModelsList({
  models,
  modelAction,
  menuOpenId,
  menuPosition,
  onAdd,
  onConnectVllm,
  onOpen,
  onToggleRunning,
  onToggleMenu,
  onRename,
  onPerformance,
  onApi,
  onSettings,
  onRemove,
}: ModelsListProps) {
  return (
    <>
      <div className="models-toolbar">
        <div>
          <span className="eyebrow">{text.common.models}</span>
          <h1>{text.common.models}</h1>
        </div>
        <div className="models-toolbar-actions">
          <button className="secondary-button" type="button" onClick={onConnectVllm}>
            {text.vllm.connectAction}
          </button>
          <button className="primary-button" type="button" onClick={onAdd}>
            {text.common.addModel}
          </button>
        </div>
      </div>

      <div className="models-table" role="table" aria-label={text.models.tableLabel}>
        {models.length > 0 ? models.map((model) => (
          <div className="models-table-row" role="row" key={model.id}>
            <button className="model-row-button" type="button" onClick={() => onOpen(model)} aria-label={text.models.openDetails(model.name)}>
              <div className="model-main">
                <span className={`led ${model.running ? "on" : "off"}`} aria-label={model.running ? text.details.running : text.details.stopped} />
                <div>
                  <strong role="cell">{model.name}</strong>
                  <p>{model.modelName} · {model.runtimeId === "vllm" ? "vLLM" : model.runtimeId === "ollama" ? "Ollama" : "Dummy"} · {model.version} · {model.location === "remote" ? model.targetName ?? text.models.remoteLinux : text.models.thisMachine}{model.runtimeId === "vllm" ? text.models.externalSuffix : model.managed === false ? text.models.discoveredSuffix : ""}</p>
                  {modelAction?.id === model.id && (
                    <span className="model-transition"><i className="model-control-spinner" /> {modelAction.action === "starting" ? text.models.starting : text.models.stopping}</span>
                  )}
                </div>
              </div>
            </button>
            <div className="model-actions">
              <button
                className="icon-button"
                type="button"
                onClick={() => onToggleRunning(model.id)}
                disabled={!model.runtimeCapabilities.modelStartStop || modelAction !== undefined}
                title={!model.runtimeCapabilities.modelStartStop ? model.runtimeCapabilities.lifecycle === "external" ? text.models.externalLifecycleTitle : text.models.unmanagedTitle : model.running ? text.details.stop : text.details.start}
                aria-label={modelAction?.id === model.id ? modelAction.action === "starting" ? text.models.startingAria : text.models.stoppingAria : model.running ? text.details.stop : text.details.start}
              >
                {modelAction?.id === model.id
                  ? <span className="model-control-spinner" />
                  : <span className={`model-control-icon ${model.running ? "stop" : "start"}`} />}
              </button>
              <div className="menu-wrap" data-model-menu>
                <button
                  className="icon-button"
                  type="button"
                  onClick={(event) => onToggleMenu(model.id, event.currentTarget)}
                  aria-haspopup="menu"
                  aria-expanded={menuOpenId === model.id}
                >
                  ⋯
                </button>
                {menuOpenId === model.id && menuPosition && createPortal(
                  <div className="menu-popover" data-model-menu role="menu" style={{ top: `${menuPosition.top}px`, left: `${menuPosition.left}px` }}>
                    <button type="button" role="menuitem" onClick={() => onRename(model)}>{text.common.rename}</button>
                    <button type="button" role="menuitem" onClick={() => onOpen(model)}>{text.common.logs}</button>
                    <button type="button" role="menuitem" onClick={() => onPerformance(model)}>{text.common.performance}</button>
                    <button type="button" role="menuitem" onClick={() => onApi(model)}>{text.common.api}</button>
                    {model.runtimeCapabilities.perModelConfiguration && (
                      <button type="button" role="menuitem" onClick={() => onSettings(model)}>{text.navigation.settings}</button>
                    )}
                    <button type="button" role="menuitem" onClick={() => onRemove(model.id)}>{text.common.remove}</button>
                  </div>,
                  document.body,
                )}
              </div>
            </div>
          </div>
        )) : (
          <div className="empty-state" role="row">
            <strong>{text.models.emptyTitle}</strong>
            <p>{text.models.emptyDescription}</p>
          </div>
        )}
      </div>
    </>
  );
}
