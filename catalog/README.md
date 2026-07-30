# Catalog

This directory contains development fixtures and schema documentation for the signed, versioned Lumen Source model/runtime catalog.

## Externally generated model list

The model-information producer/consumer boundary is defined by two files:

- `model-list.schema.json` is the normative JSON Schema (Draft 2020-12).
- `model-list.template.json` is a valid, copyable example for a generator.
- `model-list.json` is the generated artifact embedded as the desktop's bundled
  model catalog.

The separate generator owns discovery, online research, normalization, and
source attribution. Lumen Source must not scrape model information. It should
only validate and consume a generated artifact, then apply its own local
hardware detection and recommendation logic.

All storage and memory quantities use integer bytes. This avoids ambiguous
`GB`/`GiB` values. Requirements belong to an exact model variant because
quantization changes download size and memory needs. Minimum and recommended
system RAM are separate fields; VRAM can be `null` when it is not required.
`download_item_count` is the pinned manifest's config blob plus layer count. It
lets LumenSource show which model-download item is active while the byte
percentage tracks that item; cached blobs may complete immediately.

Every model and variant requires source URLs and retrieval timestamps. Optional
performance values must be tied to a named hardware configuration and source;
the producer should omit `benchmarks` rather than inventing a generic
tokens-per-second estimate.

The model-list contract intentionally excludes runtime binary download and
installation metadata. That metadata is security-sensitive and remains owned
by Lumen Source's signed runtime catalog. The generated model list accepts
`ollama` and `vllm` engine metadata. Ollama variants use an exact tagged model
reference. vLLM-compatible variants may also provide a Hugging Face repository
identifier, pinned model and tokenizer revisions, task/runner information, and
compatibility tags. These vLLM fields describe model compatibility only: an
external vLLM service is connected from the installed model's Settings tab,
while managed vLLM installation remains a separate delivery step.

Schema v2 includes structured license permissions, obligations, restrictions,
usage-policy links, geographic conditions, and minimum UI notice behavior. The
desktop presents those fields in a dedicated license-review step and enforces
the required acknowledgement before installation. A separate-license reference
is a local user assertion, not validation by LumenSource.

A model-list artifact fetched independently at runtime must also be signed and
verified before Lumen Source consumes it. The `generator` and `sources` fields
provide provenance but are not an integrity or authenticity mechanism. A file
generated during the application build may instead be reviewed and shipped as
an application resource.

Production catalogs are fetched over HTTPS, authenticated, and cached as the last-known-good version. Fixtures here are not trusted production catalogs.

Detached Ed25519 signatures are transported as standard base64 text in a sibling
artifact (for example, `catalog.json.sig`). The signature covers the exact bytes
of the JSON artifact; producers must not reformat it after signing.

Fixtures:

- `fixtures/catalog.v1.valid.json` — schema v1 data containing an Ollama model
  and the no-download dummy model/runtime used by desktop UI tests
- `fixtures/catalog.invalid-schema.json` — well-formed catalog using an unsupported schema
- `fixtures/catalog.invalid-shape.json` — malformed catalog data for parser rejection
- `fixtures/catalog.v1.example.json` — minimal empty-catalog example

The dummy runtime URLs and checksums are deliberately non-production values.
The desktop adapter recognizes `dummy-runtime` before artifact installation and
routes it to its dedicated in-memory implementation; those artifacts must never
be downloaded.
