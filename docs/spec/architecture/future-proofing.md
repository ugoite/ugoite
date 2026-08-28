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

## Derived projections

DerivedRelation is the storage primitive for rebuildable OCR/text/embedding/
graph-style projections. A producer must publish a semantic fingerprint,
compatibility epoch, typed schema, source coordinate, and bounded rebuild
contract. New builds swap through an independent Relation Head; they
do not become Forms or alter the meaning of SpaceCheckpoint. AssetText is the
first internal example and remains a searchable-text projection rather than a
full-text inverted index.

## Reversible Knowledge history

The current publication model is the foundation for reversible Knowledge
changes. A committed immutable publication may carry one Change descriptor, and
the descriptor's Change ID is the same identity used to coordinate that
publication. History is therefore portable with the Space prefix and can be
reconstructed from the reachable chain. Runs remain correlation-only and Pins
remain exact Head-owned references to immutable publications.

Revert planning is selective and schema-aware: unchanged fields are preserved,
fields still equal to the target Change's after-value may be restored, and later
edits, deleted fields, removed Forms, and incompatible types produce explicit
conflicts. It never rewinds Head or resurrects a schema. Multi-Form atomicity,
automatic asset purging, and retention/expiration are not implied by this model.

## Compatibility

Protocol versions and OpenAPI snapshots require tests and explicit documentation.
The current internal pre-release Space format version remains unchanged during
the destructive architecture transition: superseded layouts fail explicitly
instead of acquiring migration readers or compatibility flags.
