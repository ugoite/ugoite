---
title: "Engineering Principles"
description: Why the repository looks the way it does and how to change it safely.
sidebar:
  order: 2
---

Engineering Principles explain why the architecture stays stable while product
discovery continues. Read them after
[Product Principles](../vision/principles.md) and before the implementation docs
in [Architecture](../architecture/index.md).

## Foundation is stable

The v0.1 Foundation of operator-owned Spaces, portable append-only history, and
thin adapters is frozen. Product work completes and clarifies that foundation
instead of churning it.

## Product discovery over architecture churn

New understanding becomes better task pages, validation messages, recovery
flows, and documentation first. Crate boundaries move only when a cross-surface
outcome requires it.

## One application model

Domain and use-case behavior lives once in reusable Rust crates. CLI, server
(including MCP via the server), WASM, and browser transport code remain adapters
and must not introduce a parallel persistence or authorization authority.

## Shared Rust semantics

Portable behavior is shared through Rust: `ugoite-domain` for types and
validation, `ugoite-api-client` for the transport-neutral operation protocol,
and `ugoite-core` for application behavior. See
[Repository Map](repository-map.md).

## Thin adapters

`ugoite-server`, `ugoite-cli`, and `ugoite-wasm` translate between the shared
model and their runtime. The frontend uses the portable protocol instead of
reconstructing endpoint semantics. Do not duplicate REST semantics across
adapters.

## Cross-surface outcomes

A Knowledge operation keeps the same meaning in Browser, CLI, REST, MCP, and
Konase-assisted workflows. When surfaces disagree, fix the shared semantics
rather than papering over one surface.

## No hidden authority

Search indexes, SQL sessions, browser sessions, model context, agent memory, and
render caches are derived or disposable. Only the Space prefix, the Node
control-store prefix, and the node secret together form a recovery set.

## Prefer completion over breadth

Finish the Golden Journey of Space, Form, Entry, search, history, and restore
before adding new capability. A completable product beats a broad but
inconsistent one.

## Evidence before claims

Behavior changes ship with implementation and verification evidence. Missing or
ambiguous evidence becomes an explicit gap, never a narrowed requirement. Mitase
validates declared relationships and evidence; it does not execute tests or
become a second Knowledge authority.

## Documentation is a product surface

Product prose lives once under `docs/` and is rendered directly by Starlight.
Generate facts from code and existing machine authority, write understanding in
prose, and test contracts rather than documentation implementation. See
[Documentation Development](documentation.md).
