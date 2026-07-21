#!/usr/bin/env bash
set -euo pipefail

readonly SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly REPOSITORY_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
readonly REQUIRED_UBUNTU_VERSION="24.04"
readonly DEFAULT_NODE_VERSION="$(tr -d '[:space:]' < "${REPOSITORY_ROOT}/.node-version")"
readonly DEFAULT_RUST_VERSION="$(
  awk -F '"' '/^[[:space:]]*channel[[:space:]]*=/ { print $2; exit }' \
    "${REPOSITORY_ROOT}/rust-toolchain.toml"
)"
readonly NODE_VERSION="${NODE_VERSION:-$DEFAULT_NODE_VERSION}"
readonly RUST_VERSION="${RUST_VERSION:-$DEFAULT_RUST_VERSION}"
readonly NODE_ROOT="${HOME}/.local/share/lumen-source/node-v${NODE_VERSION}"
readonly LOCAL_BIN="${HOME}/.local/bin"

if [[ ! -r /etc/os-release ]]; then
  echo "Unable to identify this operating system (/etc/os-release is missing)." >&2
  exit 1
fi

# shellcheck disable=SC1091
source /etc/os-release
if [[ "${ID:-}" != "ubuntu" || "${VERSION_ID:-}" != "$REQUIRED_UBUNTU_VERSION" ]]; then
  echo "This installer targets Ubuntu ${REQUIRED_UBUNTU_VERSION}; detected ${PRETTY_NAME:-unknown}." >&2
  echo "Use docs/development.md for manual setup on another platform." >&2
  exit 1
fi

echo "Installing Ubuntu packages required by Rust, Tauri 2, and AppImage/deb builds..."
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
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

mkdir -p "$LOCAL_BIN" "${HOME}/.local/share/lumen-source"

install_node() {
  local machine_arch node_arch archive base_url checksum_line
  machine_arch="$(uname -m)"
  case "$machine_arch" in
    x86_64) node_arch="x64" ;;
    aarch64|arm64) node_arch="arm64" ;;
    *)
      echo "Unsupported CPU architecture for the Node.js installer: ${machine_arch}" >&2
      exit 1
      ;;
  esac

  archive="node-v${NODE_VERSION}-linux-${node_arch}.tar.xz"
  base_url="https://nodejs.org/dist/v${NODE_VERSION}"
  local temp_dir
  temp_dir="$(mktemp -d)"
  trap 'rm -rf "$temp_dir"' RETURN

  echo "Downloading Node.js ${NODE_VERSION} (${node_arch})..."
  curl --fail --location --proto '=https' --tlsv1.2 \
    "${base_url}/${archive}" -o "${temp_dir}/${archive}"
  curl --fail --location --proto '=https' --tlsv1.2 \
    "${base_url}/SHASUMS256.txt" -o "${temp_dir}/SHASUMS256.txt"

  checksum_line="$(grep " ${archive}$" "${temp_dir}/SHASUMS256.txt" || true)"
  if [[ -z "$checksum_line" ]]; then
    echo "Node.js checksum was not found for ${archive}." >&2
    exit 1
  fi
  (
    cd "$temp_dir"
    printf '%s\n' "$checksum_line" | sha256sum --check -
  )

  rm -rf "$NODE_ROOT"
  mkdir -p "$NODE_ROOT"
  tar --extract --xz --strip-components=1 \
    --file "${temp_dir}/${archive}" --directory "$NODE_ROOT"
  ln -sfn "${NODE_ROOT}/bin/node" "${LOCAL_BIN}/node"
  ln -sfn "${NODE_ROOT}/bin/npm" "${LOCAL_BIN}/npm"
  ln -sfn "${NODE_ROOT}/bin/npx" "${LOCAL_BIN}/npx"
}

if ! command -v node >/dev/null 2>&1 \
  || [[ "$(node --version)" != "v${NODE_VERSION}" ]]; then
  install_node
else
  echo "Node.js ${NODE_VERSION} is already installed."
fi

if ! command -v rustup >/dev/null 2>&1; then
  echo "Installing rustup..."
  rustup_installer="$(mktemp)"
  curl --fail --proto '=https' --tlsv1.2 \
    https://sh.rustup.rs -o "$rustup_installer"
  sh "$rustup_installer" -y --profile minimal --default-toolchain "$RUST_VERSION"
  rm -f "$rustup_installer"
fi

# shellcheck disable=SC1091
source "${HOME}/.cargo/env"
rustup toolchain install "$RUST_VERSION" --profile minimal
rustup default "$RUST_VERSION"
rustup component add --toolchain "$RUST_VERSION" clippy rustfmt

export PATH="${LOCAL_BIN}:${PATH}"

echo
echo "Dependencies installed."
echo "Ensure ${LOCAL_BIN} is present in PATH, then run:"
echo "  scripts/check-prerequisites.sh"
echo "  cd apps/desktop && npm install"
