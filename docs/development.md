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
`docs/platform-support.md`.

## Checks

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
