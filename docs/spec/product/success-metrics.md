# Success Metrics

These metrics help evaluate whether Ugoite is delivering on its principles
(**Low Cost**, **Easy**, **Freedom**).

## Product Metrics

- **Time-to-first-entry**: new user can create a space + first entry quickly.
- **Time-to-first-structured-field**: user can define a Form and see extracted fields.
- **Search usefulness**: keyword search returns expected results with low latency.

## Reliability Metrics

- **Data safety**: revisions prevent data loss from conflicts.
- **Integrity**: HMAC signatures detect corruption/tampering.

## Performance Metrics

- **List/query latency**: `GET /spaces/{id}/entries` and `POST /spaces/{id}/query` remain fast as entries scale.
- **Indexer cost**: incremental updates complete quickly and do not block the UI.

## Developer Experience Metrics

- **Requirement traceability**: every REQ-* maps to tests (and tests map back).
- **Doc/code consistency**: feature registry entries point to real symbols.

