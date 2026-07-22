# Development setup

## Containerized build (recommended)

The checked-in `Dockerfile` provides the Ubuntu 24.04 native libraries, pinned
Node.js version, and Rust tools needed to check and package Lumen Source. It
does not require `sudo` on the host, but the current user must be allowed to
access the Docker daemon.

From the repository root:

```shell
scripts/container.sh image
scripts/container.sh check
scripts/container.sh package
```

The package command produces Ubuntu `.deb` and AppImage artifacts under
`target/release/bundle/`. The source tree is mounted into the container, and
the container user has the same UID and GID as the host user, so generated
files are not owned by root. Download caches are retained in the ignored
`.container-cache/` directory.

Useful development commands include:

```shell
scripts/container.sh shell
scripts/container.sh run cargo test -p lumen-source-catalog
```

Set `CONTAINER_ENGINE` to use another Docker-compatible command. Set
`LUMEN_SOURCE_BUILD_IMAGE` to override the generated image name.

The container is only a build and validation environment. Install and run the
resulting application directly on Ubuntu to test desktop integration, hardware
detection, model installation, and Ollama process management. Prefer the
`.deb` for Ubuntu testing because the system package manager can install and
track native runtime dependencies.

## Supported development host

The v0.1 reference platform is Ubuntu 24.04 LTS on x86_64. Run the checked-in
installer on a development machine when a fully native toolchain is desired:

```shell
scripts/setup-ubuntu-24.04.sh
scripts/check-prerequisites.sh
```

The installer adds the native Tauri packages, stable Rust with `rustfmt` and
`clippy`, and the Node.js version pinned in `.node-version`.

## Toolchains

The Rust release in `rust-toolchain.toml` and the Node.js release in
`.node-version` are the authoritative toolchain versions. Rust includes
`rustfmt` and `clippy`; the desktop frontend uses npm.

## Manual Linux dependency installation

Tauri 2 requires native WebKit and desktop integration development packages.
On Ubuntu 24.04:

```shell
sudo apt update
sudo apt install \
  build-essential \
  ca-certificates \
  curl \
  file \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  libssl-dev \
  libwebkit2gtk-4.1-dev \
  libxdo-dev \
  patchelf \
  pkg-config \
  wget \
  xz-utils
```

The exact package names differ on other distributions. Other operating systems
are architectural targets but are not v0.1 release targets; see
[platform support](platform-support.md).

## Checks

The preferred check runs formatting, Clippy with warnings denied, every Rust
test including the Tauri bridge, TypeScript type checking, and the production
frontend build:

```shell
scripts/container.sh check
```

For a native machine without the GTK/WebKit development libraries, the shared
Rust crates can still be checked separately:

From the repository root:

```shell
cargo fmt --all --check
cargo clippy --workspace --exclude lumen-source-desktop --all-targets -- -D warnings
cargo test --workspace --exclude lumen-source-desktop
```

From `apps/desktop`:

```shell
npm ci
npm run typecheck
npm run build
npm run tauri dev
```

The desktop package cannot be compiled natively on Linux unless the native
Tauri dependencies are installed. The containerized build supplies those
dependencies inside its image.

## Testing the guided flow

Use **Dummy Test Model** in a development build to exercise the wizard and
models page without downloading a runtime or model. It is overlaid from the
legacy test fixture and is not included in release builds. You can also select
a compatible entry from `catalog/model-list.json` for an Ollama-backed test.

1. Open **Add model** and keep local deployment selected.
2. Continue through hardware detection and select any use case.
3. Open **Choose from all available models** and select a catalog model;
   verify its detail panel replaces the recommended model's information.
4. Complete preflight and choose whether **Start model after installation** is
   checked.
5. Verify the Ready screen, copy controls, and model-list start/stop button.
6. Reload the frontend and verify that a running model remains running.
7. Install the same model again and verify both entries remain in the model
   list after a frontend reload. Starting or stopping either entry should
   update both because they reference the same runtime model.
8. Open a model action menu and verify that clicking elsewhere or pressing
   Escape closes it.
9. Select a model row and verify that the full-body detail page opens on Logs,
   the model name replaces **Local models**, and **Back to model list** returns
   to the list.
10. Verify the recorded lifecycle entries and test Copy and Clear.
11. Open the **Performance** tab and verify that the selected model state,
    total resident memory, model allocation in system RAM, GPU VRAM allocation,
    and context capacity refresh every two seconds. Stop the model and verify that
    its allocations fall to zero. Leave the page open long enough to verify
    that each memory metric builds a line in the rolling graph.
12. Start a stopped real model and verify that an in-row spinner and **Starting
    model…** label remain visible until the runtime request finishes.
13. Open the **API** tab and verify that its base URL, chat-completions URL, and
    model identifier belong to the selected entry. Copy each value and the cURL
    example, verifying that the button temporarily changes to **✓ Copied**.
14. Complete the wizard and repeat the clipboard-feedback check on its Ready
    screen.

For cancellation testing, begin a real runtime/model download and select
**Cancel installation**. The wizard should report cancellation, remain on the
install step, remove temporary runtime download/staging files, and allow a
retry.
