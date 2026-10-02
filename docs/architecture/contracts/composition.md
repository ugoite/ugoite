---
title: Composition contract
description: The portable document and semantic boundaries for Composition v1.
---

# Composition contract

**Status:** accepted semantic boundary; the parser implementation and resource
limits are selected from measured evidence in the Composition implementation.

## Portable document

A Composition is a user-owned Knowledge document represented as one Entry
revision in its Space. The Space remains its authority. History, restore,
receipts, and stale-write conflicts use the existing Entry revision contract;
no separate database or hidden catalog becomes authoritative.

The exchange format is a restricted `.ugcomp.yaml` document at format version
`1`. Its semantic kind is `dashboard` for v0.2.2. The document describes its
name, typed parameters, sources, components, and sections. A source identifies
either an Entry query template or an exact Saved SQL revision. Components refer
to named sources and sections group named components.

The complete illustrative document is maintained at
[`monthly-expense.ugcomp.yaml`](../../../crates/ugoite-domain/tests/fixtures/composition/monthly-expense.ugcomp.yaml).
It is a contract fixture for domain, parser, canonicalization, and WASM parity
tests, not an assertion that persistence or rendering is already shipped.

## Semantic authority

Rust typed domain values are the semantic authority. Native and WASM callers
use the same parser, validator, canonicalizer, fingerprint, and diagnostic code
definitions. Browser TypeScript does not implement a second Composition
parser, resolver, query engine, or semantic validator.

The v1 syntax rejects duplicate mapping keys, multiple documents, anchors and
aliases, merge keys, custom tags, implicit timestamp interpretation, and
unknown fields. Resource limits apply to input bytes, nesting depth, and
collection lengths. Their numeric values are chosen only after native and
`wasm32-unknown-unknown` measurements; they are not implied by this contract
document.

Canonical YAML uses schema field order, block collections, two-space
indentation, LF line endings, and no BOM. Canonical output is derived from the
normalized typed value; comments are not preserved. Semantic fingerprints hash
the normalized semantic value, so formatting changes do not change identity.

## Failure and recovery boundary

Unsupported format versions and invalid documents return stable diagnostic
codes. They do not make raw inspection, export, or revision history unavailable.
Broken source references are execution failures; they do not erase the stored
document.

Entry query templates resolve to the existing bounded EntryQuery contract.
Saved SQL references identify an exact Entry and Revision; they never fall
back to the latest revision. Current authorization is re-evaluated by the
existing query paths.

Browser page, scroll, result, and cache state are transient Work. They are not
fields of a Composition and do not become Space Knowledge.
