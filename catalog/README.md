# Catalog

This directory contains development fixtures and schema documentation for the signed, versioned Lumen Source model/runtime catalog.

Production catalogs are fetched over HTTPS, authenticated, and cached as the last-known-good version. Fixtures here are not trusted production catalogs.

Detached Ed25519 signatures are transported as standard base64 text in a sibling
artifact (for example, `catalog.json.sig`). The signature covers the exact bytes
of the JSON artifact; producers must not reformat it after signing.

Fixtures:

- `fixtures/catalog.v1.valid.json` — realistic schema v1 runtime and model data
- `fixtures/catalog.invalid-schema.json` — well-formed catalog using an unsupported schema
- `fixtures/catalog.invalid-shape.json` — malformed catalog data for parser rejection
- `fixtures/catalog.v1.example.json` — minimal empty-catalog example
