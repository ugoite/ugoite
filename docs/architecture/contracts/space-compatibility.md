---
title: "Space compatibility contract"
---

Ugoite Product Version, Space compatibility Version, and internal physical or
storage representation are separate concepts. A Product 0.2 release can and
normally will open a Space 0.1. A physical layout change alone does not bump
the Space compatibility Version.

## Current authority

The only durable Space compatibility identity is:

```json
{ "space_version": "0.1" }
```

`ugoite-domain` owns the parser and supported-version classifier. Iceberg,
Core, the server, and the local CLI reuse that classifier; they do not infer a
Space Version from a Product Version, an Iceberg schema ID, or a subsystem-local
`schema_version`.

The current supported set is exactly `0.1`. The compatibility Version changes
only when durable Knowledge meaning changes incompatibly. Internal physical
encoding, table metadata, and other subsystem-local schema versions may evolve
without changing the Space Version.

## Fail-closed open order

Space access follows this order:

1. read minimal bootstrap metadata;
2. classify `space_version`;
3. reject missing, malformed, or unsupported values;
4. validate the current structural contract;
5. read Knowledge and permit an authoritative mutation only after the checks.

Unsupported values return the stable typed error `UNSUPPORTED_SPACE_VERSION`
with the detected value and supported values in its detail. An unsupported
Space is not repaired, normalized, or migrated during open. In particular,
`schema_version: 3` without `space_version` is not treated as Space 0.1, and
`space_version` is never silently defaulted when missing.

## Compatibility evidence

`fixtures/spaces/0.1/` is the frozen historical bootstrap fixture. The
compatibility regression opens that fixture, reads its Form and Entries,
performs an append-only update, closes and reopens the service, and verifies
identity, current values, and revision history. Negative cases cover unknown,
future, missing, malformed, and schema-only metadata; unsupported open is
asserted to leave the authoritative tree unchanged.

## Fixture layout

The expected `space-compat-check` fixture layout is explicit: the fixture
root (`fixtures/spaces/`) contains canonical Space version directories only
(`0.1` today). A non-directory entry or a non-canonical directory name is
invalid test input, not an alternate layout, and the check fails closed
instead of skipping it. Likewise each version directory holds canonical
Space bootstrap fixtures only; stray files are not silently treated as
Spaces. The regression test pins rejection of non-directory and
non-canonical entries so drift between code and fixtures can never hide.

There is no migration registry, generic migration trait, migration graph,
downgrade framework, or automatic migration-on-open. If a future incompatible
Space Version becomes necessary, its explicit conversion design and evidence
must be reviewed as a separate change.
