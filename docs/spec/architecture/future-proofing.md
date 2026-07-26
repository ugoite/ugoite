---
title: 'Future-proofing'
---

Future work must preserve current invariants rather than create a second product model.

## Browser-local runtime

A future browser adapter may persist a complete Space locally and optionally synchronize with a relay. It must define migrations, conflict handling, integrity, offline behavior, and recovery before being marked implemented.

## Storage portability

`ugoite-storage` uses OpenDAL and Space-relative paths so local files and
compatible object stores share one model. Physical Iceberg internals remain in
`ugoite-iceberg`; Core consumes domain-facing storage/query contracts only.

## AI surfaces

MCP may add resources, prompts, and tools only with explicit authorization and untrusted-content framing. AI functionality must not obtain hidden ownership over user data.

## Compatibility

Protocol versions and OpenAPI snapshots require tests and explicit documentation.
The current internal pre-release Space format version remains unchanged during
the destructive architecture transition: superseded layouts fail explicitly
instead of acquiring migration readers or compatibility flags.
