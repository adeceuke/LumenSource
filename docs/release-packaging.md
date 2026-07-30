# Release packaging and data retention

Stable Windows releases require an Authenticode identity. The release workflow
imports the protected PFX, builds MSI and NSIS artifacts, and fails unless every
installer reports a valid signature. Local release operators use
`scripts/package-release.ps1`; an unsigned package is a development artifact,
not a stable release.

Ubuntu 24.04 x86_64 releases produce a deb and AppImage in the pinned builder.
`scripts/package-release.sh` validates both formats and emits `SHA256SUMS`.
Clean-install, upgrade, repair, and uninstall results are recorded in the
release acceptance checklist rather than inferred from a successful build.

Lumen Source keeps these data classes separate:

- Application executables are owned by the OS package.
- Settings and the recoverable inventory live in the per-user application-data
  directory.
- Ollama weights remain in the configured Ollama model store.
- Hugging Face weights and vLLM compile data remain in their named caches or
  managed container volumes.
- API keys, sharing tokens, and remote passwords remain in the operating-system
  credential store.

Uninstalling the application does not imply deletion of model weights or shared
caches. Models and Storage provide explicit removal actions with scope
confirmation. **Safe reset** preserves installed data and credentials; the
separate **Reset and forget inventory** action still leaves model files and
caches intact.

Before a settings-schema migration, Lumen Source saves
`state.pre-migration.json`. Each subsequent atomic state write preserves the
last valid `state.backup.json`. If the primary file is truncated or corrupt at
startup, the application loads the valid backup. User-created state backups
never include credential-store secrets.
