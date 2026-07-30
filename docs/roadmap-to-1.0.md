# Roadmap from 0.5 to 1.0

This document tracks the product work proposed between Lumen Source 0.5 and
1.0. The goal is not to accumulate runtime features. The goal is to make local
AI model installation, deployment, and management dependable for people who
do not already understand model formats, quantization, context allocation,
runtime processes, or GPU infrastructure.

## Product rules for every release

- There is no first-run-only wizard or onboarding installation path.
- **Add model** always opens the same installation assistant, for the first
  model and every later model.
- Previously saved defaults may preselect values, but they must not skip safety,
  license, compatibility, or validation gates.
- Ollama remains the default runtime for new installations.
- Existing models never change runtime or revision silently.
- Beginner-facing choices use outcome language such as **Safe**, **Balanced**,
  and **Fast**. Runtime-specific terminology remains available under advanced
  or custom settings.
- A successful installation means that Lumen Source has made a real test
  request, not merely detected a process or downloaded files.
- Destructive actions state whether they remove a server, model weights,
  shared caches, settings, or credentials.
- Runtime, model, and application updates require an explicit user action and
  preserve the last working configuration until the replacement is validated.
- Managed services remain loopback-only unless the user deliberately enables a
  secured sharing workflow.
- Offline operation uses the verified cached catalog and already installed
  runtimes and models wherever possible.

## 0.6: Consistent installation and automatic safe configuration

Goal: make every model installation understandable and finish with a model
that has actually answered a validation request.

### Reusable installation assistant

- [x] Use one **Add model** workflow regardless of how many models are already
      installed.
- [x] Preserve the same steps and safety checks on every invocation.
- [x] Preselect saved target, runtime, use case, and performance profile without
      hiding or bypassing them.
- [x] Let the user change those selections for each installation.
- [ ] Make the Back, Cancel, retry, and interrupted-install behavior consistent
      at every step.
- [x] Return to the model library or open the installed model after completion,
      using an explicit user choice rather than first-run behavior.

### Beginner performance profiles

- [x] Add **Safe**, **Balanced**, **Fast**, and **Custom** profiles.
- [x] Make **Balanced** the default.
- [x] Derive context length, concurrency, GPU-memory allocation, offloading,
      and preferred accelerator from detected hardware and catalog limits.
- [x] Show a short outcome-oriented explanation for each profile.
- [x] Keep calculated runtime values visible in the model Settings tab.
- [x] Allow **Custom** to use the detailed Ollama and vLLM controls introduced
      in 0.5.
- [ ] Warn when a custom value exceeds the detected memory budget.
- [x] Store the selected profile with the model so its effective configuration
      can be explained later.

### Post-install validation

- [x] Send a small deterministic chat, completion, or embedding request suited
      to the model's declared capabilities.
- [x] Verify runtime health, served model identity, endpoint behavior, and
      authentication.
- [x] Confirm that the configured context and accelerator settings were
      accepted where the runtime exposes that information.
- [x] Report whether the model is running on CPU, GPU, or a mixed allocation
      without treating unavailable metrics as failure.
- [x] Redact the validation prompt and response from lifecycle logs and
      telemetry.
- [x] Offer **Retry**, **Use safer settings**, **Open diagnostics**, and
      **Remove incomplete installation** when validation fails.
- [x] Mark a model Ready only after validation succeeds.

### Catalog readiness

- [ ] Publish real equivalent Ollama and vLLM variants for a small,
      acceptance-tested model set.
- [x] Pin model and tokenizer revisions for managed vLLM entries.
- [x] Identify gated models before installation and request a Hugging Face
      token only when required.
- [x] Add plain-language labels such as **Beginner friendly**, **Fast**,
      **High quality**, **Reasoning**, **Embeddings**, and **Vision**.
- [x] Include an estimated loaded-memory range in addition to download size.
- [x] Record which hardware configurations have passed an actual inference
      validation for each supported runtime family.

### 0.6 acceptance criteria

- [x] The first and tenth model use the same installation workflow.
- [x] A user can install a recommended model without understanding runtime
      engine settings.
- [x] Every successful installation ends with a verified inference or embedding
      response.
- [x] A failed validation provides at least one safe recovery action.
- [x] Safe and Balanced configurations do not knowingly exceed detected memory.
- [ ] Windows Ollama and Linux Ollama pass clean-machine installation tests.
- [ ] At least one managed-vLLM catalog variant passes the Linux NVIDIA flow.

### Remaining 0.6 release evidence

The implementation is complete and passes the automated Windows check suite.
The unchecked clean-machine and managed-vLLM acceptance items require physical
runtime environments and remain release evidence rather than unfinished
application code:

- Run an Ollama chat or embedding installation on clean Windows and Linux
  machines, covering validation success and safer-profile recovery.
- Run the pinned Qwen 2.5 0.5B vLLM variant on Linux with an NVIDIA GPU and
  container GPU support, then record the persisted validation report.
- Exercise cancellation at each assistant stage and restart during a download;
  restart recovery itself is intentionally tracked under 0.7 interrupted
  operations.

## 0.7: Inventory, storage, import, and interrupted-operation recovery

Goal: make it obvious what is installed, where disk space is used, and how to
recover when files or processes change outside Lumen Source.

### Storage manager

- [ ] Add a Storage page showing:
  - [ ] Installed model weights by runtime and model
  - [ ] Ollama model storage
  - [ ] Hugging Face weight cache
  - [ ] vLLM compile cache
  - [ ] Lumen Source runtime downloads
  - [ ] Temporary and incomplete downloads
- [ ] Distinguish exact values from estimates.
- [ ] Show which models or services share each cache.
- [ ] Provide cleanup recommendations ordered by recoverable disk space.
- [ ] Explain what will need to be downloaded or compiled again.
- [ ] Never delete a shared cache as an implicit consequence of removing one
      server.
- [ ] Recheck available space immediately before large downloads.

### Existing installation discovery

- [ ] Discover existing Ollama models and match them to catalog variants.
- [ ] Preserve unmatched models as clearly labeled externally discovered
      entries.
- [ ] Rediscover Lumen Source-managed vLLM containers using ownership labels.
- [ ] Detect missing containers, deleted models, changed endpoints, and stale
      state without discarding user settings automatically.
- [ ] Let the user repair, reconnect, forget, or rematch a stale entry.
- [ ] Import and export external-runtime connection profiles without exporting
      API keys or other credentials.
- [ ] Prevent reconciliation from creating duplicate entries for persistent
      derived Ollama models.

### Interrupted operations

- [ ] Persist enough installation progress to identify an interrupted
      operation after application restart.
- [ ] Resume downloads when the runtime and source safely support it.
- [ ] Otherwise restart the operation safely and explain why it cannot resume.
- [ ] Identify and clean Lumen Source temporary files left by interruption.
- [ ] Reconcile data already committed to an Ollama or Hugging Face store
      instead of assuming cancellation removed it.
- [ ] Make runtime start, stop, update, remove, and cache operations mutually
      exclusive per model.

### 0.7 acceptance criteria

- [ ] Reported storage totals reconcile with the managed directories and
      runtime inventories.
- [ ] Every cleanup action lists its exact scope before confirmation.
- [ ] Existing Ollama installations can be adopted without downloading models
      again.
- [ ] Restarting Lumen Source during an installation results in a recoverable
      state.
- [ ] External changes never corrupt the persisted model library.

## 0.8: Safe updates and resource orchestration

Goal: manage the model library over time and prevent models from competing for
resources in surprising ways.

### Model and runtime updates

- [ ] Detect newer catalog revisions for an installed model.
- [ ] Separate application, runtime, container-image, model-weight, and
      tokenizer updates.
- [ ] Show download size, disk-space impact, license changes, revision changes,
      and expected compatibility before updating.
- [ ] Classify updates as optional, recommended, compatibility-related, or
      security-related without installing them automatically.
- [ ] Install the candidate alongside the working copy where the runtime
      permits it.
- [ ] Run the same post-install validation used for a new installation.
- [ ] Switch the model entry only after validation succeeds.
- [ ] Preserve enough metadata to roll back to the previous working revision.
- [ ] Never overwrite a user's model settings without showing the migration.
- [ ] Handle an updated license or usage policy as a new acknowledgement gate.

### Resource conflict management

- [ ] Estimate RAM and VRAM before starting a model.
- [ ] Account for loaded models, context allocation, concurrency, vLLM
      instances, and known shared-runtime behavior.
- [ ] Warn before a start is likely to exceed available resources.
- [ ] Offer **Stop other models and continue** with an exact list of affected
      models.
- [ ] Queue conflicting start and update operations instead of launching them
      simultaneously.
- [ ] Preserve models that the user explicitly pinned as always available.
- [ ] Show current RAM/VRAM consumers and the reason a model is waiting.
- [ ] Recover the queue cleanly after runtime or application restart.

### 0.8 acceptance criteria

- [ ] No update removes the last validated model copy before the replacement
      works.
- [ ] Failed updates leave the previous model startable.
- [ ] Runtime and model versions are visible and unambiguous.
- [ ] Starting two models that exceed detected memory produces a guided choice,
      not an unexplained runtime failure.
- [ ] Update and resource decisions are reproducible in lifecycle diagnostics.

## 0.9: Secure sharing and release hardening

Goal: prepare the supported workflows, packages, and state transitions for a
stable public release.

### Guided API sharing

- [ ] Keep localhost-only access as the default.
- [ ] Add an explicit **Allow other devices** workflow rather than making raw
      bind addresses the primary control.
- [ ] Require authentication before enabling managed network exposure.
- [ ] Generate, rotate, copy, and revoke API tokens through the operating
      system credential store.
- [ ] Display the exact reachable address and currently exposed models.
- [ ] Explain firewall and router requirements without silently changing them.
- [ ] Warn when transport is unencrypted and document a reverse-proxy/TLS
      deployment path.
- [ ] Provide one action to disable sharing and restore loopback-only binding.
- [ ] Do not claim that external services are secured or firewalled by Lumen
      Source.

### Diagnostics and support

- [ ] Export a redacted diagnostic bundle containing application, runtime,
      model, hardware, health, effective-setting, and recent lifecycle data.
- [ ] Exclude secrets, prompts, responses, user paths, hostnames, and raw remote
      addresses.
- [ ] Include a human-readable summary with recommended recovery actions.
- [ ] Add a user-visible state backup and restore workflow.
- [ ] Provide a safe reset that preserves installed model data unless the user
      explicitly chooses otherwise.

### Packaging and data safety

- [ ] Sign Windows installers and executables using the release signing
      identity.
- [ ] Produce verified Linux release packages for the supported distribution
      matrix.
- [ ] Test clean install, in-place upgrade, repair, and uninstall.
- [ ] Keep models, caches, settings, and credentials separate so uninstall can
      offer clear retention choices.
- [ ] Back up state before every schema migration.
- [ ] Restore the previous state automatically when migration fails.
- [ ] Recover from truncated or corrupted state files using the last valid
      backup.
- [ ] Ensure the application can start offline after an upgrade.

### Usability and accessibility

- [ ] Complete keyboard-only testing for every model-management workflow.
- [ ] Verify screen-reader labels, focus order, error announcements, and modal
      focus containment.
- [ ] Test contrast and layouts at operating-system text scaling levels.
- [ ] Remove remaining broken character encoding from visible strings.
- [ ] Use consistent beginner-facing names for model, runtime, server,
      endpoint, weights, cache, and context.
- [ ] Review every error for a clear next action.

### 0.9 acceptance criteria

- [ ] Network access cannot be enabled accidentally or without authentication.
- [ ] Diagnostic exports contain none of the prohibited sensitive fields.
- [ ] A failed state migration returns to the previous working application
      state.
- [ ] Signed Windows and supported Linux packages pass clean-machine upgrade
      and uninstall tests.
- [ ] Core workflows pass keyboard, screen-reader, scaling, and contrast
      acceptance checks.

## 1.0: Stabilization and support commitment

Goal: release a dependable supported product without adding another major
runtime or workflow during the stabilization cycle.

### Release freeze

- [ ] Freeze new runtime and orchestration features at the 1.0 release
      candidate.
- [ ] Resolve all data-loss, secret-exposure, installation, update, rollback,
      and model-removal defects.
- [ ] Resolve documented critical accessibility failures.
- [ ] Publish the final supported operating system, architecture, accelerator,
      runtime, and remote-target matrix.
- [ ] Publish known limitations without describing untested combinations as
      supported.

### End-to-end acceptance

- [ ] On every supported local platform:
  - [ ] Install Lumen Source on a clean machine
  - [ ] Detect hardware
  - [ ] Install an Ollama model with Balanced settings
  - [ ] Validate an inference response
  - [ ] Stop, start, configure, update, roll back, and remove the model
  - [ ] Upgrade Lumen Source without losing retained data
  - [ ] Uninstall with both retain-data and remove-data choices
- [ ] On a supported Linux NVIDIA machine:
  - [ ] Install and validate a cataloged managed-vLLM model
  - [ ] Restart and reconfigure its container
  - [ ] Install a second model with a distinct container and port
  - [ ] Remove servers independently from shared caches
- [ ] For external services:
  - [ ] Connect and authenticate to Ollama and vLLM
  - [ ] Confirm that Lumen Source never controls their lifecycle
- [ ] For remote targets:
  - [ ] Complete the documented supported Ollama workflow
  - [ ] Recover from connection loss without corrupting model state
- [ ] Complete offline-start, cached-catalog, low-disk, low-memory,
      interrupted-download, corrupted-state, and rejected-credential scenarios.

### 1.0 completion criteria

- [ ] Every advertised workflow has a repeatable clean-machine acceptance test.
- [ ] No critical or high-severity release-blocking issue remains open.
- [ ] All persisted schemas have tested migration and rollback paths.
- [ ] The full Windows and Linux build, test, package, and installer workflows
      pass from clean environments.
- [ ] User documentation covers installation, model selection, storage,
      updates, sharing, recovery, privacy, and uninstallation.
- [ ] A non-expert can complete the primary workflow without using a terminal or
      knowing which runtime flags to select.

## Explicitly deferred until after 1.0

- Additional general-purpose runtimes such as llama.cpp
- Native Windows managed vLLM
- Managed remote vLLM deployment
- ROCm and Intel managed-vLLM support without dedicated hardware acceptance
- Kubernetes, multi-node, and distributed inference
- Fine-tuning and training
- Agent frameworks and tool execution
- Prompt libraries and prompt synchronization
- A public model marketplace
- Hardware-independent performance rankings without reproducible benchmarks

Deferral does not prevent design work or experiments, but these items do not
block 1.0 and must not displace reliability work in the planned releases.

## Dependency order

```text
0.6 validated installation and safe profiles
    -> 0.7 recoverable inventory and storage ownership
        -> 0.8 side-by-side update, rollback, and resource scheduling
            -> 0.9 secure exposure, packaging, and state hardening
                -> 1.0 acceptance, documentation, and stabilization
```

Each release should ship only after its acceptance criteria pass. Incomplete
work moves forward explicitly; it must not be hidden by changing the criterion
after implementation.
