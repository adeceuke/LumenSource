# Known limitations for 1.0

- The stable support claims remain candidates until the clean-machine
  acceptance campaign is signed off.
- Managed vLLM is Linux/NVIDIA/container-only. Windows can connect to an
  existing vLLM endpoint but cannot deploy one.
- Lumen Source does not install a persistent operating-system service for a
  standalone Ollama runtime. Reboot behavior depends on the Ollama installation
  or service owner.
- The authenticated sharing gateway is HTTP. TLS, internet routing, VPNs,
  firewall rules, and reverse-proxy maintenance remain the operator's
  responsibility.
- External Ollama/vLLM lifecycle and security remain external even when their
  models appear in Lumen Source.
- Remote deployment is agentless and requires a working OpenSSH client,
  supported authentication, and an existing remote Ollama service.
- Hardware counters are best effort. Some firmware, drivers, virtual machines,
  and non-NVIDIA accelerators do not expose memory speed, VRAM, utilization, or
  per-model allocation.
- Storage totals are estimates where a runtime does not expose exact per-model
  ownership or where files are shared.
- Lumen Source does not delete model weights, caches, or credential-store
  entries merely because the application package is uninstalled.
- Application self-update delivery is not automatic; users explicitly install
  a newer application package. Unsigned Windows packages trigger
  unknown-publisher warnings and may be blocked by managed security policy.
