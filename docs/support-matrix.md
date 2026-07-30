# Lumen Source 1.0 support matrix

This is the candidate support boundary for the 1.0 acceptance campaign. A row
becomes supported only after its linked clean-machine cases pass; untested
combinations are not implied by architectural compatibility.

| Area | 1.0 candidate | Status before campaign |
| --- | --- | --- |
| Windows desktop | Windows 11 x86_64, native MSI and NSIS | Candidate |
| Linux desktop | Ubuntu 24.04 LTS x86_64, deb and AppImage | Candidate |
| macOS desktop | None | Unsupported |
| Default runtime | Local Ollama, existing or explicitly downloaded | Candidate |
| Managed vLLM | Ubuntu 24.04, NVIDIA GPU, Docker or Podman GPU support | Candidate |
| Native Windows managed vLLM | None | Unsupported |
| External services | OpenAI-compatible vLLM and Ollama connections | Candidate; lifecycle remains external |
| Remote targets | Agentless OpenSSH to an existing Ollama service on documented Linux or Windows targets | Preview until campaign passes |
| CPU inference | Ollama catalog variants within detected RAM/storage limits | Candidate |
| NVIDIA acceleration | Ollama; managed vLLM on the Linux configuration above | Candidate |
| AMD/Intel acceleration | Ollama where the runtime and installed driver expose it | Best effort, not a managed-vLLM claim |
| Architecture | x86_64 | Candidate |
| aarch64 | None | Unsupported |

Catalog compatibility is narrower than this table when a model variant declares
specific OS, architecture, RAM, VRAM, runtime, or accelerator requirements.
Lumen Source blocks a catalog variant that fails those hard requirements.

Explicitly unsupported for 1.0: Kubernetes, multi-node inference, fine-tuning,
training, ROCm or Intel managed vLLM, managed remote vLLM, llama.cpp, MLX,
public internet exposure without an independently maintained TLS/network
gateway, and automatic lifecycle control of external services.
