# Lumen Source 1.0 acceptance campaign

Run every case from a clean snapshot. Record the package SHA-256, signature,
OS build, CPU, RAM, GPU/driver, runtime version, container engine, result,
evidence location, tester, and date. A failure is not waived by rerunning it;
link the defect and record the build containing the fix.

Start with `scripts/acceptance.ps1` on Windows or `scripts/acceptance.sh` on
Ubuntu. The scripts run the automated suite and create an evidence template.

## Windows 11 x86_64

- `WIN-PKG-001`: verify MSI/NSIS Authenticode signatures and package hashes.
- `WIN-INSTALL-001`: clean install, launch offline with the bundled catalog,
  detect hardware, and confirm the default model directory.
- `WIN-OLLAMA-001`: with Ollama absent, explicitly install it, install a
  Balanced chat model, validate inference, stop/start/configure it, update,
  roll back, and remove it.
- `WIN-OLLAMA-002`: adopt a separately installed Ollama model without another
  download; verify lifecycle ownership is explained.
- `WIN-STATE-001`: interrupt a download, restart, resume or safely restart it,
  and reconcile committed Ollama data.
- `WIN-UPGRADE-001`: upgrade the previous stable package; verify settings,
  inventory, credentials, retained models, offline startup, and catalog cache.
- `WIN-RECOVERY-001`: truncate `state.json`, start from the last valid backup,
  export redacted diagnostics, restore a user backup, and perform Safe reset.
- `WIN-UNINSTALL-001`: uninstall while retaining data, reinstall and rediscover.
- `WIN-UNINSTALL-002`: explicitly remove models/caches/credentials, uninstall,
  and verify the documented scopes are gone.

## Ubuntu 24.04 LTS x86_64

- `LINUX-PKG-001`: verify deb/AppImage structure and published SHA-256 values.
- `LINUX-INSTALL-001`: clean deb install and AppImage launch; repeat offline.
- `LINUX-OLLAMA-001`: complete the Windows Ollama lifecycle scenario.
- `LINUX-VLLM-001`: on NVIDIA/container GPU support, install and validate the
  pinned managed-vLLM catalog variant.
- `LINUX-VLLM-002`: restart/reconfigure it, install a second distinct container
  and port, remove servers independently, and retain/remove shared caches only
  after exact confirmation.
- `LINUX-UPGRADE-001`, `LINUX-RECOVERY-001`, `LINUX-UNINSTALL-001`, and
  `LINUX-UNINSTALL-002`: repeat the equivalent Windows data-safety cases.

## Connections, resources, and sharing

- `EXT-OLLAMA-001`: connect to an external Ollama service and verify Lumen
  Source never claims lifecycle or network control.
- `EXT-VLLM-001`: connect with and without a saved API key; reject bad
  credentials without exposing them.
- `REMOTE-OLLAMA-001`: complete the documented SSH Ollama workflow, lose the
  connection during use, reconnect, and verify inventory integrity.
- `RESOURCE-001`: start two models beyond the detected memory budget; verify
  the exact stop list, pin behavior, operation queue, and restart recovery.
- `SHARE-001`: prove sharing cannot start without a token or selected model.
- `SHARE-002`: localhost sharing, token rotation, revocation, and exact model
  list.
- `SHARE-003`: allow another LAN device after the warning, verify bearer-token
  rejection/acceptance, then disable sharing and confirm loopback restoration.
- `LOW-DISK-001` and `LOW-MEMORY-001`: verify preflight and start-time guided
  recovery without corrupt state.

## Accessibility and comprehension

At 100%, 150%, and 200% OS text scaling, complete Add model, Models, Machines,
Storage, Settings, update/rollback, recovery, and sharing:

- `A11Y-KEYBOARD-001`: keyboard only, visible focus, logical order, Escape,
  menus, and modal focus containment/return.
- `A11Y-SCREENREADER-001`: names, roles, values, headings, dynamic status/error
  announcements, and destructive-scope confirmations.
- `A11Y-CONTRAST-001`: text, controls, focus rings, status colors, disabled
  controls, charts, and high-contrast mode.
- `COMPREHENSION-001`: a non-expert installs and validates the recommended
  model without a terminal or runtime flags, then explains where to change its
  settings, free storage, and recover from a failure.

## Release decision

1. Every candidate support row has passing required cases.
2. No open critical/high data-loss, secret-exposure, install, update, rollback,
   removal, or accessibility defect remains.
3. Failures and unsupported combinations appear in Known limitations.
4. Windows signature status is recorded accurately; all published package
   hashes match. Unsigned Windows packages carry an explicit release warning.
5. The release commit, catalog revision, package hashes, and completed evidence
   reports are immutable and linked from the release record.
