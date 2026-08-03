import { useEffect, useState } from "react";
import { desktopCommands, isChatCancellationMessage } from "../commands";
import { browserMessages } from "../i18n";
import type { ChatMessage, EndpointDetails, RunningModelEntry } from "../types";

const text = browserMessages();

export function useModelChat(
  model: RunningModelEntry | undefined,
  endpointActive: boolean,
  errorFrom: (error: unknown) => string,
) {
  const [sessions, setSessions] = useState<Record<string, ChatMessage[]>>({});
  const [draft, setDraft] = useState("");
  const [busyModelId, setBusyModelId] = useState<string>();
  const [busyRequestId, setBusyRequestId] = useState<string>();
  const [cancelBusy, setCancelBusy] = useState(false);
  const [error, setError] = useState<string>();
  const [endpoint, setEndpoint] = useState<EndpointDetails>();
  const [endpointLoading, setEndpointLoading] = useState(false);
  const [endpointError, setEndpointError] = useState<string>();
  const messages = model ? sessions[model.id] ?? [] : [];

  useEffect(() => {
    if (!endpointActive || !model) return;
    if (!model.runtimeModelId) {
      setEndpoint(undefined);
      setEndpointError(text.errors.missingRuntimeModelId);
      setEndpointLoading(false);
      return;
    }

    let disposed = false;
    setEndpoint(undefined);
    setEndpointError(undefined);
    setEndpointLoading(true);
    void desktopCommands.modelEndpoint(model.id, model.modelId, model.runtimeModelId, model.targetId)
      .then((details) => {
        if (!disposed) setEndpoint(details);
      })
      .catch((endpointFailure: unknown) => {
        if (!disposed) setEndpointError(errorFrom(endpointFailure));
      })
      .finally(() => {
        if (!disposed) setEndpointLoading(false);
      });
    return () => {
      disposed = true;
    };
  }, [endpointActive, errorFrom, model?.id, model?.modelId, model?.runtimeModelId, model?.targetId]);

  useEffect(() => {
    setDraft("");
    setError(undefined);
  }, [model?.id]);

  const clear = () => {
    if (!model || busyModelId === model.id) return;
    setSessions((current) => ({ ...current, [model.id]: [] }));
    setError(undefined);
  };

  const send = async () => {
    const content = draft.trim();
    if (!model || !content || busyModelId) return;
    if (!model.running) {
      setError(text.errors.startBeforeChat);
      return;
    }
    if (!model.runtimeCapabilities.chat || !model.runtimeModelId || !endpoint?.apiAvailable || !endpoint.chatAvailable) {
      setError(text.errors.chatUnavailable);
      return;
    }

    const userMessage: ChatMessage = { role: "user", content };
    const requestMessages = [...(sessions[model.id] ?? []), userMessage];
    setSessions((current) => ({
      ...current,
      [model.id]: [...requestMessages, { role: "assistant", content: "" }],
    }));
    setDraft("");
    setError(undefined);
    setBusyModelId(model.id);
    const requestId = crypto.randomUUID();
    setBusyRequestId(requestId);
    let streamFinalized = false;

    try {
      const completion = await desktopCommands.chat(
        requestId,
        model.modelId,
        model.id,
        model.runtimeModelId,
        model.targetId,
        requestMessages,
        {},
        (event) => {
          if (event.requestId !== requestId || streamFinalized) return;
          if (event.event === "status") return;
          setSessions((current) => {
            const nextMessages = [...(current[model.id] ?? [])];
            const last = nextMessages.at(-1);
            if (!last || last.role !== "assistant") return current;
            if (event.event === "delta") {
              nextMessages[nextMessages.length - 1] = { ...last, content: last.content + event.content };
            } else if (!last.content) {
              nextMessages.pop();
            }
            return { ...current, [model.id]: nextMessages };
          });
        },
      );
      streamFinalized = true;
      if (completion.requestId !== requestId) throw new Error("The runtime returned a mismatched chat completion.");
      setSessions((current) => {
        const nextMessages = [...(current[model.id] ?? [])];
        const last = nextMessages.at(-1);
        if (!last || last.role !== "assistant") return current;
        if (completion.content) {
          nextMessages[nextMessages.length - 1] = { ...last, content: completion.content };
        } else {
          nextMessages.pop();
        }
        return { ...current, [model.id]: nextMessages };
      });
    } catch (chatFailure) {
      streamFinalized = true;
      const message = errorFrom(chatFailure);
      setSessions((current) => {
        const nextMessages = [...(current[model.id] ?? [])];
        if (nextMessages.at(-1)?.role === "assistant" && !nextMessages.at(-1)?.content) nextMessages.pop();
        return { ...current, [model.id]: nextMessages };
      });
      if (!isChatCancellationMessage(message)) setError(message);
    } finally {
      streamFinalized = true;
      setBusyModelId((current) => current === model.id ? undefined : current);
      setBusyRequestId((current) => current === requestId ? undefined : current);
      setCancelBusy(false);
    }
  };

  const stop = async () => {
    if (!busyModelId || !busyRequestId || cancelBusy) return;
    setCancelBusy(true);
    try {
      await desktopCommands.cancelChat(busyRequestId);
    } catch (cancelError) {
      setError(errorFrom(cancelError));
      setCancelBusy(false);
    }
  };

  return {
    messages,
    draft,
    setDraft,
    busy: busyModelId === model?.id,
    cancelBusy,
    error,
    endpoint,
    endpointLoading,
    endpointError,
    clear,
    send,
    stop,
  };
}
