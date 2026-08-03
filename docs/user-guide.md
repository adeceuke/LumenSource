# Lumen Source user guide

Lumen Source helps people choose, install, validate, run, update, and remove
local AI models without requiring runtime flags or container commands. Ollama
is the default. Every installation uses the same **Add model** assistant,
whether it is the first model or the fiftieth.

## Install Lumen Source

On Windows, run the MSI or NSIS setup installer. On Ubuntu 24.04 x86_64,
install the deb package or run the verified AppImage. Packages and their
SHA-256 checksums are published together. When a Windows installer is unsigned,
the release notes and signature report say so explicitly; verify its checksum
and expect an unknown-publisher warning before installation.

Lumen Source can use an existing Ollama installation. If Ollama is missing,
the installation assistant can download the catalog-pinned runtime after you
explicitly approve it. Ollama is not bundled in the Lumen Source installer.
Managed vLLM requires Linux, an NVIDIA GPU with working container GPU support,
and Docker or Podman.

## Choose and install a model

Select **Add model**, choose the local or supported remote target, and select
what you want to do. Recommendations account for the detected CPU, RAM,
storage, accelerator, download size, and estimated loaded-memory range.

The default **Balanced** profile calculates context, concurrency, offload, and
accelerator settings. **Safe** reserves more memory headroom; **Fast** uses more
available resources; **Custom** exposes runtime-specific controls. Lumen Source
shows the effective values before applying them.

Review the model license and any usage policy. An installation is marked Ready
only after the runtime responds to a deterministic validation request. If that
fails, retry, choose safer settings, open diagnostics, or remove the incomplete
installation.

## Manage models

Models shows installed, externally discovered, stale, and running entries.
Select a model for Chat, Logs, Performance, API, and Settings. Pin a model if
resource orchestration should avoid stopping it. Before an over-budget start,
Lumen Source lists the exact models it proposes to stop.

Settings belong to the model entry. Separately managed Ollama or vLLM services
remain external: Lumen Source can connect to them but does not start, stop,
update, secure, or firewall them.

## Storage

Storage separates model weights, Ollama data, Hugging Face cache, vLLM compile
cache, runtime downloads, and temporary files. Exact runtime values are labeled
separately from directory estimates. Cleanup always shows its scope and what
must be downloaded or compiled again. Removing a server never silently removes
a shared cache.

## Updates and rollback

Application, runtime, container-image, model-weight, and tokenizer revisions
are separate. Updates are never installed automatically. The update page shows
the current and candidate revision, download and disk impact, compatibility,
and license changes.

Lumen Source retains the previous validated revision while validating the
candidate. A failed candidate is restored automatically. When a successful
update has a retained previous revision, **Roll back** restores its runtime,
weights or managed container, model settings, and validation metadata.

## Share an API

Localhost is the default. In **Settings > Share an API**, generate a token,
select the models to expose, and apply the configuration. **Allow other
devices** requires another explicit confirmation. The page displays the
reachable address and exact exposed models. Revoking the token stops the
gateway and restores loopback-only access.

The gateway authenticates requests but uses HTTP. It does not open a firewall
or configure a router. See [API sharing and security](sharing-and-security.md)
before using another network or a TLS reverse proxy.

## Backup, diagnostics, and recovery

**Export redacted diagnostics** creates a support document without credentials,
prompts, responses, user paths, hostnames, or raw addresses. **Back up settings
and inventory** creates a restorable document without credential-store secrets.
Restored sharing stays disabled until you explicitly generate or reuse a local
token and enable it.

Lumen Source saves state before schema migration and retains the previous valid
state after writes. A truncated primary state file falls back to that backup.
**Safe reset** preserves models, inventory, weights, caches, and credentials.
The separate **Reset and forget inventory** choice still does not delete model
files; rediscovery can adopt supported local Ollama models again.

## Privacy

Prompts and model responses remain out of lifecycle logs and diagnostic
exports. Optional aggregate telemetry is off until you opt in and never
contains inference content, files, arbitrary errors, or remote connection
details. Credentials live in the operating-system credential store.

## Uninstall

The OS package manager removes the application. Settings, model weights,
runtime caches, and credential-store entries are separate data classes. To
remove model data, use Models and Storage first so each destructive scope is
visible. To retain models for a later reinstall, uninstall without those
explicit cleanup actions. See [release packaging and data
retention](release-packaging.md) for the platform paths and release policy.
