# Runtime support and recovery

Lumen Source 0.4 uses Ollama by default. Existing models keep their runtime when
the default for future installations changes.

## Support matrix

| Controller and target | Ollama | External vLLM | Lumen Source-managed vLLM |
| --- | --- | --- | --- |
| Windows, local | Supported, including managed standalone Ollama | Supported | Not supported by upstream vLLM |
| Windows to remote machine | Supported over SSH | Supported by connecting to the service endpoint | Not yet supported |
| Linux, local NVIDIA GPU | Supported | Supported | Supported with Docker or Podman and NVIDIA container support |
| Linux, local AMD or Intel GPU | Supported where Ollama supports the accelerator | Supported | Not yet acceptance-tested |
| Linux to remote machine | Supported over SSH | Supported by connecting to the service endpoint | Not yet supported |
| macOS, local | Supported by Ollama | Supported | Not supported |

External vLLM means Lumen Source stores a connection for one served model. It
does not start, stop, update, or reconfigure that server. Managed vLLM uses the
official `vllm/vllm-openai:v0.23.0` image, one loopback port and container per
model, and shared persistent Hugging Face and vLLM compile-cache volumes.

Managed vLLM is deliberately NVIDIA-first. Lumen Source detects Docker or
Podman and NVIDIA container support, but it never installs a container engine,
GPU driver, or GPU runtime. Native Windows vLLM management remains unavailable;
Windows users can continue to use Ollama or connect to a vLLM service hosted on
Linux.

## Settings behavior

- Chat defaults apply to the next request made by Lumen Source. Other API
  clients can supply their own request values.
- Context, accelerator and vLLM engine changes are load-time settings and
  require **Apply and restart**.
- Persistent Ollama parameters create or update the explicitly named derived
  model. Reapplying the same name replaces it instead of creating duplicates.
- External vLLM engine fields are read-only. Its endpoint, TLS behavior,
  timeouts and API key remain per-model connection settings.
- Hugging Face tokens and vLLM API keys are held by the operating-system
  credential store. They are not written to application state or lifecycle
  logs.

## Managed vLLM prerequisites

On the Linux machine running Lumen Source:

1. Install a compatible NVIDIA driver.
2. Install Docker with NVIDIA Container Toolkit, or Podman with NVIDIA CDI
   support.
3. Verify that GPU-enabled containers work outside Lumen Source.
4. If the selected Hugging Face model is private or gated, save a Hugging Face
   token on the Lumen Source Settings page.

Lumen Source launches only predetermined arguments without a shell. Containers
bind to `127.0.0.1`, API documentation is disabled, and remote-code and local
media access are not enabled.

## Recovery

If new model settings fail to start, Lumen Source restores the last persisted
working settings and attempts to relaunch the previous configuration. Open the
model's **Settings** tab to inspect runtime health, version, effective settings,
container identity, endpoint, and recent managed-container logs.

For a managed vLLM failure:

1. Confirm Docker or Podman and the NVIDIA runtime still work.
2. Check that the configured managed port range has a free loopback port.
3. Review the diagnostics for model-loading, revision, quantization, or memory
   errors.
4. Reset model overrides and apply the runtime defaults.
5. Remove the managed server if it cannot be recovered. This removes only its
   container; shared model and compile caches are retained.
6. Delete shared vLLM caches from global Settings only when you explicitly want
   the model weights or compiled artifacts to be downloaded or built again.

Runtime migration always installs and validates the equivalent catalog variant
first. The original copy remains installed and is never deleted automatically.
If no equivalent catalog variant exists, the migration action stays disabled.
