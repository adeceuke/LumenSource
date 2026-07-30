# Usage telemetry

LumenSource usage telemetry is optional. The desktop asks before collecting
anything, stores the choice locally, and exposes the setting in the persistent
footer. Disabling telemetry immediately deletes every unsent report.

## Collected data

The desktop stores weekly aggregates containing:

- application version, operating-system family, and CPU architecture;
- catalog revision, source (`network`, `cache`, or `bundled`), and load count;
- coarse RAM, VRAM, and accelerator tiers;
- catalog model and variant IDs;
- local versus remote deployment;
- aggregate install, uninstall, start, and built-in chat outcome counts; and
- allowlisted failure categories.

Prompts, responses, message counts, tokens, file or workspace metadata,
hostnames, remote target identifiers, account identifiers, IP addresses, and
raw error messages are not part of the client payload. Usage sent directly to
an Ollama/OpenAI-compatible endpoint does not pass through LumenSource and
cannot be measured by this telemetry.

## Offline queue and retry behavior

Reports are saved under the operating system's local application-data
directory in `lumen-source/telemetry-v1.json`. The queue retains at most 52
weekly reports.

Every recorded occurrence schedules a best-effort upload. Startup also
schedules an upload. A connection error, timeout, non-2xx response, malformed
local queue, or local persistence error is isolated from the application flow
and never makes a LumenSource operation fail.

The server must return any 2xx response only after durably accepting the
reports. The client then deletes exactly the acknowledged batch from its local
queue. Failed or interrupted attempts leave reports intact, and the next
startup or occurrence retries them. Concurrent occurrences are coalesced so an
offline client does not accumulate network retries.

## Ingestion contract

The production endpoint is:

```text
POST https://lumensource.dev/v2/telemetry
Content-Type: application/json
```

It can be overridden for development with `LUMEN_SOURCE_TELEMETRY_URL`.

The request has this shape:

```json
{
  "schemaVersion": 1,
  "reports": [
    {
      "reportId": "b7e5708d-50dc-4fd8-80fa-d9477211778b",
      "periodStart": "2026-07-20",
      "periodEnd": "2026-07-26",
      "appVersion": "0.6.0",
      "platform": "linux",
      "architecture": "x86_64",
      "catalog": {
        "2026.07.1:network": {
          "loads": 3
        }
      },
      "hardware": {
        "ramTier": "16-31-gib",
        "vramTier": "8-15-gib",
        "accelerator": "cuda"
      },
      "models": [
        {
          "modelId": "qwen3",
          "variantId": "qwen3-8b-q4_k_m",
          "deployment": "local",
          "installs": {
            "attempted": 1,
            "succeeded": 1,
            "failed": 0
          },
          "uninstalls": {
            "attempted": 0,
            "succeeded": 0,
            "failed": 0
          },
          "starts": {
            "attempted": 4,
            "succeeded": 4,
            "failed": 0
          },
          "chats": {
            "attempted": 9,
            "succeeded": 8,
            "failed": 0,
            "cancelled": 1
          },
          "failures": {}
        }
      ]
    }
  ]
}
```

The ingestion service should enforce a request-size limit, reject unknown
fields or schema versions, deduplicate on `reportId`, discard source IP
addresses from analytics storage, and keep transport/security logs separate
from product analytics.
