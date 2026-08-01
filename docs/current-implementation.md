# Current implementation

This page describes the desktop implementation leading into the 1.0 acceptance
campaign. Windows 11 x86_64 and Ubuntu 24.04 LTS x86_64 are the candidate local
targets; the exact boundary and evidence status are maintained in
[the support matrix](support-matrix.md).

The original sections below preserve detailed subsystem behavior. The current
cross-feature workflows—including consistent installation, Ollama and managed
vLLM settings, validation, storage ownership, interrupted-operation recovery,
safe updates and rollback, resource orchestration, authenticated API sharing,
diagnostic redaction, state backup/restore, and safe reset—are summarized in
the [user guide](user-guide.md) and tracked in the
[roadmap](roadmap-to-1.0.md).

## Guided installation wizard

The wizard implements local and Linux remote flows:

1. Select deployment. Local deployment is available. The Linux remote preview
   connects through OpenSSH to an existing target Ollama service.
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
headroom, storage headroom, and (when required) VRAM headroom. When a catalog
variant carries a reviewed external overall tier, higher tiers promote that
variant among compatible choices and remain visible as attributed,
informational evaluation metadata in the recommendation detail. The bundled
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

At startup, the desktop fetches the production catalog from
`https://lumensource.dev/v2/model-list.json` and its detached signature. It
accepts the catalog only after verifying the Ed25519 signature over the exact
response bytes with the public key compiled into LumenSource. Only a verified,
schema-compatible network catalog replaces the last-known-good cache.

The startup fallback order is:

1. The signature-verified production catalog fetched over HTTPS.
2. The last signature-verified catalog saved in the operating system's
   application-cache directory.
3. The schema-v2 `catalog/model-list.json` artifact bundled with the
   application.

The persistent footer makes the active source visible:

- **Latest catalog** (green) means the catalog was fetched and verified during
  this startup.
- **Cached catalog** (amber) means the online fetch failed and LumenSource is
  using the last verified copy saved on this device.
- **Bundled catalog** (neutral) means neither the network catalog nor a verified
  cache was available, so LumenSource is using its build-time catalog.

The footer also shows the catalog revision and model count. Hovering the status
explains the active fallback. The following environment variables remain
available as optional development overrides:

- `LUMEN_SOURCE_CATALOG_URL`
- `LUMEN_SOURCE_CATALOG_SIGNATURE_URL`
- `LUMEN_SOURCE_CATALOG_PUBLIC_KEY`
- `LUMEN_SOURCE_TELEMETRY_URL`

## Optional usage telemetry

The footer exposes an explicit usage-statistics choice. Until the user opts in,
no telemetry is collected. Weekly aggregate reports contain catalog delivery,
coarse hardware tiers, catalog model and variant identifiers, and install,
uninstall, start, and built-in chat outcome counts. Prompts, responses, files,
remote connection details, arbitrary strings, and raw errors are excluded.

Reports remain in a bounded 52-week local queue while offline. A successful
2xx server acknowledgement removes the submitted batch; timeouts, connection
failures, and non-2xx responses leave it intact for a silent retry at the next
startup or recorded occurrence. Telemetry never changes the result of a
catalog, model, runtime, or chat operation. The complete client payload and
server contract are documented in [usage telemetry](telemetry.md).

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
Ollama executable. If one is unavailable, the local install step offers an
explicit **Download and install Ollama** choice. When selected, Linux installs
a catalog-pinned `.tar.zst` archive and Windows installs a catalog-pinned
standalone ZIP after verifying its SHA-256 checksum. The runtime is stored in
the operating system's local application-data directory rather than bundled
with Lumen Source. It then launches the fixed `ollama serve` command without a
shell. Remote targets continue to require an existing Ollama installation.

An Ollama server launched by Lumen Source is deliberately independent from the
desktop process: closing Lumen Source does not terminate it. A later launch
reconnects through the loopback API. Lumen Source does not currently install a
login or boot service, so surviving a machine reboot depends on the system's
Ollama service configuration.

## Machines page and live usage

The application sidebar separates installed models from configured machines.
The local device is always present. Remote Linux machines use the same saved
SSH target configuration as the model-install wizard, including host, port,
username, SSH-agent or identity-file authentication, and optional password
authentication.

Selecting a machine opens its details view on the **Hardware** tab. Static
hardware detection covers CPU, memory, supported accelerators, storage, and
platform information. The adjacent **Live usage** tab graphs CPU, memory, and
supported GPU utilization. One snapshot is requested every two seconds while
the machine details view is open, with the latest 60 snapshots forming a
rolling two-minute window.

Local samples use Linux `/proc`, `/sys`, and optional GPU tools. Remote samples
run a fixed, non-user-generated probe through OpenSSH and return the normalized
data to the local desktop client. Remote hardware inspection does not require
Ollama to be installed. Passwords are never written to LumenSource state files.
They can remain in memory for the current session or be saved in the operating
system credential store and reused for later SSH reconnections.

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

- Linux/Windows remote deployment controls an existing target Ollama
  installation through an authenticated SSH tunnel and does not require a
  Lumen Source agent. It identifies Linux or Windows, finds Ollama through
  platform-specific paths, starts a stopped `ollama serve`, checks its loopback
  API, and collects normalized CPU, memory, storage, and supported GPU facts
  for recommendation and preflight. The
  deployment step uses an initially empty selector backed by saved non-secret
  target profiles; adding a target happens in a dialog and selects the saved
  target. SSH keys/agents are preferred; a password can remain session-only or
  be saved in the operating system credential store for automatic reconnects
  on Linux and Windows controllers. The password is passed to OpenSSH through
  a private Unix socket or one-time local Windows named pipe.
  Remote runtime installation is not implemented. See
  [`remote-hosts.md`](remote-hosts.md).
- Linux and Windows hardware probing are implemented; macOS remains a planned
  adapter.
- Ollama does not expose per-model processor utilization or per-request
  throughput, token rate, and latency through `/api/ps`; Performance therefore
  reports per-model memory allocation and context capacity.
- Remove stops the selected model when necessary, deletes it from Ollama's local
  model store, and removes every Lumen Source entry referencing that runtime
  model. Failures leave the saved entries intact and are reported.
- Discovered external Ollama models are visible but are not controllable until
  they match a supported catalog variant.
- The dummy runtime validates application flow only and provides no inference.
