# Lumen Source

Lumen Source is a desktop application for deploying, running, and maintaining
open-weight AI models on hardware you control.

It aims to make local model hosting approachable without hiding the operational
details that matter. Lumen Source detects the machine's hardware, recommends
compatible models and settings, installs or connects to a runtime, manages the
model lifecycle, and provides OpenAI-compatible connection details for other
applications.

The project currently focuses on:

- choosing models that fit the available CPU, RAM, storage, and GPU resources;
- installing and managing local models through Ollama;
- managing vLLM deployments on supported Linux/NVIDIA systems and connecting to
  existing vLLM services;
- starting, stopping, updating, validating, and removing models;
- monitoring model state, resource use, logs, and storage;
- exposing a local OpenAI-compatible API for tools that already support that
  protocol; and
- operating supported remote machines over SSH without installing a Lumen
  Source agent on them.

Lumen Source is not a model runtime itself. It provides a desktop management
layer around runtimes such as Ollama and vLLM, while keeping model execution and
data on infrastructure controlled by the user.

## Website

Find more information on our website:
[Lumen Source](https://lumensource.app)

Questions, feedback, or ideas? Contact us at
[contact@lumensource.de](mailto:contact@lumensource.de).

## Platform availability

| Platform | Current status |
| --- | --- |
| Ubuntu 24.04 LTS, x86_64 | Supported development and packaging target (`.deb` and AppImage) |
| Windows 10/11, x86_64 | Supported native development and packaging target (MSI and NSIS) |
| macOS | **Not currently available** |

macOS is planned, but not yet supported. If you have access to Mac hardware,
feel free to contribute to the project! :)

For detailed capability boundaries, see the [platform support](docs/platform-support.md),
[runtime support](docs/runtime-support.md), and [known limitations](docs/known-limitations.md)
documents.

## Build from source

The repository contains a Tauri 2 desktop application with a React/TypeScript
frontend and a Rust workspace. Run all commands below from the repository root
unless a step says otherwise.

### Ubuntu 24.04 (recommended container build)

Prerequisites:

- Docker, or another Docker-compatible container engine;
- permission to access its daemon; and
- an x86_64 Ubuntu or Linux host.

Validate the project and build installable packages:

```sh
scripts/container.sh check
scripts/container.sh package
```

The package command writes the `.deb` and AppImage artifacts under
`target/release/bundle/`.

For a fully native Ubuntu toolchain instead of the container, install the
checked-in prerequisites and then run the desktop in development mode:

```sh
scripts/setup-ubuntu-24.04.sh
scripts/check-prerequisites.sh
cd apps/desktop
npm ci
npm run tauri dev
```

### Windows 10/11

Install:

- Visual Studio 2022 Build Tools with **Desktop development with C++**;
- Rust through `rustup` (the repository pins the required toolchain);
- the Node.js version specified in `.node-version`; and
- Microsoft Edge WebView2 Runtime, which is normally present on current Windows
  installations.

Open a Visual Studio Developer PowerShell in the repository root, then validate
and package the application:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\check-prerequisites.ps1
powershell -ExecutionPolicy Bypass -File scripts\container.ps1 check
powershell -ExecutionPolicy Bypass -File scripts\container.ps1 package
```

Despite its name, `container.ps1` performs a native Windows build; Docker is not
required. MSI and NSIS artifacts are written under `target\release\bundle\`.

To run the desktop directly during development:

```powershell
Set-Location apps\desktop
npm ci
npm run tauri dev
```

See [development setup](docs/development.md) for native Linux dependencies,
narrower test commands, release packaging, and acceptance checks.

## Basic usage

1. Install a package produced for Ubuntu or Windows and open Lumen Source.
2. Select **Add model** and choose the local machine or a configured supported
   remote machine.
3. Let Lumen Source inspect the target hardware, then choose a recommended model
   or another compatible catalog entry.
4. Review the expected resource use, model license, usage policy, and runtime
   settings.
5. Run the preflight checks and start the installation. Lumen Source can use an
   existing Ollama installation or, with explicit approval, install its managed
   standalone Ollama runtime for a local deployment.
6. Start the model and wait for validation to complete.
7. Open the model's **Chat** tab for a quick local test, or use the **API** tab
   to copy its OpenAI-compatible base URL, model identifier, and example request
   into another application.
8. Use **Models**, **Machines**, and **Storage** to monitor resources, change
   settings, stop or restart models, manage updates, and remove data explicitly.

Ollama is the simplest path for a first local model. Managed vLLM has additional
Linux, NVIDIA, and container-runtime prerequisites; see
[runtime support](docs/runtime-support.md) before using it.

The complete workflow, including API sharing, backup, privacy, updates, and
uninstallation, is documented in the [user guide](docs/user-guide.md).

## Repository layout

- `apps/desktop/` — Tauri desktop shell and React/TypeScript interface
- `crates/lumen-source-core/` — application orchestration and use cases
- `crates/lumen-source-catalog/` — model catalog loading and verification
- `crates/lumen-source-hardware/` — hardware detection and usage sampling
- `crates/lumen-source-runtime/` — runtime adapters and lifecycle management
- `crates/lumen-source-host/` — local and remote host abstractions
- `crates/lumen-source-recommend/` — compatibility filtering and recommendations
- `catalog/` — model catalog data and fixtures
- `docs/` — user, development, platform, security, and release documentation

Development builds include a no-download dummy model/runtime for testing the
workflow. Release builds exclude it.
