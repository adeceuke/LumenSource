export type IntegrationId = "codex" | "copilot" | "cursor";

export interface IntegrationConnection {
  baseUrl: string;
  modelId: string;
}

export interface IntegrationField {
  label: string;
  value: string;
}

export interface IntegrationDefinition {
  id: IntegrationId;
  name: string;
  actionLabel: string;
  description: string;
  badge: string;
  badgeTone: "blocked" | "pending" | "experimental";
  compatibility: string;
  fields: (connection: IntegrationConnection) => IntegrationField[];
  configuration: (connection: IntegrationConnection) => string;
  notes: string[];
}

function profileSlug(modelId: string): string {
  return modelId
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "")
    .slice(0, 48) || "model";
}

export const integrationDefinitions: readonly IntegrationDefinition[] = [
  {
    id: "codex",
    name: "Codex",
    actionLabel: "Add in Codex",
    description: "Create a separate Codex profile that uses the authenticated Lumen provider.",
    badge: "Responses required",
    badgeTone: "blocked",
    compatibility: "Unavailable until the Lumen gateway supports and verifies the Responses API for this model.",
    fields: ({ baseUrl, modelId }) => [
      { label: "Profile", value: `lumen-${profileSlug(modelId)}` },
      { label: "Base URL", value: baseUrl },
      { label: "Model ID", value: modelId },
      { label: "Token variable", value: "LUMEN_SOURCE_API_KEY" },
      { label: "Wire API", value: "responses" },
    ],
    configuration: ({ baseUrl, modelId }) => {
      const profile = `lumen-${profileSlug(modelId)}`;
      return `# ~/.codex/${profile}.config.toml\nmodel = "${modelId}"\nmodel_provider = "lumen_source"\n\n[model_providers.lumen_source]\nname = "Lumen Source"\nbase_url = "${baseUrl}"\nenv_key = "LUMEN_SOURCE_API_KEY"\nwire_api = "responses"\n\n# Launch after setting LUMEN_SOURCE_API_KEY\ncodex --profile ${profile}`;
    },
    notes: [
      "The token is never written into the profile.",
      "Profile installation remains disabled until Responses compatibility passes.",
    ],
  },
  {
    id: "copilot",
    name: "GitHub Copilot CLI",
    actionLabel: "Add in Copilot",
    description: "Preview process-local environment variables for GitHub Copilot CLI.",
    badge: "Verification pending",
    badgeTone: "pending",
    compatibility: "Streaming and tool-call probes must pass before Lumen Source can mark this integration ready.",
    fields: ({ baseUrl, modelId }) => [
      { label: "Provider type", value: "openai" },
      { label: "Base URL", value: baseUrl },
      { label: "Model ID", value: modelId },
      { label: "Token variable", value: "LUMEN_SOURCE_API_KEY" },
    ],
    configuration: ({ baseUrl, modelId }) =>
      `export COPILOT_PROVIDER_TYPE=openai\nexport COPILOT_PROVIDER_BASE_URL='${baseUrl}'\nexport COPILOT_PROVIDER_API_KEY="$LUMEN_SOURCE_API_KEY"\nexport COPILOT_MODEL='${modelId}'\n\n# Optional only when every provider is local:\n# export COPILOT_OFFLINE=true\ncopilot`,
    notes: [
      "Phase 1 targets GitHub Copilot CLI, not editor inline completion.",
      "Lumen Source will not edit shell startup files automatically.",
    ],
  },
  {
    id: "cursor",
    name: "Cursor",
    actionLabel: "Add in Cursor",
    description: "Review the connection values for a manual Cursor setup.",
    badge: "Experimental",
    badgeTone: "experimental",
    compatibility: "Cursor support depends on the installed version exposing a documented custom base-URL setting.",
    fields: ({ baseUrl, modelId }) => [
      { label: "Provider", value: "OpenAI-compatible" },
      { label: "Base URL", value: baseUrl },
      { label: "Model ID", value: modelId },
      { label: "API token", value: "Use the Lumen Source sharing token" },
    ],
    configuration: ({ baseUrl, modelId }) =>
      `Provider: OpenAI-compatible\nBase URL: ${baseUrl}\nModel: ${modelId}\nAPI token: <Lumen Source sharing token>`,
    notes: [
      "No undocumented Cursor state files will be changed.",
      "Cursor Tab and specialized agent features may continue to use Cursor-hosted models.",
    ],
  },
] as const;
