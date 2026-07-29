# Runtime and settings implementation plan

This document tracks the work required to add application settings, global
Ollama and vLLM configuration, vLLM support, and per-model settings to Lumen
Source.

The plan has three delivery steps. Complete them in order because each step
provides the persistence and runtime boundaries required by the next one.

## Product rules

- Ollama is the default runtime on a fresh installation. The user is not asked
  to choose a runtime during onboarding.
- The selected default runtime applies to new model installations only.
  Existing models retain the runtime with which they were installed.
- The catalog is refreshed from the network at every startup. When the online
  catalog is unavailable, Lumen Source falls back to the last verified cached
  catalog and then to the bundled catalog.
- Global runtime settings configure only services managed by Lumen Source.
  Connections to separately managed Ollama or vLLM services are configured for
  the individual model that uses them.
- Changing a model to another runtime is a separate reinstall/migration
  operation; it must never happen silently.
- Settings use this inheritance order:

  ```text
  Lumen Source defaults
      -> global runtime defaults
          -> model-specific overrides
              -> individual API request values
  ```

- The UI distinguishes settings that apply immediately, on the next request,
  after a model restart, after a runtime restart, or only to new installations.
- Secrets such as API keys and Hugging Face tokens are stored in the operating
  system credential store, never in `state.json` or telemetry.
- Managed services bind to loopback by default. Exposing a runtime to the
  network requires an explicit advanced setting and security warning.
- vLLM is initially supported as an existing Linux-hosted service. Managed
  deployment follows after external endpoint support is stable. Native Windows
  vLLM installation is not part of the initial scope.

## Step 1: Build the settings foundation and configurable Ollama experience

Goal: introduce versioned settings, the global Settings page, and useful Ollama
configuration without changing the default installation experience.

### Settings domain and persistence

- [x] Add versioned `ApplicationSettings`, `OllamaSettings`, `VllmSettings`,
      `ModelSettings`, `RuntimeId`, and `RuntimeManagementMode` types.
- [x] Default `RuntimeId` to Ollama when settings are absent.
- [x] Add backward-compatible deserialization and explicit migrations.
- [x] Add Tauri commands to load, validate, save, and reset settings.
- [x] Keep settings writes atomic through the existing state-writing path.
- [x] Add credential-store operations for vLLM and Hugging Face secrets.
- [x] Exclude settings and secrets from telemetry and ordinary lifecycle logs.

### Global Settings page

- [x] Add **Settings** below **Models** and **Machines** in the left navigation.
- [x] Add General, Runtimes, Storage, Privacy and telemetry, and About sections.
- [x] Add these Lumen Source settings:
  - [x] Default runtime for new installations
  - [x] Default installation target
  - [x] Start model after installation
  - [x] Automatically start managed runtimes when required
  - [x] Model and cache storage locations
  - [x] Telemetry preference
  - [x] Lifecycle-log retention
  - [x] Model-deletion confirmation
- [x] Show Ollama as selected by default without a first-run runtime question.
- [x] Explain that changing the default does not alter installed models.
- [x] Add Save, Reset, validation errors, unsaved-change protection, and
      restart-required indicators.

### Global Ollama settings

- [x] Add basic Ollama controls:
  - [x] Endpoint URL
  - [x] Auto-detected executable path with manual override
  - [x] Default context length, with `Auto` as the default
  - [x] Keep-alive duration
  - [x] Maximum loaded models
  - [x] Parallel requests per model
  - [x] Maximum queued requests
- [x] Add a connection test that reports endpoint health and version.
- [x] Estimate the RAM/VRAM effect of context and concurrency where possible.
- [x] Add collapsed advanced controls for GPU selection, experimental Vulkan,
      bind address, allowed origins, and debug logging.
- [x] Require confirmation before changing from a loopback bind address.
- [x] Apply managed-server environment changes only after explicit restart.
- [x] Explain that these settings apply only to Ollama processes launched by
      Lumen Source; separately managed services are a per-model concern.

### Step 1 acceptance criteria

- [x] A fresh installation uses Ollama without asking the user.
- [x] An existing v0.4 state loads without data loss.
- [x] Settings survive an application restart.
- [x] Secrets are stored only in the credential store.
- [x] The Settings page and basic controls are keyboard accessible.
- [x] Invalid URLs, ports, durations, context lengths, and concurrency limits
      cannot be saved.
- [x] Changing the default runtime does not change installed model entries.
- [x] Rust tests, TypeScript checks, frontend build, and the complete Windows
      `scripts/container.ps1 check` workflow pass.

## Step 2: Generalize runtime handling and add vLLM connectivity

Goal: remove Ollama-specific assumptions from shared orchestration and support
an existing vLLM OpenAI-compatible server safely.

### Runtime architecture

- [ ] Add a runtime registry/resolver keyed by `RuntimeId`.
- [ ] Separate runtime connection, model discovery, artifact acquisition,
      managed lifecycle, inference capabilities, and settings validation.
- [ ] Define capabilities for:
  - [ ] Managed model storage
  - [ ] Multiple installed models
  - [ ] Chat, embeddings, and pooling
  - [ ] Model start/stop
  - [ ] Global and per-model configuration
  - [ ] Lumen Source-managed versus externally managed lifecycle
- [ ] Replace Ollama-name checks in the bridge, reconciliation, endpoints,
      performance reporting, and remote flow with capability checks.
- [ ] Preserve the dummy runtime for deterministic development and tests.

### Catalog and installed-model identity

- [ ] Extend catalog variants with:
  - [ ] Ollama model reference/tag
  - [ ] vLLM Hugging Face model identifier
  - [ ] Pinned model and tokenizer revisions
  - [ ] Supported task/runner
  - [ ] Quantization/runtime compatibility
  - [ ] Download size and hardware requirements
- [ ] Persist `runtimeId` on every installed model.
- [ ] Keep runtime identifiers separate from catalog IDs and display names.
- [ ] Reconcile Ollama, vLLM, dummy, local, and remote entries together.
- [ ] Prevent removal in one runtime from deleting another runtime's copy.

### Existing vLLM server support

- [ ] Add per-model external-runtime connection settings rather than a global
      endpoint for separately managed services.
- [ ] Add a vLLM client for health, `/v1/models`, chat completions, embeddings,
      endpoint details, and authentication reporting.
- [ ] Add endpoint URL, credential-store API key, TLS verification, connection
      timeout, request timeout, and Test connection settings.
- [ ] Clearly label externally managed vLLM services.
- [ ] Hide lifecycle and engine settings that Lumen Source cannot apply.
- [ ] Account for one model normally being hosted by each vLLM server instance.
- [ ] Do not expose raw vLLM command-line arguments.

### Defaults for future managed vLLM instances

- [ ] Persist curated defaults for:
  - [ ] Hugging Face cache directory and credential-store token
  - [ ] GPU/device selection
  - [ ] GPU-memory utilization
  - [ ] Maximum context length and concurrent sequences
  - [ ] Prefix caching
  - [ ] Weight data type and quantization
  - [ ] KV-cache data type or offloading
  - [ ] Tensor- and pipeline-parallel counts
  - [ ] Bind address and managed port range
  - [ ] Pinned vLLM runtime/container version
- [ ] Validate version-sensitive values against the supported pinned version.
- [ ] Keep `trust_remote_code` and local-media filesystem access disabled until
      a separate security-reviewed workflow exists.

### Step 2 acceptance criteria

- [ ] Ollama behavior remains unchanged when vLLM is not configured.
- [ ] A user can configure, authenticate to, test, and use a vLLM endpoint.
- [ ] UI actions are rendered from capabilities instead of runtime-name checks.
- [ ] External vLLM servers are never stopped, restarted, or reconfigured.
- [ ] Ollama and vLLM models coexist in the Models page.
- [ ] API keys and tokens are absent from JSON state, logs, errors, and telemetry.
- [ ] Failures have actionable connection/authentication/capability messages.
- [ ] Contract tests cover Ollama, vLLM, dummy, and external-service behavior.

## Step 3: Add per-model settings, managed vLLM, and production hardening

Goal: allow safe model-level overrides and complete managed vLLM deployment on
supported Linux GPU machines.

### Model Settings tab

- [ ] Extend model detail tabs to:

  ```text
  Chat | Logs | Performance | API | Settings
  ```

- [ ] Persist `ModelSettings` with every managed model.
- [ ] Render controls from the runtime's capabilities.
- [ ] Show whether values are inherited or overridden.
- [ ] Add **Use runtime default**, **Save**, **Reset overrides**,
      **Discard changes**, and **Apply and restart** actions.
- [ ] Separate **Lumen Source chat defaults** from **Runtime/load settings**.
- [ ] Add chat defaults for system prompt, temperature, maximum output tokens,
      top-p, top-k, min-p, repetition penalty, seed, stop sequences, structured
      output, and supported reasoning level.
- [ ] Add load settings for context, keep-alive, load-on-startup, and preferred
      accelerator/device.
- [ ] Explain that external API clients can override request parameters.
- [ ] Validate settings against catalog limits and detected memory.

### Runtime-specific model settings

- [ ] Add Ollama overrides for persistent parameters, system prompt, optional
      derived model identity, and supported GPU/backend selection.
- [ ] Avoid duplicate derived models unless a persistent Modelfile override
      requires one.
- [ ] Add managed-vLLM settings for model/tokenizer revision, served name,
      task/runner, data type, quantization, GPU-memory utilization, context,
      concurrency, prefix caching, KV cache, and GPU parallelism.
- [ ] Make external vLLM engine settings read-only.
- [ ] Require explicit restart for load-time changes.
- [ ] Roll back to the last working configuration when restart fails.

### Managed vLLM deployment

- [ ] Support compatible Linux GPU targets using a pinned official container.
- [ ] Detect Docker/Podman and required GPU-container support.
- [ ] Never install Docker, drivers, or GPU runtimes without a separate,
      explicit user-approved workflow.
- [ ] Mount persistent Hugging Face and vLLM compile caches.
- [ ] Allocate one server instance and port per served model.
- [ ] Reserve ports atomically and detect conflicts.
- [ ] Launch predetermined arguments without a shell.
- [ ] Bind to loopback by default; require authentication for network exposure.
- [ ] Implement start, health, stop, restart, logs, and removal.
- [ ] Separate server removal from confirmed cache/model deletion.
- [ ] Acceptance-test NVIDIA first; add ROCm and Intel only after dedicated
      hardware validation.
- [ ] Keep native Windows vLLM management out of scope until upstream provides
      a supportable path.

### Migration and release hardening

- [ ] Add **Reinstall with another runtime** only after both runtimes are stable.
- [ ] Verify an equivalent catalog variant before migration.
- [ ] Validate the replacement before offering to remove the old copy.
- [ ] Log configuration changes without secrets.
- [ ] Add runtime version, health, effective settings, and restart diagnostics.
- [ ] Update API, performance, and model status views for both runtimes.
- [ ] Document unsupported combinations and recovery procedures.
- [ ] Update Windows/Linux and local/remote acceptance matrices.

### Step 3 acceptance criteria

- [ ] Model settings inherit global defaults and survive restarts.
- [ ] Request-time changes apply without restart; load changes clearly require it.
- [ ] Failed changes do not leave a managed model unusable.
- [ ] Existing models keep their runtime when the global default changes.
- [ ] Managed vLLM can resolve, launch, serve, restart, stop, and remove a
      catalog model on a supported Linux GPU target.
- [ ] Multiple vLLM models receive distinct ports and process identities.
- [ ] External vLLM endpoints remain connection-only.
- [ ] Windows remains fully functional with Ollama when vLLM is unavailable.
- [ ] Migration, credentials, cancellation, rollback, and removal tests pass.
- [ ] Documentation clearly distinguishes managed and external runtimes.

## References

- [Ollama FAQ and server settings](https://docs.ollama.com/faq)
- [Ollama context length](https://docs.ollama.com/context-length)
- [Ollama Modelfile parameters](https://docs.ollama.com/modelfile)
- [Ollama chat API](https://docs.ollama.com/api/chat)
- [vLLM quickstart](https://docs.vllm.ai/en/latest/getting_started/quickstart/)
- [vLLM engine arguments](https://docs.vllm.ai/en/latest/configuration/engine_args/)
- [vLLM server arguments](https://docs.vllm.ai/en/latest/cli/serve/)
- [Official vLLM containers](https://docs.vllm.ai/en/latest/deployment/docker/)
