# Lumen Source desktop

Tauri 2 + React + TypeScript desktop operator for discovering, installing, and
running private local AI models.

## Development

Install the packages listed in `package.json`, then run:

```sh
npm run tauri dev
```

The Vite UI is intentionally a presentation layer. Typed calls live in
`src/commands.ts`; the corresponding Tauri command handlers live in
`src-tauri/src/commands.rs`. Page-sized views live in `src/components`, while
model lifecycle, chat, and performance state live in `src/hooks`. Shared
presentation helpers are kept in `src/modelUi.ts`.

`src-tauri/src/bridge.rs` is the adaptation seam for shared Rust APIs. The
serialized desktop types are defined in `src-tauri/src/bridge_types.rs`, and
installed-model normalization and reconciliation live in
`src-tauri/src/model_reconciliation.rs`. The small binary entry point delegates
application construction to the library crate.

User-facing copy is defined in `src/locales/en.ts` and accessed through
`src/i18n.ts`. Tauri progress, preflight, and remote-check responses use stable
message keys so the frontend—not the backend—chooses their localized wording.
Machine-specific diagnostics and catalog-authored metadata remain data rather
than translated interface copy.

The guided wizard uses the shared hardware, catalog, recommendation, host, and
runtime crates for the local flow: it detects hardware (including CPU frequency
and best-effort RAM generation/speed), ranks compatible catalog variants, lets
the user choose the recommendation or any supported catalog variant, shows
preflight checks directly in the recommendation detail, requires a separate
license review, reports installation progress, starts the model when requested,
and displays OpenAI-compatible endpoint details. Changing the model selection
updates one shared detail panel; incompatible catalog entries remain visible
with their rejection reasons.

The installation step includes a **Start model after installation** checkbox,
enabled by default. Clearing it installs the model without loading it; the
models page can start it later.

When no local Ollama installation is detected, the step also offers an
unchecked **Download and install Ollama** option. Consent is carried in the
installation request and enforced by the Rust backend. The verified standalone
runtime is installed under local application data and is not bundled into the
desktop package.

License and usage-policy actions open the HTTPS publisher URL from the signed
catalog in the operating system's default browser through Tauri's opener
plugin. LumenSource does not host a second copy of the license text.

The wizard identifies each model publisher using its plain-text name from the
catalog. It does not display or download company logos, imitate company brand
styles, or imply endorsement by model publishers.

The desktop fetches and signature-verifies the production catalog at startup.
A successful response becomes the last-known-good cached catalog. If the
network or verification fails, the desktop uses that verified cache; if no
verified cache exists, it falls back to the bundled `catalog/model-list.json`.
Development builds overlay a dummy model and its dedicated in-memory runtime
from the legacy test fixture for install and lifecycle coverage without
downloads. Release builds exclude it.

The footer identifies the active source as **Latest catalog**, **Cached
catalog**, or **Bundled catalog**, together with the revision and model count.
Green means the catalog was fetched and verified at startup, amber identifies a
last-known-good cached copy, and the neutral bundled state means no verified
network or cached catalog was available. Hovering the status shows the full
fallback explanation.

The footer also exposes the usage-statistics privacy choice. Telemetry is
disabled until the user explicitly chooses to share weekly aggregate model,
reliability, and coarse hardware statistics. Upload failure is non-fatal and
silent; queued reports are retried at the next startup or recorded occurrence.
See [usage telemetry](../../docs/telemetry.md) for the payload and ingestion
contract.

## Command surface

- Hardware detection
- Local and SSH-remote machine usage sampling
- Cached catalog load and network refresh
- Intent-based recommendations
- Installation preflight
- Installation with `install-progress` events
- Cooperative cancellation of an active download/install
- Runtime start, stop, and status
- Persisted lifecycle-log viewing, copying, and clearing
- Live model-state and per-model Ollama memory-allocation sampling
- OpenAI-compatible endpoint details
- Installed/running model reconciliation and persisted UI metadata

RAM generation is read from Linux EDAC sysfs when available, with DMI as a
fallback. RAM speed is reported in MT/s (the accurate unit for DDR transfer
rate). Firmware information is not exposed on every machine; unavailable
optional fields do not make hardware detection fail.

## Runtime lifecycle and discovery

The left navigation separates installed **Models** from configured
**Machines**. The Machines list always includes the local device and includes
each saved SSH target. Selecting a machine opens a details view with static
hardware information and a separate **Live usage** tab. While machine details
are open, LumenSource samples CPU, memory, and supported GPU usage every two
seconds and keeps a rolling two-minute graph. Remote samples are collected
through the configured SSH connection and rendered by the local desktop
client. SSH passwords can remain session-only or be saved in the operating
system credential store. They are never written to persisted target
configuration.

When Ollama is reachable, the local adapter discovers downloaded Ollama models
through `/api/tags` and loaded models through `/api/ps`. Catalog-matched models
are restored as managed entries with model-specific start/stop controls; other
Ollama models remain visible as discovered external entries with controls
disabled. Start and stop use Ollama's model API with `keep_alive: -1` and
`keep_alive: 0`, respectively.

When Lumen Source needs to launch `ollama serve`, the server is not terminated
when the desktop application closes. A later Lumen Source process reconnects to
the existing loopback endpoint. This keeps inference available independently of
the operator UI. The v0.1 adapter does not yet install an OS login/boot service,
so persistence across a machine reboot still depends on an existing Ollama
service installation.

The Ready screen presents Ollama's `/v1` base URL, the
`/v1/chat/completions` URL, the runtime model identifier, and local API-key
requirements. Copy buttons place individual values on the clipboard. For the
dummy runtime these values are explicitly placeholders. A successful copy is
acknowledged by a temporary **✓ Copied** label.

Model metadata is held in memory and flushed through a serialized persistence
path, preventing an older asynchronous save from replacing a newer multi-model
list. Reloading the frontend reconciles both Ollama and dummy runtime state; it
does not reset a running dummy model while the Rust backend is still alive.
Installing the same catalog model more than once creates separate list entries
that share the underlying runtime model and its start/stop state. Model action
menus close when clicking elsewhere or pressing Escape.

The application body fills the space between the top bar and footer, with a
persistent left navigation rail. Selecting a model row replaces the list with
a full-body model detail page. Its Chat tab provides a small, text-only local
playground with streaming responses, cancellation, and in-memory conversation
history; prompts and responses are not persisted or added to lifecycle logs.
Logs, Performance, and API remain separate full-page tabs. Start/stop actions
show an in-row spinner and transition label while the runtime request is pending.

The Logs view contains lifecycle activity recorded by Lumen Source rather than
Ollama server stdout, prompts, or generated responses. Performance refreshes
every two seconds and leads with a rolling two-minute graph of the selected
model's total resident memory, GPU VRAM, and model allocation in system RAM. The
system-RAM value is model memory not placed in GPU VRAM, not CPU utilization.
Current-value cards also show context capacity and loaded state. Ollama does not
expose reliable per-model processor utilization, request throughput, or token
latency through `/api/ps`.

The API tab shows the selected entry's OpenAI-compatible base URL,
chat-completions URL, exact runtime model identifier, authentication requirement,
loaded state, and a copyable cURL example. The endpoint is runtime-wide and the
model identifier selects the model for each request. Dummy endpoint values are
clearly marked as non-functional placeholders.
Current UI boundaries: the remote preview supports Linux and Windows OpenSSH
targets with an existing Ollama installation. It starts `ollama serve` as the
SSH user when needed and detects the target's CPU, RAM, storage, and supported
GPUs for recommendation and preflight, but does not install remote services.
The wizard selects from saved non-secret target
profiles; **Add target machine** opens the connection-settings dialog and
selects the new target after saving. SSH keys and agents are preferred; a
password can remain session-only or be saved securely for automatic reconnects
on Unix controllers; Windows controllers currently require key authentication.
Remove stops the selected model when necessary,
deletes it from the selected Ollama model store, and clears its Lumen Source entries. See
[current implementation](../../docs/current-implementation.md) for the complete
behavior and limitations.

Desktop and mobile packaging icons are generated from the repository's current
Lumen Source `icon.png`. The desktop process also applies the generated default
icon explicitly to its main window so Linux development sessions do not use a
generic fallback icon.
