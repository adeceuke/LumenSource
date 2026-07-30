# Documentation

The Markdown documentation describes the current implementation:

- [`current-implementation.md`](current-implementation.md) — implemented flows,
  runtime semantics, and known boundaries
- [`development.md`](development.md) — Ubuntu 24.04 toolchains, packages, tests,
  and dummy-runtime testing
- [`platform-support.md`](platform-support.md) — v0.1 target and cross-platform
  boundaries
- [`remote-hosts.md`](remote-hosts.md) — implemented Linux remote preview plus
  cross-platform architecture, security, delivery phases, and acceptance gates
- [`telemetry.md`](telemetry.md) — optional aggregate usage data, offline retry
  semantics, and the server ingestion contract
- [`runtime-settings-plan.md`](runtime-settings-plan.md) — three-step tracked
  delivery plan for settings, configurable Ollama, vLLM, and model overrides
- [`roadmap-to-1.0.md`](roadmap-to-1.0.md) — planned 0.6 through 1.0 releases,
  with one consistent model-installation workflow and release acceptance gates
- [`user-guide.md`](user-guide.md) — installation, model management, storage,
  updates, sharing, recovery, privacy, and uninstallation
- [`support-matrix.md`](support-matrix.md) — exact 1.0 candidate boundaries
- [`known-limitations.md`](known-limitations.md) — unsupported and best-effort
  combinations
- [`acceptance-1.0.md`](acceptance-1.0.md) — repeatable clean-machine campaign
- [`release-checklist.md`](release-checklist.md) — final release decision gates
- [`sharing-and-security.md`](sharing-and-security.md) — authenticated gateway,
  firewall boundary, and TLS deployment
- [`release-packaging.md`](release-packaging.md) — signed/verified packages and
  data retention

The product and architecture source documents are stored at the repository
root:

- `Lumen Source - Product Requirements Document.docx`
- `Lumen Source - Engineering Guidelines.docx`
- `Lumen Source - Brand & Design Decisions.docx`
- `Lumen Source - v0.1 Technical Spike.docx`

The Word documents capture product requirements and design intent. When they
and the code differ, `current-implementation.md` records the behavior present
in this repository. Future architecture decisions should be added here in a
text-based format so they can be reviewed directly in Git.
