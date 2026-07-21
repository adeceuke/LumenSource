# Lumen Source

Lumen Source is a desktop application that detects hardware, recommends compatible local AI models, installs the selected runtime and model, and exposes the runtime endpoint to external tools.

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

The remote Lumen Source Agent is intentionally deferred until the local v0.1 flow is proven. The host abstraction is the extension point for a later agent-backed host.

## v0.1 platform

Ubuntu 24.04 LTS x86_64 is the first release and acceptance-test target.
Domain and orchestration code remains platform-neutral so macOS and Windows
adapters can be added without changing product logic.

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

See `docs/development.md` for the complete dependency list and
`docs/platform-support.md` for platform boundaries.

## v0.1 milestone

Detect hardware → fetch and verify catalog → recommend model → ensure Ollama/model → start runtime → show endpoint.
