# Release packaging and data retention

Windows releases produce both an MSI and an NSIS setup executable on the native
Windows runner. The release workflow always records each installer's
Authenticode status and SHA-256 checksum. Until an Authenticode identity is
configured, the workflow publishes clearly identified unsigned installers.
Windows displays an unknown-publisher warning for these files, and managed
security policy may prevent them from running.

Ubuntu 24.04 x86_64 releases produce a deb and AppImage in the pinned builder.
`scripts/package-release.sh` validates both formats and emits `SHA256SUMS`.

Pushing a version tag such as `v1.0.0` runs the GitHub Actions release workflow.
The tag must match the versions in `Cargo.toml`, `apps/desktop/package.json`, and
the Tauri configuration. Ubuntu runs the complete check suite before building
its native packages. Windows runs the complete check suite and builds both
installer formats on every release. The workflow creates a GitHub Release for
the tag and uploads the installers, signature-status reports, and SHA-256
checksum files. It can also be run manually for an existing tag from the
Actions page.

To build the unsigned installers locally from a Visual Studio Developer
PowerShell, run:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\container.ps1 package
```

Configure these GitHub Actions repository secrets before the first official
Windows release:

- `WINDOWS_SIGNING_PFX_BASE64`: the release-signing PFX encoded as base64;
- `WINDOWS_SIGNING_PFX_PASSWORD`: the password for that PFX.

After configuring and verifying the signing identity, set the Actions
repository variable `WINDOWS_SIGNING_ENABLED` to `true`. The workflow then
imports the certificate, uses `scripts/package-release.ps1`, and fails unless
both installers have valid Authenticode signatures. Leave the variable unset
to publish the same MSI and NSIS formats unsigned.

After updating every checked project version and committing the release, start
the pipeline by pushing its tag:

```sh
git tag v1.0.0
git push origin v1.0.0
```

Clean-install, upgrade, repair, and uninstall results for both installer formats
are recorded in the release acceptance checklist rather than inferred from a
successful build. An unsigned release must retain its warning in the GitHub
Release notes and its `SIGNATURES-windows-x86_64.txt` report.

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
