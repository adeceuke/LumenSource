import { useEffect, useMemo, useRef, useState, type ChangeEvent, type KeyboardEvent, type ReactNode } from "react";
import { desktopCommands } from "../commands";
import { useModalFocus } from "../hooks/useModalFocus";
import type {
  ApplicationSettings,
  ChatMessage,
  Conversation,
  ConversationMessage,
  RunningModelEntry,
} from "../types";

const MAX_RENDERED_MESSAGES = 120;
const MAX_REQUEST_MESSAGES = 80;
const MAX_SYSTEM_PROMPT_BYTES = 256 * 1024;
const LAST_SYSTEM_PROMPT_KEY = "lumen-source:last-system-prompt";

interface SystemPromptSelection {
  content: string;
  name?: string;
}

interface LumenChatWorkspaceProps {
  models: RunningModelEntry[];
  settings?: ApplicationSettings;
  initialModelId?: string;
  copiedField?: string;
  errorFrom: (error: unknown) => string;
  onCopy: (value: string, key: string) => void;
  onModelLog: (modelId: string, message: string) => void;
  onSettingsChanged?: (settings: ApplicationSettings) => void;
  onStartModel: (modelId: string) => void;
}

function now(): string {
  return new Date().toISOString();
}

function newConversation(model?: RunningModelEntry, systemPrompt: SystemPromptSelection = { content: "" }): Conversation {
  const timestamp = now();
  return {
    schemaVersion: 1,
    id: crypto.randomUUID(),
    title: "New conversation",
    modelEntryId: model?.id,
    modelNameSnapshot: model?.name,
    systemPrompt: systemPrompt.content,
    systemPromptName: systemPrompt.name,
    saveHistory: true,
    createdAt: timestamp,
    updatedAt: timestamp,
    parameters: {},
    messages: [],
  };
}

export function LumenChatWorkspace({
  models,
  settings,
  initialModelId,
  copiedField,
  errorFrom,
  onCopy,
  onModelLog,
  onSettingsChanged,
  onStartModel,
}: LumenChatWorkspaceProps) {
  const compatibleModels = useMemo(
    () => models.filter((model) => model.inventoryStatus === "available" && model.runtimeCapabilities.chat),
    [models],
  );
  const preferredModel = compatibleModels.find(({ id }) => id === initialModelId)
    ?? compatibleModels.find(({ id }) => id === settings?.lastUsedModelEntryId)
    ?? compatibleModels.find(({ id }) => id === settings?.defaultModelEntryId)
    ?? compatibleModels[0];
  const requestedModel = initialModelId ? models.find(({ id }) => id === initialModelId) : undefined;
  const requestedModelUnsupported = Boolean(requestedModel && !compatibleModels.some(({ id }) => id === requestedModel.id));
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [selectedId, setSelectedId] = useState<string>();
  const [draft, setDraft] = useState("");
  const [search, setSearch] = useState("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string>();
  const [activeRequest, setActiveRequest] = useState<{ requestId: string; conversationId: string }>();
  const [cancelBusy, setCancelBusy] = useState(false);
  const [deletePending, setDeletePending] = useState(false);
  const [generationPhase, setGenerationPhase] = useState<string>();
  const [historyNotice, setHistoryNotice] = useState<string>();
  const [lastSystemPrompt, setLastSystemPrompt] = useState<SystemPromptSelection | undefined>(loadLastSystemPrompt);
  const transcriptRef = useRef<HTMLDivElement>(null);
  const systemPromptInputRef = useRef<HTMLInputElement>(null);
  const deleteDialogRef = useModalFocus<HTMLDivElement>(
    deletePending ? () => setDeletePending(false) : undefined,
    deletePending,
  );

  const current = conversations.find(({ id }) => id === selectedId);
  const selectedModel = compatibleModels.find(({ id }) => id === current?.modelEntryId);
  const persistedModelMissing = Boolean(current?.modelEntryId && !selectedModel);
  const currentGenerating = Boolean(current?.messages.some(({ status }) => status === "generating"));
  const conversationBusy = Boolean(activeRequest) || currentGenerating;

  useEffect(() => {
    let disposed = false;
    void desktopCommands.listConversations()
      .then((loaded) => {
        if (disposed) return;
        const inheritedPrompt = lastSystemPrompt ?? promptFromConversation(
          loaded.find(({ systemPrompt }) => Boolean(systemPrompt)),
        );
        if (!lastSystemPrompt && inheritedPrompt.content) {
          setLastSystemPrompt(inheritedPrompt);
          storeLastSystemPrompt(inheritedPrompt);
        }
        const matching = initialModelId
          ? loaded.find(({ modelEntryId }) => modelEntryId === initialModelId)
          : undefined;
        if (loaded.length > 0) {
          if (initialModelId && !matching) {
            const conversation = newConversation(preferredModel, inheritedPrompt);
            setConversations([conversation, ...loaded]);
            setSelectedId(conversation.id);
          } else {
            setConversations(loaded);
            setSelectedId(matching?.id ?? loaded[0].id);
          }
        } else {
          const conversation = newConversation(preferredModel, inheritedPrompt);
          setConversations([conversation]);
          setSelectedId(conversation.id);
        }
      })
      .catch((failure: unknown) => setError(errorFrom(failure)))
      .finally(() => {
        if (!disposed) setLoading(false);
      });
    return () => {
      disposed = true;
    };
  }, [errorFrom, initialModelId]);

  useEffect(() => {
    const frame = window.requestAnimationFrame(() => {
      transcriptRef.current?.scrollTo({ top: transcriptRef.current.scrollHeight });
    });
    return () => window.cancelAnimationFrame(frame);
  }, [current?.messages]);

  useEffect(() => {
    if (loading || conversationBusy || !current) return;
    const timeout = window.setTimeout(() => {
      void desktopCommands.saveConversation(current).catch((failure: unknown) => setError(errorFrom(failure)));
    }, 500);
    return () => window.clearTimeout(timeout);
  }, [conversationBusy, current, errorFrom, loading]);

  useEffect(() => {
    if (activeRequest || !current || !currentGenerating || !current.saveHistory) return;
    let disposed = false;
    let timer: number | undefined;
    let inactiveChecks = 0;
    const conversationId = current.id;
    const storedUpdatedAt = current.updatedAt;
    const synchronize = async () => {
      let finished = false;
      try {
        const stored = (await desktopCommands.listConversations())
          .find(({ id }) => id === conversationId);
        if (disposed || !stored) return;
        const pending = stored.messages.find(({ status }) => status === "generating");
        finished = !pending;
        const storageChanged = stored.updatedAt !== storedUpdatedAt
          || stored.messages.length !== current.messages.length
          || stored.messages.some((message, index) => (
            message.status !== current.messages[index]?.status
            || message.content !== current.messages[index]?.content
          ));
        if (storageChanged) {
          setConversations((existing) => [
            stored,
            ...existing.filter(({ id }) => id !== stored.id),
          ]);
        }
        if (pending && !storageChanged) {
          const requestActive = pending.requestId
            ? await desktopCommands.chatRequestActive(pending.requestId)
            : false;
          inactiveChecks = requestActive ? 0 : inactiveChecks + 1;
          if (inactiveChecks >= 2) {
            const stopped = {
              ...stored,
              messages: stored.messages.map((message) => message.id === pending.id
                ? { ...message, requestId: undefined, status: "stopped" as const }
                : message),
              updatedAt: now(),
            };
            setConversations((existing) => [
              stopped,
              ...existing.filter(({ id }) => id !== stopped.id),
            ]);
            await desktopCommands.saveConversation(stopped);
            finished = true;
          }
        }
      } catch (failure) {
        finished = true;
        if (!disposed) setError(errorFrom(failure));
      } finally {
        if (!disposed && !finished) timer = window.setTimeout(() => void synchronize(), 1_000);
      }
    };
    void synchronize();
    return () => {
      disposed = true;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [activeRequest, current?.id, current?.saveHistory, current?.updatedAt, currentGenerating]);

  useEffect(() => {
    if (!current || current.modelEntryId || !preferredModel || conversationBusy) return;
    setConversations((existing) => existing.map((conversation) => conversation.id === current.id
      ? { ...conversation, modelEntryId: preferredModel.id, modelNameSnapshot: preferredModel.name, updatedAt: now() }
      : conversation));
  }, [conversationBusy, current, preferredModel]);

  useEffect(() => {
    if (!settings) return;
    const defaultModelEntryId = compatibleModels.some(({ id }) => id === settings.defaultModelEntryId)
      ? settings.defaultModelEntryId
      : undefined;
    const lastUsedModelEntryId = compatibleModels.some(({ id }) => id === settings.lastUsedModelEntryId)
      ? settings.lastUsedModelEntryId
      : undefined;
    if (defaultModelEntryId === settings.defaultModelEntryId && lastUsedModelEntryId === settings.lastUsedModelEntryId) return;
    void desktopCommands
      .setChatModelPreferences(defaultModelEntryId, lastUsedModelEntryId)
      .then(onSettingsChanged)
      .catch((failure: unknown) => setError(errorFrom(failure)));
  }, [compatibleModels, errorFrom, onSettingsChanged, settings]);

  const replaceConversation = (conversation: Conversation) => {
    setConversations((existing) => [
      conversation,
      ...existing.filter(({ id }) => id !== conversation.id),
    ]);
  };

  const persist = async (conversation: Conversation) => {
    try {
      await desktopCommands.saveConversation(conversation);
    } catch (failure) {
      setError(errorFrom(failure));
    }
  };

  const createConversation = () => {
    const conversation = newConversation(preferredModel, lastSystemPrompt);
    setConversations((existing) => [conversation, ...existing]);
    setSelectedId(conversation.id);
    setDraft("");
    setError(undefined);
    setHistoryNotice(undefined);
    if (conversation.saveHistory) void persist(conversation);
  };

  const renameConversation = () => {
    if (!current || conversationBusy) return;
    const requested = window.prompt("Conversation name", current.title)?.trim();
    if (!requested || requested === current.title) return;
    const updated = { ...current, title: requested.slice(0, 160), updatedAt: now() };
    replaceConversation(updated);
    void persist(updated);
  };

  const deleteConversation = async () => {
    if (!current || conversationBusy) return;
    setDeletePending(false);
    try {
      await desktopCommands.deleteConversation(current.id);
      const remaining = conversations.filter(({ id }) => id !== current.id);
      if (remaining.length > 0) {
        setConversations(remaining);
        setSelectedId(remaining[0].id);
      } else {
        const replacement = newConversation(preferredModel, lastSystemPrompt);
        setConversations([replacement]);
        setSelectedId(replacement.id);
      }
    } catch (failure) {
      setError(errorFrom(failure));
    }
  };

  const clearConversation = () => {
    if (!current || conversationBusy || current.messages.length === 0 || !window.confirm("Clear all messages in this conversation?")) return;
    const updated = { ...current, messages: [], updatedAt: now() };
    replaceConversation(updated);
    void persist(updated);
  };

  const updateConversation = (changes: Partial<Conversation>) => {
    if (!current || conversationBusy) return;
    const updated = { ...current, ...changes, updatedAt: now() };
    replaceConversation(updated);
  };

  const loadSystemPrompt = async (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file || !current || conversationBusy) return;
    if (file.size > MAX_SYSTEM_PROMPT_BYTES) {
      setError("Choose a system prompt file smaller than 256 KiB.");
      return;
    }
    try {
      const content = await file.text();
      if (!content.trim()) {
        setError("The selected system prompt file is empty.");
        return;
      }
      const selection = { content, name: file.name.slice(0, 255) };
      setLastSystemPrompt(selection);
      storeLastSystemPrompt(selection);
      updateConversation({ systemPrompt: selection.content, systemPromptName: selection.name });
      setError(undefined);
    } catch (failure) {
      setError(errorFrom(failure));
    }
  };

  const clearSystemPrompt = () => {
    if (!current || conversationBusy) return;
    const selection = { content: "" };
    setLastSystemPrompt(selection);
    storeLastSystemPrompt(selection);
    updateConversation({ systemPrompt: "", systemPromptName: undefined });
    setError(undefined);
  };

  const selectModel = (modelEntryId: string) => {
    const model = compatibleModels.find(({ id }) => id === modelEntryId);
    if (!current || !model || conversationBusy) return;
    const updated = {
      ...current,
      modelEntryId: model.id,
      modelNameSnapshot: model.name,
      updatedAt: now(),
    };
    replaceConversation(updated);
    void persist(updated);
    if (settings) {
      void desktopCommands
        .setChatModelPreferences(settings.defaultModelEntryId, model.id)
        .then(onSettingsChanged)
        .catch((failure: unknown) => setError(errorFrom(failure)));
    }
  };

  const setAsDefault = () => {
    if (!selectedModel) return;
    void desktopCommands
      .setChatModelPreferences(selectedModel.id, selectedModel.id)
      .then(onSettingsChanged)
      .catch((failure: unknown) => setError(errorFrom(failure)));
  };

  const setSaveHistory = (saveHistory: boolean) => {
    if (!current || conversationBusy) return;
    const updated = { ...current, saveHistory, updatedAt: now() };
    replaceConversation(updated);
    if (saveHistory) {
      void persist(updated);
    } else {
      void desktopCommands.deleteConversation(updated.id).catch((failure: unknown) => setError(errorFrom(failure)));
    }
  };

  const generate = async (base: Conversation, userContent?: string) => {
    const model = compatibleModels.find(({ id }) => id === base.modelEntryId);
    if (!model?.runtimeModelId) {
      setError("Choose an available chat-capable model before sending.");
      return;
    }
    if (!model.running) {
      try {
        const plan = model.targetId === "local" ? await desktopCommands.resourceStartPlan(model.id) : undefined;
        const affected = plan?.consumers.map((consumer) => `${consumer.name}${consumer.pinned ? " (pinned)" : ""}`).join(", ");
        setHistoryNotice(plan
          ? `${plan.waitingReason ?? `${model.name} is stopped.`}${affected ? ` Affected models: ${affected}.` : ""} Start ${model.name} to continue.`
          : `Start ${model.name} before sending this message.`);
      } catch {
        setHistoryNotice(`Start ${model.name} before sending this message.`);
      }
      return;
    }

    const requestId = crypto.randomUUID();
    const requestLabel = requestId.slice(0, 8);
    const timestamp = now();
    const userMessage: ConversationMessage | undefined = userContent === undefined ? undefined : {
      id: crypto.randomUUID(),
      role: "user",
      content: userContent,
      createdAt: timestamp,
      status: "complete",
    };
    const assistantId = crypto.randomUUID();
    const assistantMessage: ConversationMessage = {
      id: assistantId,
      requestId,
      role: "assistant",
      content: "",
      createdAt: timestamp,
      status: "generating",
    };
    const requestConversation = {
      ...base,
      title: base.messages.length === 0 && userContent
        ? userContent.replace(/\s+/g, " ").slice(0, 64)
        : base.title,
      messages: [...base.messages, ...(userMessage ? [userMessage] : []), assistantMessage],
      updatedAt: timestamp,
    };
    replaceConversation(requestConversation);
    setActiveRequest({ requestId, conversationId: base.id });
    setError(undefined);
    setHistoryNotice(undefined);
    let generated = "";

    if (requestConversation.saveHistory) {
      await persist(requestConversation);
    }

    const requestMessages = requestConversation.messages
      .filter((message) => message.id !== assistantId && (message.role === "user" || message.role === "assistant") && message.content.trim())
      .slice(-MAX_REQUEST_MESSAGES)
      .map<ChatMessage>((message) => ({ role: message.role as "user" | "assistant", content: message.content }));
    if (requestConversation.messages.length - 1 > MAX_REQUEST_MESSAGES) {
      setHistoryNotice(`Only the most recent ${MAX_REQUEST_MESSAGES} messages were included to keep the request bounded.`);
    }
    const startedAt = performance.now();
    setGenerationPhase("Waiting for model response…");
    onModelLog(
      model.id,
      `Lumen Chat [${requestLabel}] started · runtime=${model.runtimeId} · target=${model.targetId === "local" ? "local" : "remote"} · messages=${requestMessages.length} · streaming=true.`,
    );
    let receivedRuntimeEvent = false;
    let streamFinalized = false;
    const firstEventTimer = window.setTimeout(() => {
      if (receivedRuntimeEvent) return;
      setGenerationPhase("Still waiting for the runtime…");
      onModelLog(model.id, `Lumen Chat [${requestLabel}] no runtime stream event received after 15 s; request remains pending.`);
    }, 15_000);

    try {
      const completion = await desktopCommands.chat(
        requestId,
        model.modelId,
        model.id,
        model.runtimeModelId,
        model.targetId,
        requestMessages,
        {
          systemPrompt: base.systemPrompt || undefined,
          temperature: base.parameters.temperature,
          maxOutputTokens: base.parameters.maxOutputTokens,
        },
        (event) => {
          if (event.requestId !== requestId || streamFinalized) return;
          receivedRuntimeEvent = true;
          window.clearTimeout(firstEventTimer);
          if (event.event === "status") {
            if (event.status === "reasoning") {
              setGenerationPhase("Model is reasoning…");
              onModelLog(model.id, `Lumen Chat [${requestLabel}] reasoning began after ${formatElapsed(event.elapsedMs)}.`);
            } else {
              setGenerationPhase("Receiving response…");
              onModelLog(model.id, `Lumen Chat [${requestLabel}] first response text arrived after ${formatElapsed(event.elapsedMs)}.`);
            }
            return;
          }
          if (event.event !== "delta") return;
          generated += event.content;
          setConversations((existing) => existing.map((conversation) => (
            conversation.id === base.id
              ? {
                  ...conversation,
                  messages: conversation.messages.map((message) => message.id === assistantId
                    ? { ...message, content: generated }
                    : message),
                }
              : conversation
          )));
        },
      );
      streamFinalized = true;
      if (completion.requestId !== requestId) {
        throw new Error("The runtime returned a mismatched chat completion.");
      }
      if (completion.content !== generated) {
        onModelLog(
          model.id,
          `Lumen Chat [${requestLabel}] reconciled the final response · streamedCharacters=${generated.length} · finalCharacters=${completion.content.length}.`,
        );
      }
      generated = completion.content;
      const completed = {
        ...requestConversation,
        messages: requestConversation.messages.map((message) => message.id === assistantId
          ? { ...message, requestId: undefined, content: generated, status: "complete" as const }
          : message),
        updatedAt: now(),
      };
      replaceConversation(completed);
      await persist(completed);
      onModelLog(
        model.id,
        `Lumen Chat [${requestLabel}] completed in ${formatElapsed(performance.now() - startedAt)} · outputCharacters=${generated.length}.`,
      );
    } catch (failure) {
      streamFinalized = true;
      const message = errorFrom(failure);
      const cancelled = message.toLowerCase().includes("cancel");
      const failed = {
        ...requestConversation,
        messages: requestConversation.messages.map((item) => item.id === assistantId
          ? { ...item, requestId: undefined, content: generated, status: cancelled ? "stopped" as const : "failed" as const }
          : item),
        updatedAt: now(),
      };
      replaceConversation(failed);
      await persist(failed);
      if (cancelled) {
        onModelLog(model.id, `Lumen Chat [${requestLabel}] cancelled after ${formatElapsed(performance.now() - startedAt)}.`);
      } else {
        onModelLog(
          model.id,
          `Lumen Chat [${requestLabel}] failed after ${formatElapsed(performance.now() - startedAt)} · ${diagnosticFailure(message)}.`,
        );
        setError(message);
      }
    } finally {
      streamFinalized = true;
      window.clearTimeout(firstEventTimer);
      setActiveRequest((active) => active?.requestId === requestId ? undefined : active);
      setCancelBusy(false);
      setGenerationPhase(undefined);
    }
  };

  const send = () => {
    const content = draft.trim();
    if (!current || !content || conversationBusy) return;
    if (!selectedModel?.running) {
      void generate(current, content);
      return;
    }
    setDraft("");
    void generate(current, content);
  };

  const regenerate = () => {
    if (!current || conversationBusy) return;
    const assistantIndex = current.messages.map(({ role }) => role).lastIndexOf("assistant");
    if (assistantIndex < 0) return;
    const base = { ...current, messages: current.messages.slice(0, assistantIndex), updatedAt: now() };
    replaceConversation(base);
    void generate(base);
  };

  const editAndResend = (messageId: string) => {
    if (!current || conversationBusy) return;
    const index = current.messages.findIndex(({ id }) => id === messageId);
    const message = current.messages[index];
    if (index < 0 || message.role !== "user") return;
    const content = window.prompt("Edit message", message.content)?.trim();
    if (!content) return;
    const base = {
      ...current,
      messages: current.messages.slice(0, index + 1).map((item, itemIndex) => itemIndex === index ? { ...item, content } : item),
      updatedAt: now(),
    };
    replaceConversation(base);
    void generate(base);
  };

  const stop = async () => {
    if (!activeRequest || cancelBusy) return;
    setCancelBusy(true);
    try {
      await desktopCommands.cancelChat(activeRequest.requestId);
    } catch (failure) {
      setError(errorFrom(failure));
      setCancelBusy(false);
    }
  };

  const handleComposerKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key !== "Enter" || event.shiftKey) return;
    event.preventDefault();
    send();
  };

  const exportConversation = (format: "markdown" | "json") => {
    if (!current) return;
    const body = format === "json"
      ? JSON.stringify(current, null, 2)
      : [`# ${current.title}`, current.systemPrompt ? `> System prompt: ${current.systemPrompt}` : "", ...current.messages.map((message) => `## ${message.role === "user" ? "You" : message.role}\n\n${message.content}`)].filter(Boolean).join("\n\n");
    downloadText(`${safeFilename(current.title)}.${format === "json" ? "json" : "md"}`, body, format === "json" ? "application/json" : "text/markdown");
  };

  if (loading) return <div className="details-empty full-detail-empty">Loading private conversations…</div>;
  if (!current) return <div className="details-empty full-detail-empty">No conversation is available.</div>;

  const filteredConversations = conversations.filter((conversation) => {
    const query = search.trim().toLowerCase();
    return !query || conversation.title.toLowerCase().includes(query)
      || conversation.messages.some(({ content }) => content.toLowerCase().includes(query));
  });
  const renderedMessages = current.messages.slice(-MAX_RENDERED_MESSAGES);
  const estimatedTokens = Math.ceil((current.systemPrompt.length + current.messages.reduce((total, message) => total + message.content.length, 0)) / 4);
  const contextLimit = selectedModel?.modelSettings?.contextLength;
  const lastAssistant = [...current.messages].reverse().find(({ role }) => role === "assistant");

  return (
    <section className="lumen-chat-workspace" aria-label="Lumen Chat workspace">
      <aside className="conversation-sidebar">
        <div className="conversation-sidebar-heading">
          <div><span className="eyebrow">Private workspace</span><h2>Lumen Chat</h2></div>
          <button className="primary-button" type="button" onClick={createConversation} disabled={conversationBusy}>New</button>
        </div>
        <input value={search} onChange={(event) => setSearch(event.target.value)} placeholder="Search conversations" aria-label="Search conversations" />
        <div className="conversation-list" role="listbox" aria-label="Conversations">
          {filteredConversations.map((conversation) => (
            <button key={conversation.id} type="button" role="option" aria-selected={conversation.id === current.id} className={conversation.id === current.id ? "active" : ""} onClick={() => setSelectedId(conversation.id)} disabled={conversationBusy}>
              <strong>{conversation.title}</strong>
              <span>{conversation.modelNameSnapshot ?? "Choose a model"} · {new Date(conversation.updatedAt).toLocaleDateString()}</span>
            </button>
          ))}
        </div>
      </aside>

      <div className="lumen-chat-main">
        <header className="lumen-chat-toolbar">
          <div>
            <span className="eyebrow">Conversation</span>
            <h1>{current.title}</h1>
          </div>
          <div className="lumen-chat-toolbar-actions">
            <button className="secondary-button" type="button" onClick={renameConversation} disabled={conversationBusy}>Rename</button>
            <button className="secondary-button" type="button" onClick={clearConversation} disabled={conversationBusy || current.messages.length === 0}>Clear</button>
            <button className="secondary-button" type="button" onClick={() => exportConversation("markdown")}>Export MD</button>
            <button className="secondary-button" type="button" onClick={() => exportConversation("json")}>Export JSON</button>
            <button className="secondary-button danger" type="button" onClick={() => setDeletePending(true)} disabled={conversationBusy}>Delete</button>
          </div>
        </header>

        {error && <div className="inline-error" role="alert">{error}</div>}
        {compatibleModels.length === 0 && <div className="api-notice warning"><strong>No chat-capable model is available</strong><span>Install or reconnect a conversational model from the Models section to use Lumen Chat.</span></div>}
        {requestedModelUnsupported && <div className="api-notice warning"><strong>{requestedModel?.name} cannot generate chat responses</strong><span>Select one of the compatible installed models below. This model’s Lumen Chat tab remains available so the workspace and history stay consistent.</span></div>}
        {persistedModelMissing && <div className="api-notice warning"><strong>Previous model unavailable</strong><span>Choose another installed chat model. The conversation history has been preserved.</span></div>}
        {historyNotice && <div className="api-notice warning"><strong>Before sending</strong><span>{historyNotice}</span></div>}

        <div className="lumen-chat-controls">
          <label>Model
            <select value={selectedModel?.id ?? ""} onChange={(event) => selectModel(event.target.value)} disabled={conversationBusy || compatibleModels.length === 0}>
              <option value="" disabled>Choose a model</option>
              {compatibleModels.map((model) => <option key={model.id} value={model.id}>{model.name} · {model.running ? "running" : "stopped"} · {model.location}{model.modelSettings?.contextLength ? ` · ${model.modelSettings.contextLength.toLocaleString()} ctx` : ""}</option>)}
            </select>
          </label>
          <button className="secondary-button" type="button" onClick={setAsDefault} disabled={!selectedModel || settings?.defaultModelEntryId === selectedModel.id}>Set as default</button>
          {selectedModel && !selectedModel.running && <button className="primary-button" type="button" onClick={() => onStartModel(selectedModel.id)}>Start {selectedModel.name}</button>}
          <span className="context-estimate">≈ {estimatedTokens.toLocaleString()} tokens{contextLimit ? ` / ${contextLimit.toLocaleString()}` : ""}</span>
        </div>

        <div className="lumen-chat-body">
          <div className="lumen-chat-transcript" ref={transcriptRef} role="log" aria-live="polite" aria-label={`Conversation ${current.title}`}>
            {current.messages.length > MAX_RENDERED_MESSAGES && <div className="chat-window-notice">Showing the most recent {MAX_RENDERED_MESSAGES} messages. Older messages remain saved and searchable.</div>}
            {renderedMessages.length === 0 ? (
              <div className="chat-empty"><strong>Start a private conversation</strong><span>Select a local model, adjust the conversation settings if needed, and send a message.</span></div>
            ) : renderedMessages.map((message) => (
              <article className={`lumen-message ${message.role} ${message.status}`} key={message.id}>
                <header><strong>{message.role === "user" ? "You" : selectedModel?.name ?? current.modelNameSnapshot ?? "Assistant"}</strong><span>{message.status === "generating" ? generationPhase ?? "Generating…" : message.status}</span></header>
                <SafeMarkdown
                  content={message.content}
                  busy={message.status === "generating"}
                  emptyLabel={message.status === "generating"
                    ? generationPhase
                    : message.status === "failed"
                      ? "No response was produced."
                      : message.status === "stopped"
                        ? "Generation stopped."
                        : "No response text was returned."}
                  copyPrefix={`chat-code-${message.id}`}
                  copiedField={copiedField}
                  onCopy={onCopy}
                />
                <footer>
                  <button type="button" onClick={() => onCopy(message.content, `chat-message-${message.id}`)}>{copiedField === `chat-message-${message.id}` ? "✓ Copied" : "Copy"}</button>
                  {message.role === "user" && <button type="button" onClick={() => editAndResend(message.id)} disabled={conversationBusy}>Edit and resend</button>}
                </footer>
              </article>
            ))}
          </div>

          <aside className="conversation-options">
            <section className="system-prompt-file" aria-labelledby="system-prompt-file-title">
              <input
                ref={systemPromptInputRef}
                type="file"
                accept=".txt,.md,.markdown,.prompt,text/plain,text/markdown"
                onChange={(event) => void loadSystemPrompt(event)}
                disabled={conversationBusy}
                hidden
              />
              <div>
                <span id="system-prompt-file-title">System prompt file</span>
                <strong>{current.systemPrompt ? current.systemPromptName ?? "Saved system prompt" : "Model default"}</strong>
                <small>{current.systemPrompt
                  ? `${current.systemPrompt.length.toLocaleString()} characters · stored with this conversation`
                  : "No conversation-specific system prompt"}</small>
              </div>
              <div className="system-prompt-file-actions">
                <button className="secondary-button" type="button" onClick={() => systemPromptInputRef.current?.click()} disabled={conversationBusy}>Load file</button>
                <button className="secondary-button" type="button" onClick={clearSystemPrompt} disabled={conversationBusy || !current.systemPrompt}>Use model default</button>
              </div>
              {current.systemPrompt && (
                <details>
                  <summary>Preview prompt</summary>
                  <pre>{current.systemPrompt}</pre>
                </details>
              )}
            </section>
            <label>Temperature <span>{current.parameters.temperature ?? "Inherited"}</span>
              <input type="range" min="0" max="2" step="0.1" value={current.parameters.temperature ?? selectedModel?.modelSettings?.temperature ?? 0.7} onChange={(event) => updateConversation({ parameters: { ...current.parameters, temperature: Number(event.target.value) } })} disabled={conversationBusy} />
            </label>
            <label>Maximum output tokens
              <input type="number" min="1" value={current.parameters.maxOutputTokens ?? ""} placeholder={selectedModel?.modelSettings?.maxOutputTokens?.toString() ?? "Inherited"} onChange={(event) => updateConversation({ parameters: { ...current.parameters, maxOutputTokens: event.target.value ? Number(event.target.value) : undefined } })} disabled={conversationBusy} />
            </label>
            <label className="save-history-choice"><input type="checkbox" checked={!current.saveHistory} onChange={(event) => setSaveHistory(!event.target.checked)} disabled={conversationBusy} /> Do not save this chat</label>
            <p>{current.saveHistory ? "Stored only in Lumen Source application data." : "This conversation will be removed from disk and kept only until it is closed."}</p>
          </aside>
        </div>

        <div className="lumen-chat-composer">
          <textarea rows={3} maxLength={65_536} value={draft} onChange={(event) => setDraft(event.target.value)} onKeyDown={handleComposerKeyDown} placeholder={selectedModel?.running ? "Message this model…" : "Choose and start a model first"} disabled={conversationBusy} aria-label="Chat message" />
          <div>
            <small>Enter to send · Shift+Enter for a new line · Requests include at most the latest {MAX_REQUEST_MESSAGES} messages</small>
            <div>
              <button className="secondary-button" type="button" onClick={regenerate} disabled={conversationBusy || !lastAssistant}>{lastAssistant?.status === "failed" ? "Retry" : "Regenerate"}</button>
              {activeRequest
                ? <button className="secondary-button" type="button" onClick={() => void stop()} disabled={cancelBusy}>{cancelBusy ? "Stopping…" : "Stop"}</button>
                : currentGenerating
                  ? <button className="secondary-button" type="button" disabled>Generating in background…</button>
                  : <button className="primary-button" type="button" onClick={send} disabled={!draft.trim() || !selectedModel?.running}>Send</button>}
            </div>
          </div>
        </div>
      </div>

      {deletePending && (
        <div className="modal-backdrop" onClick={() => setDeletePending(false)}>
          <div
            ref={deleteDialogRef}
            tabIndex={-1}
            className="modal-card chat-delete-dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="chat-delete-dialog-title"
            onClick={(event) => event.stopPropagation()}
          >
            <span className="eyebrow">Delete conversation</span>
            <h3 id="chat-delete-dialog-title">Lumen Source</h3>
            <p>Delete “{current.title}” and its local messages?</p>
            <div className="modal-actions">
              <button className="secondary-button" type="button" data-modal-initial-focus onClick={() => setDeletePending(false)}>Cancel</button>
              <button className="primary-button" type="button" onClick={() => void deleteConversation()}>Delete conversation</button>
            </div>
          </div>
        </div>
      )}
    </section>
  );
}

function SafeMarkdown({ content, busy, emptyLabel, copyPrefix, copiedField, onCopy }: { content: string; busy?: boolean; emptyLabel?: string; copyPrefix: string; copiedField?: string; onCopy: (value: string, key: string) => void }) {
  if (!content) return <p className="generating-markdown">{busy && <span className="model-control-spinner" />} {emptyLabel ?? "No response text was returned."}</p>;
  const blocks = content.split(/```/);
  return <div className="safe-markdown">{blocks.map((block, index) => {
    if (index % 2 === 1) {
      const newline = block.indexOf("\n");
      const language = newline >= 0 ? block.slice(0, newline).trim() : "";
      const code = newline >= 0 ? block.slice(newline + 1) : block;
      const key = `${copyPrefix}-${index}`;
      return <div className="markdown-code" key={key}><header><span>{language || "code"}</span><button type="button" onClick={() => onCopy(code, key)}>{copiedField === key ? "✓ Copied" : "Copy code"}</button></header><pre><code className={language ? `language-${language}` : undefined}>{highlightCode(code)}</code></pre></div>;
    }
    return block.split(/\n{2,}/).filter(Boolean).map((paragraph, paragraphIndex) => {
      const trimmed = paragraph.trim();
      if (trimmed.startsWith("### ")) return <h4 key={`${index}-${paragraphIndex}`}>{inlineMarkdown(trimmed.slice(4))}</h4>;
      if (trimmed.startsWith("## ")) return <h3 key={`${index}-${paragraphIndex}`}>{inlineMarkdown(trimmed.slice(3))}</h3>;
      if (trimmed.startsWith("# ")) return <h2 key={`${index}-${paragraphIndex}`}>{inlineMarkdown(trimmed.slice(2))}</h2>;
      if (trimmed.split("\n").every((line) => /^[-*] /.test(line))) return <ul key={`${index}-${paragraphIndex}`}>{trimmed.split("\n").map((line) => <li key={line}>{inlineMarkdown(line.slice(2))}</li>)}</ul>;
      return <p key={`${index}-${paragraphIndex}`}>{inlineMarkdown(trimmed)}</p>;
    });
  })}</div>;
}

function inlineMarkdown(value: string): ReactNode[] {
  return value.split(/(`[^`]+`|\*\*[^*]+\*\*)/g).filter(Boolean).map((part, index) => {
    if (part.startsWith("`") && part.endsWith("`")) return <code key={index}>{part.slice(1, -1)}</code>;
    if (part.startsWith("**") && part.endsWith("**")) return <strong key={index}>{part.slice(2, -2)}</strong>;
    return <span key={index}>{part}</span>;
  });
}

function highlightCode(code: string): ReactNode[] {
  const pattern = /(\/\/[^\n]*|#[^\n]*|"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|\b(?:const|let|var|function|return|if|else|for|while|class|struct|impl|fn|async|await|pub|use|import|export|from|def|match|true|false|null|None|Some)\b|\b\d+(?:\.\d+)?\b)/g;
  return code.split(pattern).filter(Boolean).map((token, index) => {
    const className = token.startsWith("//") || token.startsWith("#") ? "syntax-comment"
      : token.startsWith("\"") || token.startsWith("'") ? "syntax-string"
        : /^\d/.test(token) ? "syntax-number"
          : /^(const|let|var|function|return|if|else|for|while|class|struct|impl|fn|async|await|pub|use|import|export|from|def|match|true|false|null|None|Some)$/.test(token) ? "syntax-keyword"
            : undefined;
    return <span className={className} key={index}>{token}</span>;
  });
}

function safeFilename(title: string): string {
  return title.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 64) || "lumen-chat";
}

function promptFromConversation(conversation?: Conversation): SystemPromptSelection {
  return conversation
    ? { content: conversation.systemPrompt, name: conversation.systemPromptName }
    : { content: "" };
}

function loadLastSystemPrompt(): SystemPromptSelection | undefined {
  try {
    const stored = window.localStorage.getItem(LAST_SYSTEM_PROMPT_KEY);
    if (!stored) return undefined;
    const parsed = JSON.parse(stored) as Partial<SystemPromptSelection>;
    if (typeof parsed.content !== "string" || (parsed.name !== undefined && typeof parsed.name !== "string")) return undefined;
    return { content: parsed.content, name: parsed.name };
  } catch {
    return undefined;
  }
}

function storeLastSystemPrompt(selection: SystemPromptSelection) {
  try {
    window.localStorage.setItem(LAST_SYSTEM_PROMPT_KEY, JSON.stringify(selection));
  } catch {
    // Conversation persistence still retains the prompt if browser storage is unavailable.
  }
}

function formatElapsed(milliseconds: number): string {
  if (milliseconds < 1_000) return `${Math.max(0, Math.round(milliseconds))} ms`;
  return `${(milliseconds / 1_000).toFixed(milliseconds < 10_000 ? 1 : 0)} s`;
}

function diagnosticFailure(message: string): string {
  const normalized = message.toLowerCase();
  const status = normalized.match(/(?:http(?: status)?|status)\D{0,8}(\d{3})/)?.[1];
  if (status) return `runtime returned HTTP ${status}`;
  if (normalized.includes("timeout") || normalized.includes("timed out")) return "runtime request timed out";
  if (normalized.includes("connect") || normalized.includes("connection") || normalized.includes("refused")) return "runtime connection failed";
  if (normalized.includes("authentication") || normalized.includes("unauthorized") || normalized.includes("forbidden")) return "runtime authentication failed";
  if (normalized.includes("invalid response") || normalized.includes("decode") || normalized.includes("json")) return "runtime returned an invalid response";
  if (normalized.includes("another chat")) return "another generation request was already active";
  return "runtime request failed; see the inline error for details";
}

function downloadText(filename: string, body: string, type: string) {
  const url = URL.createObjectURL(new Blob([body], { type }));
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  link.click();
  URL.revokeObjectURL(url);
}
