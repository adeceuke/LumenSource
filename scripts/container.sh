#!/usr/bin/env bash
set -euo pipefail

readonly SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly REPOSITORY_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
readonly CONTAINER_ENGINE="${CONTAINER_ENGINE:-docker}"
readonly HOST_UID="$(id -u)"
readonly HOST_GID="$(id -g)"
readonly CACHE_ROOT="${REPOSITORY_ROOT}/.container-cache"
readonly NODE_VERSION="$(tr -d '[:space:]' < "${REPOSITORY_ROOT}/.node-version")"
readonly RUST_VERSION="$(
  awk -F '"' '/^[[:space:]]*channel[[:space:]]*=/ { print $2; exit }' \
    "${REPOSITORY_ROOT}/rust-toolchain.toml"
)"
readonly IMAGE_NAME="${LUMEN_SOURCE_BUILD_IMAGE:-lumen-source-builder:ubuntu-24.04-node${NODE_VERSION}-rust${RUST_VERSION}-${HOST_UID}-${HOST_GID}}"

usage() {
  cat <<'EOF'
Usage: scripts/container.sh <command> [arguments]

Commands:
  image             Build or refresh the Ubuntu 24.04 builder image.
  check             Run formatting, linting, Rust tests, and frontend build.
  package           Build Ubuntu deb and AppImage artifacts.
  shell             Open an interactive shell in the build environment.
  run <command...>  Run an arbitrary command in the build environment.

Environment:
  CONTAINER_ENGINE          Container command to use (default: docker).
  LUMEN_SOURCE_BUILD_IMAGE  Override the local builder image name.
EOF
}

require_engine() {
  if ! command -v "$CONTAINER_ENGINE" >/dev/null 2>&1; then
    echo "Container engine '${CONTAINER_ENGINE}' is not installed or not in PATH." >&2
    exit 1
  fi
  if ! "$CONTAINER_ENGINE" info >/dev/null 2>&1; then
    echo "Cannot access the ${CONTAINER_ENGINE} daemon." >&2
    echo "Docker access must be configured by the machine administrator." >&2
    exit 1
  fi
}

build_image() {
  require_engine
  if [[ -z "$NODE_VERSION" || -z "$RUST_VERSION" ]]; then
    echo "Unable to read the pinned Node.js or Rust version." >&2
    exit 1
  fi
  "$CONTAINER_ENGINE" build \
    --build-arg "NODE_VERSION=${NODE_VERSION}" \
    --build-arg "RUST_VERSION=${RUST_VERSION}" \
    --build-arg "USER_ID=${HOST_UID}" \
    --build-arg "GROUP_ID=${HOST_GID}" \
    --tag "$IMAGE_NAME" \
    "$REPOSITORY_ROOT"
}

ensure_image() {
  require_engine
  if ! "$CONTAINER_ENGINE" image inspect "$IMAGE_NAME" >/dev/null 2>&1; then
    build_image
  fi
}

prepare_cache() {
  mkdir -p \
    "${CACHE_ROOT}/cargo-git" \
    "${CACHE_ROOT}/cargo-registry" \
    "${CACHE_ROOT}/npm"
}

run_container() {
  ensure_image
  prepare_cache

  local -a tty_arguments=()
  if [[ -t 0 && -t 1 ]]; then
    tty_arguments=(-it)
  fi

  "$CONTAINER_ENGINE" run \
    --rm \
    "${tty_arguments[@]}" \
    --env "APPIMAGE_EXTRACT_AND_RUN=1" \
    --env "CARGO_TARGET_DIR=/workspace/target" \
    --volume "${REPOSITORY_ROOT}:/workspace" \
    --volume "${CACHE_ROOT}/cargo-git:/home/builder/.cargo/git" \
    --volume "${CACHE_ROOT}/cargo-registry:/home/builder/.cargo/registry" \
    --volume "${CACHE_ROOT}/npm:/home/builder/.npm" \
    --workdir /workspace \
    "$IMAGE_NAME" \
    "$@"
}

command_name="${1:-}"
case "$command_name" in
  image)
    build_image
    ;;
  check)
    run_container bash -lc '
      set -euo pipefail
      cargo fmt --all --check
      cargo clippy --workspace --all-targets -- -D warnings
      cargo test --workspace
      cd apps/desktop
      npm ci
      npm run typecheck
      npm run build
    '
    ;;
  package)
    run_container bash -lc '
      set -euo pipefail
      cd apps/desktop
      npm ci
      npm run tauri build -- --bundles deb,appimage

      if compgen -G "target/release/bundle/appimage/*.AppImage" > /dev/null; then
        for appimage in target/release/bundle/appimage/*.AppImage; do
          target_name="${appimage// /}"
          if [[ "$appimage" != "$target_name" ]]; then
            mv -f "$appimage" "$target_name"
          fi
        done
      fi
    '
    echo "Packages are available under target/release/bundle/."
    ;;
  shell)
    run_container bash
    ;;
  run)
    shift
    if (( $# == 0 )); then
      echo "The run command requires a command to execute." >&2
      usage
      exit 1
    fi
    run_container "$@"
    ;;
  -h|--help|help)
    usage
    ;;
  *)
    usage >&2
    exit 1
    ;;
esac
