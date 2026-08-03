import { useCallback, useEffect, useMemo, useState } from "react";
import { desktopCommands } from "../commands";
import { integrationDefinitions, type IntegrationId } from "../integrations";
import type { RunningModelEntry, SharingStatus } from "../types";

interface IntegrationPanelProps {
  model: RunningModelEntry;
  copiedField?: string;
  errorFrom: (error: unknown) => string;
  onCopy: (value: string, key: string) => void;
}

export function IntegrationPanel({
  model,
  copiedField,
  errorFrom,
  onCopy,
}: IntegrationPanelProps) {
  const [status, setStatus] = useState<SharingStatus>();
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string>();
  const [selectedId, setSelectedId] = useState<IntegrationId>();

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(undefined);
    try {
      setStatus(await desktopCommands.sharingStatus());
    } catch (failure) {
      setError(errorFrom(failure));
    } finally {
      setLoading(false);
    }
  }, [errorFrom]);

  useEffect(() => {
    void refresh();
  }, [model.id, refresh]);

  const exposedModel = status?.exposedModelDetails.find(({ entryId }) => entryId === model.id);
  const providerEligible = Boolean(
    model.managed
    && model.location === "local"
    && model.inventoryStatus === "available"
    && (model.runtimeId === "ollama" || model.runtimeId === "vllm")
    && model.runtimeCapabilities.lifecycle !== "external",
  );
  const providerReady = Boolean(status?.running && status.address && exposedModel);
  const availableProtocols = [
    model.runtimeCapabilities.chat && "Chat Completions",
    model.runtimeCapabilities.embeddings && "Embeddings",
  ].filter(Boolean).join(" · ") || "No compatible inference protocol";
  const connection = useMemo(() => ({
    baseUrl: status?.address ?? "http://127.0.0.1:<port>/v1",
    modelId: exposedModel?.publicName ?? "<lumen-public-model-id>",
  }), [exposedModel?.publicName, status?.address]);
  const selected = integrationDefinitions.find(({ id }) => id === selectedId);

  return (
    <section className="detail-section integration-section" aria-labelledby="model-integration-title">
      <div className="detail-section-header">
        <div>
          <h2 id="model-integration-title">Integrations</h2>
          <p>Connect this model to other tools through the authenticated Lumen Source provider. The initial integrations are registry-driven so more can be added without changing this page.</p>
        </div>
        <button className="secondary-button" type="button" onClick={() => void refresh()} disabled={loading}>
          {loading ? "Checking…" : "Refresh status"}
        </button>
      </div>

      {error && <div className="inline-error" role="alert">{error}</div>}

      <div className={`api-notice ${providerReady ? "" : "warning"}`} role="status">
        <strong>{providerReady ? "Provider ready for configuration review" : providerEligible ? "Provider setup required" : "Provider integrations unavailable for this model"}</strong>
        <span>
          {providerReady
            ? "This model is exposed through the loopback gateway. Lumen Source must remain open while an integration uses it."
            : providerEligible
              ? "Enable the provider gateway, generate a token, and expose this model in Settings before installing an integration."
              : "Phase 1 provider integrations require an available local model managed by Lumen Source."}
        </span>
      </div>

      <div className="endpoint-card api-endpoint-card integration-connection-card">
        <ConnectionValue label="Provider status" value={providerReady ? "Running · model exposed" : status?.running ? "Running · model not exposed" : "Not running"} />
        <ConnectionValue label="Base URL" value={connection.baseUrl} copyKey={`integration-base-${model.id}`} copiedField={copiedField} onCopy={onCopy} />
        <ConnectionValue label="Public model ID" value={connection.modelId} copyKey={`integration-model-${model.id}`} copiedField={copiedField} onCopy={onCopy} />
        <ConnectionValue label="Authentication" value={status?.tokenSaved ? "Bearer token saved in credential store" : "Bearer token required"} />
        <ConnectionValue label="Available protocols" value={availableProtocols} />
      </div>

      <div className="integration-grid">
        {integrationDefinitions.map((definition) => (
          <article className={`integration-card ${selectedId === definition.id ? "selected" : ""}`} key={definition.id}>
            <div className="integration-card-heading">
              <h3>{definition.name}</h3>
              <span className={`integration-badge ${definition.badgeTone}`}>{definition.badge}</span>
            </div>
            <p>{definition.description}</p>
            <small>{definition.compatibility}</small>
            <button className="secondary-button" type="button" onClick={() => setSelectedId((current) => current === definition.id ? undefined : definition.id)}>
              {selectedId === definition.id ? "Close review" : definition.actionLabel}
            </button>
          </article>
        ))}
      </div>

      {selected && (
        <section className="integration-review" aria-labelledby={`integration-review-${selected.id}`}>
          <div className="integration-review-heading">
            <div>
              <span className="eyebrow">Configuration preview</span>
              <h3 id={`integration-review-${selected.id}`}>{selected.actionLabel}</h3>
            </div>
            <button
              className="secondary-button"
              type="button"
              onClick={() => onCopy(selected.configuration(connection), `integration-config-${model.id}-${selected.id}`)}
            >
              {copiedField === `integration-config-${model.id}-${selected.id}` ? "✓ Copied" : "Copy configuration"}
            </button>
          </div>
          <div className="integration-fields">
            {selected.fields(connection).map((field) => (
              <ConnectionValue
                key={field.label}
                label={field.label}
                value={field.value}
                copyKey={`integration-${model.id}-${selected.id}-${field.label}`}
                copiedField={copiedField}
                onCopy={onCopy}
              />
            ))}
          </div>
          <pre tabIndex={0}><code>{selected.configuration(connection)}</code></pre>
          <ul>
            {selected.notes.map((note) => <li key={note}>{note}</li>)}
          </ul>
          {!providerReady && <p className="integration-review-blocked">Installation is unavailable until the provider setup above is complete.</p>}
        </section>
      )}
    </section>
  );
}

function ConnectionValue({
  label,
  value,
  copyKey,
  copiedField,
  onCopy,
}: {
  label: string;
  value: string;
  copyKey?: string;
  copiedField?: string;
  onCopy?: (value: string, key: string) => void;
}) {
  return (
    <div>
      <span>{label}</span>
      <div className="copy-value">
        <code>{value}</code>
        {copyKey && onCopy && (
          <button className={copiedField === copyKey ? "copied" : ""} type="button" aria-live="polite" onClick={() => onCopy(value, copyKey)}>
            {copiedField === copyKey ? "✓ Copied" : "Copy"}
          </button>
        )}
      </div>
    </div>
  );
}
