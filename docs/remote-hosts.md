# Remote host deployment and control

## Status and objective

The Linux/Windows remote host slice is implemented. The installation
wizard loads saved SSH target profiles. Its target selector is empty when a new
wizard starts; **Add target machine** opens a dialog for the non-secret
connection settings, and saving returns to the deployment step with that target
selected. Lumen Source identifies a Linux or Windows target, finds Ollama in
platform-specific command and installation paths, starts `ollama serve` when
its loopback API is not already running, and collects normalized CPU, memory,
storage, and supported GPU facts before maintaining an SSH tunnel for model
operations. Remote runtime installation remains deferred.

The objective is to let Lumen Source discover, recommend, deploy, start, stop,
remove, monitor, and use models on another machine without requiring a Lumen
Source agent on that machine. Linux, macOS, and Windows must be supported as
both the controller running Lumen Source and the target running Ollama.

The feature is not cross-platform complete until supported controller/target
combinations have their own acceptance runs. Implementations may ship
incrementally, but domain types and persisted state must not assume Linux.

## Agentless boundary

The implemented preview attaches to an existing Ollama installation on the
target. If its API is stopped, Lumen Source launches a detached `ollama serve`
process as the SSH user. Lumen Source installs no executable, service, or agent
in this mode. It uses the target's existing remote-management service to create
an encrypted tunnel to Ollama's loopback API.

The target must already have:

- a supported remote-management service enabled;
- Ollama installed and executable by the target user;
- the drivers required for its accelerator; and
- a target user authorized to use Ollama and its model store.

Lumen Source uses the controller's OpenSSH client. Existing SSH
agent/configuration or an optional identity-file path is the preferred
authentication method. Password authentication is available as a
non-preferred fallback and can use the controller operating system's credential
store. On first connection, OpenSSH silently records a new host key in the controller
user's `known_hosts` file. A changed key remains a hard failure and requires
the user to verify the target identity outside Lumen Source before replacing
the saved key.

The saved profile contains the optional target name, host, port, username,
authentication mode, and optional identity-file path. It contains no password;
saved passwords are separate credential-store entries keyed by target identity.
The optional target name is used as the model-list location label. When it is
blank, the configured hostname or IP address is used. Key authentication must
work without a terminal prompt; encrypted keys must be unlocked in an
accessible SSH agent. For a password-authenticated profile, the wizard asks for
a password after selection and lets the user keep it transient or save it in
the operating system credential store. It is supplied to OpenSSH through a
mode-`0600`, one-time Unix socket and helper process and is never placed in
process arguments or environment values.

Model pulls, lifecycle requests, Chat, performance sampling, deletion, and API
details use the tunneled remote Ollama client. Saved models retain their target
identity. Key-authenticated targets reconnect on demand after restart.
Password-authenticated targets also reconnect on demand when their credential
is saved; otherwise the model action asks for the password.

It is impossible to run a local model on a machine with no inference runtime or
model data. If Ollama is absent, a later explicit agentless bootstrap may place
the Ollama runtime and model files on the target. That is still target
installation, even though it does not install a Lumen Source agent.

## Cross-platform validity

The same architecture works on all three operating systems:

```text
Lumen Source desktop
  -> target profile and authenticated remote session
  -> controller 127.0.0.1:<allocated-port>
  -> encrypted tunnel
  -> target 127.0.0.1:11434
  -> Ollama API
```

SSH can be the preferred transport everywhere:

- Linux commonly provides OpenSSH through the distribution.
- macOS provides SSH and SFTP through
  [Remote Login](https://support.apple.com/guide/mac-help/allow-a-remote-computer-to-access-your-mac-mchlp1066/mac).
- Windows 10, Windows 11, and supported Windows Server releases provide
  [OpenSSH Server](https://learn.microsoft.com/windows-server/administration/openssh/openssh_install_firstuse),
  but it is optional or disabled on many installations.

Windows does not invalidate the design, but zero target setup cannot be
guaranteed: the user may need to enable or install the Windows OpenSSH Server
feature. A later Windows transport may support WinRM/PowerShell Remoting when
SSH is unavailable.

Changing only the remote connection tool is not sufficient. The tunnel, Ollama
API, catalog rules, normalized hardware facts, and product flow are shared, but
these target concerns require OS-specific adapters:

| Concern | Linux | macOS | Windows |
| --- | --- | --- | --- |
| Preferred transport | OpenSSH | Remote Login/OpenSSH | OpenSSH; later WinRM if needed |
| Hardware facts | `/proc`, sysfs, DMI, vendor tools | `sysctl`, `system_profiler`, `vm_stat`, Metal | PowerShell/CIM, DXGI, vendor tools |
| Storage and paths | POSIX mounts and paths | POSIX paths and macOS application data | Drive letters and Windows application data |
| Runtime package | `.tar.zst` and executable | `.app`/CLI and architecture-specific files | Installer or standalone ZIP and GPU libraries |
| Durable lifecycle | systemd or user service | launchd or Ollama login item | Windows service, task, or background app |
| Acceleration | CUDA, ROCm, CPU | Metal on Apple silicon, CPU where supported | CUDA, ROCm, CPU |

Ollama is available on
[macOS, Windows, and Linux](https://docs.ollama.com/quickstart), but its hardware
support and packaging differ. Bootstrap must select artifacts using the target
OS and architecture, never the controller OS.

## Network and security model

Ollama binds to `127.0.0.1:11434` by default and its local HTTP API does not
require authentication. Lumen Source must not expose unauthenticated port 11434
to a LAN or the internet. See Ollama's
[networking guidance](https://docs.ollama.com/faq) and
[authentication behavior](https://docs.ollama.com/api/authentication).

Required controls:

- Bind the controller side of the tunnel to loopback only.
- Use OpenSSH trust-on-first-use to persist a previously unseen target host key
  without interrupting the setup flow.
- Hard-fail a changed host key until it is verified and re-authorized outside
  Lumen Source.
- Prefer existing SSH configuration, an SSH agent, or a selected key; never
  enable SSH agent forwarding.
- Do not store passwords, private-key contents, or passphrases in JSON state.
- Keep passwords out of arguments, environment values, JSON state, and logs.
- Store remembered passwords only in the operating system credential store and
  provide an explicit forget action.
- Do not log credentials, prompts, responses, or private-key paths.
- Validate usernames, hosts, ports, remote paths, and model references.
- Keep fixed remote probe commands in OS-specific adapters and never
  interpolate untrusted values into a shell command.
- Use SFTP or an equivalent typed channel for bootstrap transfer.
- Confirm the target name before remote deletion or runtime installation.

An advanced direct mode may later accept an administrator-provided HTTPS
endpoint protected by a trusted proxy or VPN. Plain remote HTTP is unsupported.

## Architecture changes

### Target identity and persistence

Add shared target types:

```rust
TargetId
TargetKind::Local | TargetKind::Ssh | TargetKind::DirectHttps
TargetProfile
TargetSession
```

Profiles store non-secret connection metadata, trusted host identity, and the
detected target OS and architecture. Secrets are referenced through OS
credential storage.

Every model contains a `target_id`. Model identity is
`(target_id, runtime, runtime_model_id)`, because separate hosts may contain the
same Ollama tag. Existing state migrates to a reserved local target. The current
free-form `location: "local"` becomes derived display data rather than identity.

### Separate API access from supervision

Split the current Ollama adapter into:

- `OllamaApiClient`: health, inventory, pull, delete, lifecycle, Chat, endpoint
  details, and allocation data for any configured base URL;
- `LocalOllamaSupervisor`: local executable discovery, installation, process
  launch, and service integration; and
- `RemoteTargetSession`: tunnel health, remote collection, and optional
  bootstrap.

A remote failure must never fall back to controlling or launching the
controller's local Ollama.

### Target-aware orchestration

Replace the desktop backend's single local probe, runtime client, and selected
runtime with a target registry and sessions keyed by `TargetId`. Every
model-affecting command carries a target ID. Reconciliation, cancellation,
running state, endpoint details, performance sampling, and deletion query the
model's target rather than global runtime state.

### Hardware collection

Separate collection from parsing:

```text
local OS collector ----\
                       -> normalized HardwareFacts -> recommendation
remote OS collector ---/
```

Linux, macOS, and Windows get sibling collectors. Storage checks inspect the
filesystem containing the target Ollama model store, not the controller disk.
Missing optional facts remain unavailable rather than failing connection.

## User flow

1. Select **Remote runtime**.
2. Choose a target from the initially empty selector, or select **Add target
   machine**.
3. In the dialog, enter connection metadata, choose authentication, and select
   **Save**. The new target is selected automatically.
4. Enter the password when required, choose whether to save it securely, then
   test the connection. OpenSSH records an unseen host identity automatically
   and rejects a changed identity.
5. Detect target OS, architecture, hardware, storage, and Ollama status.
6. Continue without installation when Ollama is already available.
7. Recommend using target hardware.
8. Review the model license as in the local flow.
9. Pull the model through the target Ollama API with streamed progress.
10. Optionally preload it.
11. Show the target name, tunneled endpoint, and connection lifetime.

The Models page groups or filters by target. Offline targets remain visible
with reconnect controls. Model actions, Chat, Performance, and API are disabled
while disconnected and never silently run locally.

The tunneled API exists only while Lumen Source maintains the connection.
Closing Lumen Source closes the tunnel but does not stop independent target
Ollama. Persistent client access requires a separately configured VPN or
authenticated HTTPS proxy.

## Delivery phases

### 1. Target-aware refactor

- Add target identity, profiles, sessions, and state migration.
- Split API access from local process supervision.
- Make commands and reconciliation target-aware.
- Preserve all local behavior before enabling remote UI.

### 2. Agentless connection

- Implement authenticated SSH and loopback forwarding.
- Support existing SSH configuration, agents, and selected keys.
- Add silent trust-on-first-use and changed-key rejection.
- Add keepalive, disconnect, and reconnect behavior.
- Enable saved remote targets in the wizard.

The first implementation may launch the controller's OpenSSH client as a fixed
subprocess without a shell, preserving SSH configuration, jump hosts,
hardware-backed keys, and agents. Validate this on all controller OSes. If
availability or GUI authentication is inconsistent, use an embedded
cross-platform SSH client behind the same transport interface.

### 3. Remote discovery and control

- Connect `OllamaApiClient` through the tunnel.
- Discover installed and loaded models.
- Pull, start, stop, remove, chat, and sample allocation remotely.
- Reuse capability-aware generate, embedding, completion, and chat behavior.
- Expose tunnel-bound OpenAI-compatible endpoints.
- Reconcile after restart and network interruption.

### 4. Cross-platform remote probing

- Keep the implemented normalized Linux collector aligned with local hardware
  facts, and add equivalent macOS and Windows collectors.
- Continue recommending and running preflight using target hardware and
  storage.
- Add OS-specific fixtures and parser tests.
- Run supported controller/target acceptance combinations.

### 5. Optional runtime bootstrap

Keep bootstrap outside the first remote release. When added, it is explicit and
separate from attaching to an existing runtime. Select a catalog-pinned target
artifact, transfer it through a typed channel, verify it, and use a user-owned
location where possible. Persistence is separate consent because systemd,
launchd, and Windows lifecycle configuration modify targets differently.

A Docker or Podman adapter may support targets that already have a container
runtime, but pulling an image remains target deployment, not zero installation.

### 6. Optional managed agent

An agent is unnecessary for single-operator control. Consider one later for
fleet policy, durable offline operations, push events, centralized audit,
multi-user authorization, or targets without acceptable native management.

## Test and release gates

Unit tests cover target validation and migration, composite identities,
capability routing, command-injection resistance, host-key changes, redaction,
and proof that remote failures cannot invoke the local supervisor.

Integration tests use an isolated SSH server and fake Ollama API to cover the
tunnel, inventory, pull progress and cancellation, lifecycle, deletion, Chat,
performance, reconnect, and duplicate model references on different targets.

Each target OS receives clean-machine acceptance for authentication, identity,
hardware and storage detection, existing loopback-only Ollama, recommendation,
pull, lifecycle, Chat, removal, tunnel closure, reconnect, and failures such as
network loss, changed identity, insufficient storage, or incompatible hardware.

Controller packaging also validates connection and credential behavior from
Linux, macOS, and Windows. A combination is not advertised merely because its
domain types compile.

## Current Linux/Windows preview boundary

The preview requires an existing Ollama installation and SSH service on the
target, installs no Lumen Source software remotely, keeps Ollama on target
loopback, and performs model operations through the tunnel. It starts a stopped
Ollama server as the SSH user when necessary. It stores non-secret target
metadata and an optional identity-file path, but no passwords, passphrases, or
key contents. Passwords can be transient or separate operating-system
credential-store entries. Closing Lumen Source closes the tunnel without
stopping target Ollama.

Windows targets require OpenSSH Server and Windows PowerShell. Windows
controllers currently support key/agent authentication; the private
password-broker transport is Unix-only.

Remote model ranking and preflight use the target's detected RAM, available
storage, operating system, and supported GPU/VRAM facts. Missing optional memory
generation or speed fields remain **Unavailable** and do not block deployment.

Cross-platform completion requires equivalent acceptance on Linux, macOS, and
Windows targets and controllers; validating Linux alone is insufficient.
