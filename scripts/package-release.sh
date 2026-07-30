#!/usr/bin/env bash
set -euo pipefail

readonly SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly REPOSITORY_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
readonly BUNDLE_ROOT="${REPOSITORY_ROOT}/target/release/bundle"

"${SCRIPT_DIR}/container.sh" check
"${SCRIPT_DIR}/container.sh" package

mapfile -t deb_packages < <(find "${BUNDLE_ROOT}/deb" -maxdepth 1 -type f -name '*.deb' -print)
mapfile -t appimages < <(find "${BUNDLE_ROOT}/appimage" -maxdepth 1 -type f -name '*.AppImage' -print)

if (( ${#deb_packages[@]} == 0 || ${#appimages[@]} == 0 )); then
  echo "Both deb and AppImage artifacts are required." >&2
  exit 1
fi

for package in "${deb_packages[@]}"; do
  dpkg-deb --info "$package" >/dev/null
done
for appimage in "${appimages[@]}"; do
  APPIMAGE_EXTRACT_AND_RUN=1 "$appimage" --appimage-version >/dev/null
done

sha256sum "${deb_packages[@]}" "${appimages[@]}" > "${BUNDLE_ROOT}/SHA256SUMS"
echo "Verified Linux release packages and wrote ${BUNDLE_ROOT}/SHA256SUMS."
