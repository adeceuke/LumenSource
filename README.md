# Lumen Source

Lumen Source is a desktop application that detects hardware, recommends
compatible local AI models, installs the selected runtime and model, and
exposes OpenAI-compatible connection details to external tools.

Use `lumen-source` wherever the product name must be represented as one technical token.

## Repository layout

- `apps/desktop/` — Tauri 2 desktop shell and React/TypeScript UI
- `crates/lumen-source-core/` — application orchestration and use cases
- `crates/lumen-source-catalog/` — catalog schema, fetch, verification, and cache
- `crates/lumen-source-hardware/` — hardware facts and usage sampling
- `crates/lumen-source-runtime/` — runtime abstraction and implementations
- `crates/lumen-source-host/` — host abstraction and local host implementation
- `crates/lumen-source-recommend/` — compatibility filtering and recommendation scoring
- `catalog/` — catalog schema notes and test fixtures
- `docs/` — documentation index

Remote deployment has an agentless SSH preview for Linux and Windows targets
with an existing Ollama installation. A managed Lumen Source Agent, remote
macOS adapters, and remote runtime installation remain deferred. See the
[remote host plan](docs/remote-hosts.md).

## Platforms

Ubuntu 24.04 LTS x86_64 remains the reference release target. Windows x86_64
has a native Tauri build, hardware detection, and local Ollama model
installation workflow. See [platform support](docs/platform-support.md) for
the acceptance status and remaining limitations.

## Development setup

Build and validate in the reproducible Ubuntu 24.04 container:

```shell
scripts/container.sh check
scripts/container.sh package
```

For a native Ubuntu 24.04 toolchain:

```shell
scripts/setup-ubuntu-24.04.sh
scripts/check-prerequisites.sh
```

On Windows, use a Visual Studio Developer PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\check-prerequisites.ps1
powershell -ExecutionPolicy Bypass -File scripts\container.ps1 check
powershell -ExecutionPolicy Bypass -File scripts\container.ps1 package
```

Docker is not required for a Windows package. The native MSVC toolchain is
required because the Ubuntu container cannot produce a Windows Tauri
MSI/NSIS installer.

See [development setup](docs/development.md), [platform support](docs/platform-support.md),
[remote host planning](docs/remote-hosts.md), and
[current implementation](docs/current-implementation.md) for details. The
[roadmap to 1.0](docs/roadmap-to-1.0.md) tracks the planned stabilization and
beginner-focused management releases. Release operators should also read
[API sharing and security](docs/sharing-and-security.md) and
[release packaging and data retention](docs/release-packaging.md). The
[1.0 user guide](docs/user-guide.md), [support matrix](docs/support-matrix.md),
and [acceptance campaign](docs/acceptance-1.0.md) define the stabilization
boundary.

## Current local workflow

Choose local deployment → detect hardware → load the catalog (and authenticate
remote updates) → use the recommended model or choose another catalog model →
run preflight checks → install the runtime/model → optionally start it → show
endpoint details.

The models page also reconciles Ollama's installed-model inventory (`/api/tags`)
with its currently loaded models (`/api/ps`), and provides catalog-backed
start/stop controls. The bundled model catalog is generated from
`catalog/model-list.json`; development builds add a no-download dummy
model/runtime from the test fixture. Release builds exclude it.
