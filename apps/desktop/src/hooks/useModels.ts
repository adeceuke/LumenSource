import { useEffect, useRef, useState, type Dispatch, type SetStateAction } from "react";
import {
  desktopCommands,
  isRemotePasswordRequiredMessage,
  messageFromError,
} from "../commands";
import { browserMessages } from "../i18n";
import { lifecycleLog } from "../modelUi";
import type { RunningModelEntry } from "../types";

const text = browserMessages();

interface ModelsController {
  models: RunningModelEntry[];
  setModels: Dispatch<SetStateAction<RunningModelEntry[]>>;
  action?: { id: string; action: "starting" | "stopping" };
  remotePasswordRequest?: RunningModelEntry;
  credentialBusy: boolean;
  toggleRunning: (id: string, password?: string) => Promise<void>;
  submitRemotePassword: (password: string, save: boolean) => Promise<void>;
  dismissRemotePassword: () => void;
  remove: (id: string) => Promise<void>;
  rename: (id: string, name: string) => void;
  clearLogs: (id: string) => void;
}

export function useModels(onError: (error: unknown) => void): ModelsController {
  const [models, setModels] = useState<RunningModelEntry[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [action, setAction] = useState<{ id: string; action: "starting" | "stopping" }>();
  const [remotePasswordRequest, setRemotePasswordRequest] = useState<RunningModelEntry>();
  const [credentialBusy, setCredentialBusy] = useState(false);
  const pendingSave = useRef<RunningModelEntry[] | null>(null);
  const saveInFlight = useRef(false);

  useEffect(() => {
    void desktopCommands.loadModels()
      .then(setModels)
      .catch(onError)
      .finally(() => setLoaded(true));
  }, [onError]);

  useEffect(() => {
    if (!loaded) return;
    pendingSave.current = models;
    if (saveInFlight.current) return;

    saveInFlight.current = true;
    void (async () => {
      try {
        while (pendingSave.current) {
          const nextModels = pendingSave.current;
          pendingSave.current = null;
          await desktopCommands.saveModels(nextModels);
        }
      } catch (error) {
        onError(error);
      } finally {
        saveInFlight.current = false;
      }
    })();
  }, [loaded, models, onError]);

  const toggleRunning = async (id: string, password?: string) => {
    const selected = models.find((model) => model.id === id);
    if (!selected || action || !selected.runtimeCapabilities.modelStartStop) return;
    setAction({ id, action: selected.running ? "stopping" : "starting" });
    try {
      if (selected.running) {
        await desktopCommands.stop(selected.modelId, selected.targetId, password, selected.id);
      } else {
        await desktopCommands.start(selected.modelId, selected.targetId, password, selected.id);
      }
      const running = !selected.running;
      const log = lifecycleLog(running ? text.lifecycle.started : text.lifecycle.stopped);
      setModels((current) => current.map((model) => (
        model.id === id || (
          selected.runtimeModelId !== undefined
          && model.targetId === selected.targetId
          && model.runtimeModelId === selected.runtimeModelId
        )
          ? { ...model, running, logs: [...model.logs, log] }
          : model
      )));
      setRemotePasswordRequest(undefined);
    } catch (error) {
      const message = messageFromError(error, text.errors.unexpected);
      if (selected.location === "remote" && isRemotePasswordRequiredMessage(message)) {
        setRemotePasswordRequest(selected);
      } else {
        onError(error);
      }
    } finally {
      setAction(undefined);
    }
  };

  const submitRemotePassword = async (password: string, save: boolean) => {
    if (!remotePasswordRequest || credentialBusy) return;
    setCredentialBusy(true);
    try {
      if (save) {
        await desktopCommands.saveRemotePassword(remotePasswordRequest.targetId, password);
      } else {
        await desktopCommands.deleteRemotePassword(remotePasswordRequest.targetId);
      }
      await toggleRunning(remotePasswordRequest.id, password);
    } catch (error) {
      onError(error);
    } finally {
      setCredentialBusy(false);
    }
  };

  const remove = async (id: string) => {
    const remainingModels = await desktopCommands.removeModel(id);
    setModels(remainingModels);
  };

  const rename = (id: string, requestedName: string) => {
    setModels((current) => current.map((model) => {
      if (model.id !== id) return model;
      const name = requestedName.trim() || model.name;
      return name === model.name
        ? model
        : { ...model, name, logs: [...model.logs, lifecycleLog(text.lifecycle.renamed(model.name, name))] };
    }));
  };

  const clearLogs = (id: string) => {
    setModels((current) => current.map((model) => (
      model.id === id ? { ...model, logs: [] } : model
    )));
  };

  return {
    models,
    setModels,
    action,
    remotePasswordRequest,
    credentialBusy,
    toggleRunning,
    submitRemotePassword,
    dismissRemotePassword: () => setRemotePasswordRequest(undefined),
    remove,
    rename,
    clearLogs,
  };
}
