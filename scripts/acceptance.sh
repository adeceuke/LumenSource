#!/usr/bin/env bash
set -euo pipefail

readonly SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly REPOSITORY_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
readonly OUTPUT_DIRECTORY="${1:-${REPOSITORY_ROOT}/acceptance-results}"
readonly TIMESTAMP="$(date -u +%Y%m%d-%H%M%S)"
readonly REPORT_PATH="${OUTPUT_DIRECTORY}/ubuntu-${TIMESTAMP}.md"

mkdir -p "$OUTPUT_DIRECTORY"
"${SCRIPT_DIR}/container.sh" check

commit="$(git -C "$REPOSITORY_ROOT" rev-parse HEAD)"
os="$(. /etc/os-release && printf '%s %s' "$NAME" "$VERSION_ID")"
cpu="$(LC_ALL=C lscpu | awk -F: '/Model name/ { sub(/^[[:space:]]+/, "", $2); print $2; exit }')"
memory_bytes="$(awk '/MemTotal/ { print $2 * 1024; exit }' /proc/meminfo)"
gpu="$(nvidia-smi --query-gpu=name,driver_version --format=csv,noheader 2>/dev/null || printf 'not detected')"

cat >"$REPORT_PATH" <<EOF
# Ubuntu 1.0 acceptance evidence

- Commit: \`$commit\`
- Recorded: \`$(date -u --iso-8601=seconds)\`
- OS: \`$os\`
- CPU: \`$cpu\`
- RAM bytes: \`$memory_bytes\`
- GPU: \`$gpu\`
- Automated suite: PASS
- Package path/hash: PENDING
- Tester: PENDING

## Required cases

| Case | Result | Evidence / defect |
| --- | --- | --- |
| LINUX-PKG-001 | PENDING | |
| LINUX-INSTALL-001 | PENDING | |
| LINUX-OLLAMA-001 | PENDING | |
| LINUX-VLLM-001 | PENDING | |
| LINUX-VLLM-002 | PENDING | |
| LINUX-UPGRADE-001 | PENDING | |
| LINUX-RECOVERY-001 | PENDING | |
| LINUX-UNINSTALL-001 | PENDING | |
| LINUX-UNINSTALL-002 | PENDING | |
| EXT-OLLAMA-001 | PENDING | |
| EXT-VLLM-001 | PENDING | |
| REMOTE-OLLAMA-001 | PENDING | |
| RESOURCE-001 | PENDING | |
| SHARE-001 | PENDING | |
| SHARE-002 | PENDING | |
| SHARE-003 | PENDING | |
| LOW-DISK-001 | PENDING | |
| LOW-MEMORY-001 | PENDING | |
| A11Y-KEYBOARD-001 | PENDING | |
| A11Y-SCREENREADER-001 | PENDING | |
| A11Y-CONTRAST-001 | PENDING | |
| COMPREHENSION-001 | PENDING | |
EOF

printf 'Created %s\n' "$REPORT_PATH"
