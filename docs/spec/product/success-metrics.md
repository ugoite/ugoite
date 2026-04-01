# Success Metrics

These metrics help evaluate whether Ugoite is delivering on its principles
(**Low Cost**, **Easy**, **Freedom**).

## Product Metrics

- **Time-to-first-login (browser path)**: newcomer can start the published
  browser stack and complete the explicit login flow without getting lost in
  auth-mode or runtime setup details.
- **Time-to-first-writable-space (browser path)**: newcomer can reach a space
  where they are actually allowed to create content after startup and login.
- **Time-to-first-entry**: newcomer can create a space + first entry quickly
  once they are in a writable state. Report this separately for browser quick
  start versus CLI/core workflows so setup/auth friction does not disappear
  behind a single number.
- **Time-to-first-structured-field**: user can define a Form and see extracted fields.
- **Search usefulness**: keyword search returns expected results with low latency.

Treat these three newcomer-path metrics together. For the browser route, image
pulls, stack startup, explicit login, and arrival in the first writable space
are all part of the Easy principle; `time-to-first-entry` alone is not an
honest proxy for the full first-use experience.

## Reliability Metrics

- **Data safety**: revisions prevent data loss from conflicts.
- **Integrity**: HMAC signatures detect corruption/tampering.

## Performance Metrics

- **List/query latency**: `GET /spaces/{id}/entries` and `POST /spaces/{id}/query` remain fast as entries scale.
- **Indexer cost**: incremental updates complete quickly and do not block the UI.

## Developer Experience Metrics

- **Requirement traceability**: every REQ-* maps to tests (and tests map back).
- **Doc/code consistency**: feature registry entries point to real symbols.
