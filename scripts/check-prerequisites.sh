#!/usr/bin/env bash
set -euo pipefail

export PATH="${HOME}/.local/bin:${PATH}"
if [[ -r "${HOME}/.cargo/env" ]]; then
  # shellcheck disable=SC1091
  source "${HOME}/.cargo/env"
fi

failed=0

check_command() {
  local command_name="$1"
  if command -v "$command_name" >/dev/null 2>&1; then
    printf 'ok  %-12s %s\n' "$command_name" "$("$command_name" --version 2>&1 | awk 'NR == 1')"
  else
    printf 'missing  %s\n' "$command_name" >&2
    failed=1
  fi
}

check_pkg_config() {
  local module="$1"
  if pkg-config --exists "$module"; then
    printf 'ok  %-12s %s\n' "$module" "$(pkg-config --modversion "$module")"
  else
    printf 'missing  pkg-config module %s\n' "$module" >&2
    failed=1
  fi
}

if [[ -r /etc/os-release ]]; then
  # shellcheck disable=SC1091
  source /etc/os-release
  if [[ "${ID:-}" == "ubuntu" && "${VERSION_ID:-}" == "24.04" ]]; then
    echo "ok  platform     Ubuntu 24.04"
  else
    echo "warning: v0.1 is validated on Ubuntu 24.04; detected ${PRETTY_NAME:-unknown}." >&2
  fi
fi

check_command cargo
check_command rustc
check_command rustfmt
check_command cargo-clippy
check_command node
check_command npm
check_command pkg-config

check_pkg_config gtk+-3.0
check_pkg_config webkit2gtk-4.1
check_pkg_config javascriptcoregtk-4.1
check_pkg_config librsvg-2.0

if (( failed != 0 )); then
  echo >&2
  echo "One or more prerequisites are missing." >&2
  echo "On Ubuntu 24.04, run scripts/setup-ubuntu-24.04.sh." >&2
  exit 1
fi

echo "All Ubuntu 24.04 development prerequisites are available."
