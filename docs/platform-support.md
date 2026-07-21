# Platform support

## v0.1 target

Lumen Source v0.1 is developed, packaged, and acceptance-tested on Ubuntu
24.04 LTS. The first supported CPU architecture is x86_64; aarch64 should not
be advertised until it has its own clean-machine acceptance run.

The release gate is:

1. Install Lumen Source on a clean Ubuntu 24.04 machine.
2. Detect CPU, RAM, storage, and available GPU acceleration.
3. Fetch and authenticate the catalog.
4. Recommend and install a compatible Ollama model.
5. Start the runtime and expose working endpoint details.
6. Obtain a response from that endpoint using an external client.

## Cross-platform architecture constraints

Windows and macOS are planned platforms even though they are not v0.1 release
targets. Code added for Ubuntu must preserve these boundaries:

- Domain types, catalog parsing, compatibility rules, and recommendation logic
  must not depend on Linux APIs.
- Hardware collection is implemented behind `HardwareProbe`, with
  platform-specific modules selected using Rust `cfg` attributes.
- Runtime process and installation behavior stays behind `Runtime`.
- Local machine operations stay behind `Host`; filesystem paths use platform
  application-data/cache directories rather than hard-coded Linux paths.
- Tauri commands call platform-neutral core use cases and must not invoke shell
  commands directly.
- Fixed operating-system commands, where unavoidable, are confined to a
  platform adapter and never constructed from untrusted user input.
- Catalog entries carry OS/architecture compatibility and per-platform
  artifacts instead of assuming Linux binaries.
- UI text must not promise Linux-specific acceleration or installation steps
  when running on another platform.

## Planned adapters

| Capability | Ubuntu 24.04 v0.1 | macOS future | Windows future |
| --- | --- | --- | --- |
| Desktop shell | Tauri/WebKitGTK | Tauri/WKWebView | Tauri/WebView2 |
| Hardware facts | `/proc`, system APIs, fixed vendor tools | system APIs, `system_profiler`/Metal | Windows APIs, WMI/DXGI |
| Initial runtime | Ollama | Ollama, later MLX | Ollama |
| Packaging | deb/AppImage | app bundle/dmg, notarized | MSI/NSIS, code-signed |
| Service/agent | deferred | launchd | Windows Service |

Platform-specific implementations should be added as sibling modules, not as
conditionals spread through recommendation or orchestration code.
