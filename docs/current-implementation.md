# Current implementation

This page describes the behavior currently implemented in the v0.1 desktop
application. Ubuntu 24.04 LTS on x86_64 is the release and acceptance-test
target.

## Guided installation wizard

The wizard implements this local flow:

1. Select deployment. Local deployment is available; remote deployment is
   visible but disabled.
2. Detect CPU, memory, storage, operating system, and supported GPU hardware.
3. Select a use case: chat, code, creative writing, or research.
4. Review the highest-ranked compatible variant or select any supported variant
   from the active catalog. The selected variant uses the same detail panel for
   its description, storage, context, runtime version, and ranking or
   incompatibility reasons. Hardware, free-storage, runtime, and source checks
   run immediately and appear under **Why this model**, including required and
   available storage.
5. Review the model license's commercial-use, redistribution, derivative,
   attribution, notice, obligation, restriction, usage-policy, and geographic
   terms. Custom terms require explicit acknowledgement. A user relying on a
   separate publisher agreement can record a local agreement reference and
   confirm that it authorizes the intended use; LumenSource does not validate
   that agreement.
6. Install the runtime when required and install/pull the model while reporting
   progress. An active download/install can be cancelled from the wizard.
7. Optionally start the model. **Start model after installation** is enabled by
   default and can be cleared before installation.
8. Show a Ready screen with the OpenAI-compatible base URL, chat-completions
   URL, model identifier, API-key requirement, and copy buttons.

When automatic start is disabled, installation still completes and the model
is added to the models page in a stopped state. The displayed endpoint values
are connection settings; they do not claim that a stopped model is already
loaded into memory.

Cancellation is cooperative across the desktop bridge and runtime adapter. It
interrupts an in-flight runtime HTTP request or streamed model pull and removes
temporary runtime archive/staging files. After cancellation the wizard remains
on the install step and can be retried. Ollama owns its model store, so any data
that Ollama committed before its HTTP stream was closed remains under Ollama's
control and is reconciled through `/api/tags` later.

The recommendation is not hard-coded to a model name. Compatible variants are
ranked by selected use case, variant profile, detected accelerator, RAM
headroom, storage headroom, and (when required) VRAM headroom. The bundled
catalog is `catalog/model-list.json`; every supported variant in that generated
artifact participates in compatibility filtering and ranking. Debug builds add
the fixture's dummy model; release builds do not.

The model picker contains every Ollama or dummy-runtime variant in the active
catalog, not just the top-ranked results. A variant that fails a hard hardware
or operating-system requirement remains visible with an **Incompatible** label
and the specific reasons; its preflight prevents installation.

## Hardware detection

The Linux hardware adapter reports:

- CPU model, architecture, logical and physical core counts, and frequency;
- installed and available system memory;
- best-effort memory generation such as DDR4, DDR5, or LPDDR5;
- best-effort effective memory speed in MT/s;
- storage capacity and available space for the configured mount (currently
  `/`);
- NVIDIA, AMD, Intel, or other DRM accelerators when detectable, including
  VRAM and driver data where the operating system or vendor tooling exposes it.

CPU frequency prefers
`/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq` and falls back to the
first `cpu MHz` value in `/proc/cpuinfo`. Memory generation and speed prefer
Linux EDAC sysfs and fall back to `dmidecode --type 17`. DMI access is commonly
restricted, and some firmware does not expose memory speed. Missing optional
fields are displayed as **Unavailable** and do not fail hardware detection.

DDR speed is shown in MT/s rather than MHz because it is an effective transfer
rate.

## Catalog behavior

The desktop embeds the schema-v2 `catalog/model-list.json` artifact so the local
flow works without catalog-service configuration. On refresh, a remote catalog is used
only when all of these environment variables are configured:

- `LUMEN_SOURCE_CATALOG_URL`
- `LUMEN_SOURCE_CATALOG_SIGNATURE_URL`
- `LUMEN_SOURCE_CATALOG_PUBLIC_KEY`

Remote catalogs are fetched over HTTPS, verified with an Ed25519 detached
signature over the exact catalog bytes, and cached as the last-known-good
catalog. The bundled catalog remains the fallback when remote catalog settings
are absent or unavailable during desktop startup.

## Ollama installation and lifecycle

The Ollama adapter uses the fixed loopback endpoint
`http://127.0.0.1:11434`. It does not build shell commands from catalog or user
input.

- Health and installed-model discovery use `GET /api/tags`.
- Loaded/running-model discovery uses `GET /api/ps`.
- Model installation uses `POST /api/pull` and streams progress.
- Starting a specific model uses `POST /api/generate` with `keep_alive: -1`.
- Stopping a specific model uses `POST /api/generate` with `keep_alive: 0`.

If no reachable Ollama service exists, Lumen Source first looks for an existing
Ollama executable. If one is unavailable, the current Linux adapter can install
a catalog-pinned `.tar.zst` archive after verifying its SHA-256 checksum. It
then launches the fixed `ollama serve` command without a shell.

An Ollama server launched by Lumen Source is deliberately independent from the
desktop process: closing Lumen Source does not terminate it. A later launch
reconnects through the loopback API. Lumen Source does not currently install a
login or boot service, so surviving a machine reboot depends on the system's
Ollama service configuration.

## Models page and discovery

At application startup, the desktop reconciles persisted UI metadata with
Ollama's runtime state:

- `/api/tags` is the source of truth for models downloaded into Ollama's local
  store, including models that are not running.
- `/api/ps` determines which models are currently loaded.
- Models matching an active catalog variant are shown as managed models and
  receive start/stop controls.
- Other Ollama models remain visible as discovered external models. Their
  start/stop button is disabled because the active catalog does not define the
  variant needed by the command interface.
- User-facing names and logs are persisted in the operating system's local
  application-data directory.
- Multiple installed entries are retained, including repeated installations
  of the same catalog model. Repeated entries reference the same underlying
  Ollama tag, so their running state changes together without duplicating the
  model bytes in Ollama's store.

Because installation discovery comes from Ollama rather than only from Lumen
Source's saved state, models in Ollama's store can be recovered after Lumen
Source is removed and reinstalled. Catalog matches become managed entries;
other recovered models are shown as external entries.

The model action menu is rendered at the document level so it remains above
all table rows and adapts its position near viewport edges. A click outside the
menu and its trigger, or pressing Escape, closes the open menu.

Each model row opens a full-body detail page. The model name replaces the
**Local models** heading, a **Back to model list** action returns to the list,
and Chat, Logs, Performance, and API are full-page tabs rather than modal dialogs.
Runtime start and stop requests display an immediate spinner and transition
label until the runtime responds; the stop control uses a larger square indicator.

**Chat** is a text-only local playground for a running, catalog-managed Ollama
model. Responses stream through an ordered Tauri channel and can be cancelled.
Conversation history remains in frontend memory for the current application
session; it is not written to persisted model state or lifecycle logs. Model
output is rendered as text rather than executable HTML.

**Logs** shows the selected entry's persisted Lumen Source lifecycle history,
including installation, start, stop, and rename activity recorded by the UI.
The history can be copied or cleared. It is not Ollama server stdout, because
Lumen Source can connect to an Ollama service that it did not launch. Prompts,
generated responses, and inference-client requests are intentionally not
recorded, so a running model does not continuously produce entries here.

**Performance** samples every two seconds while the model detail page is open,
including while its Logs tab is selected. A rolling graph keeps the latest 60
samples (approximately two minutes) and plots the selected model's total
resident memory, GPU VRAM allocation, and model allocation in system RAM. The
system-RAM value is the portion of the loaded model kept in ordinary RAM instead
of GPU VRAM; it is not CPU utilization.
Current-value cards below the graph also report context capacity and whether
the model is loaded. These values come from the matching entry in Ollama's
[`/api/ps` response](https://docs.ollama.com/api/ps), so a stopped model reports
zero allocation instead of host memory used by unrelated processes. Ollama
does not expose reliable per-model CPU/GPU processor utilization, request
throughput, tokens per second, or latency through this endpoint.

**API** shows model-specific OpenAI-compatible connection details: the `/v1`
base URL, chat-completions URL, exact runtime model identifier, local API-key
requirement, loaded state, and a copyable cURL request. The Ollama endpoint is
shared by every model; the request's `model` field selects the model. API details
are also available for discovered external Ollama models. The dummy model is
explicitly marked as having placeholder values and no inference API.

The main application body is edge-to-edge between the top bar and footer. A
persistent left rail reserves space for future navigation without wrapping the
content in an additional bordered application window.

## Dummy test runtime

In development builds, the legacy test fixture's **Dummy Test Model** is added
to the active catalog and uses a dedicated in-memory `dummy-runtime`. It is
excluded when a release is built.
It is included in recommendations when compatible and supports the same
install, start, status, and stop interface as a real runtime. It performs no
network download, writes no model artifact, and launches no server.

The dummy adapter remains alive across a frontend/webview reload, so a running
dummy model continues to appear as running after that reload. Its state is
process-local, so the running state resets only when the desktop backend process
exits. Endpoint values on its Ready screen are clearly identified as
placeholders and do not accept inference requests.

## OpenAI-compatible endpoint details

For Ollama, the Ready screen exposes:

- base URL: `http://127.0.0.1:11434/v1`;
- chat completions: `http://127.0.0.1:11434/v1/chat/completions`;
- the selected runtime model identifier;
- API key: not required by this local adapter.

These values allow an OpenAI-compatible client to target the local Ollama
service. Lumen Source currently displays and copies the values; it does not run
an inference request from the Ready screen. A successful clipboard write changes
the pressed button to **✓ Copied** for approximately two seconds. Clipboard
errors remain visible through the application error banner.

## Current boundaries

- The remote-host/agent deployment path is not implemented.
- Linux hardware probing is implemented; macOS and Windows probes remain
  planned adapters.
- Ollama does not expose per-model processor utilization or per-request
  throughput, token rate, and latency through `/api/ps`; Performance therefore
  reports per-model memory allocation and context capacity.
- Remove stops the selected model when necessary, deletes it from Ollama's local
  model store, and removes every Lumen Source entry referencing that runtime
  model. Failures leave the saved entries intact and are reported.
- Discovered external Ollama models are visible but are not controllable until
  they match a supported catalog variant.
- The dummy runtime validates application flow only and provides no inference.
