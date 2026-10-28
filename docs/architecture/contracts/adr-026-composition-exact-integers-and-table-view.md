---
title: "ADR-026 — Exact Composition integers and Table column presentation"
---

**Status: Accepted for the unreleased ADR-025 variable graph.**

## Context

The SQL executor supports signed 64-bit integer values. ADR-023 restricted
Composition integers to JavaScript's safe integer interval to prevent silent
rounding. That restriction prevents valid SQL values from being displayed or
bound through the graph. A Form-backed Table also needs understandable column
headings without changing its SQL result or exposing technical columns by
default.

## Decision

For ADR-025's current variable graph only, replace ADR-023's 17-byte integer
token limit and safe-integer interval with an ASCII `[+-]?[0-9]+` token of at
most 64 bytes and the full signed 64-bit range. Leading zeroes remain allowed;
whitespace, decimal points, exponents and out-of-range values remain invalid.
The 64-byte numeric token limit was already used by the current graph. This
decision does not change other types or rewrite the historical ADR-023 model.

Canonical Composition and evaluation protocol values remain JSON numbers,
and YAML retains numeric values. Browser code preserves the exact source token
only for fields known to be typed Composition integers, and writes exact numeric
tokens. It must reject a value when the browser cannot preserve it or when an
already-rounded number is supplied; it must not silently coerce it. Opaque JSON
values do not acquire Composition integer semantics. There is no compatibility
reader, alias, or alternate canonical string representation.

A Table may carry an ordered display projection of exact result column keys and
human-readable column headings. This is presentation for result columns, not a
label on the Table component or a second query schema. Query variables still
own SQL, bindings, descriptors and results. Unknown or duplicate configured keys
must not silently retarget a different column. A Form selection can initialize
the projection with the Form's field headings while preserving its actual SQL
and authorized Form binding. Users can explicitly choose other result columns.

## Evidence and limits

Domain tests cover signed 64-bit constants, inputs, result values, and rejection
outside that range. Core binding tests cover exact query parameters. Browser
protocol and component tests cover exact numeric tokens, typed values, display
projection, and unavailable columns. Integration journeys must exercise actual
authorized execution, save and reopen; those tests are evidence for the paths
they execute, not a claim of complete SQL GUI coverage or device coverage.
