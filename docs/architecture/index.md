---
title: "Architecture"
description: How Ugoite keeps operator-owned Spaces portable while adapters stay thin.
sidebar:
  order: 1
---

This section explains how Ugoite is put together and why its boundaries look the
way they do. Read it after [Core concepts](../guide/start/concepts.md) if you
are new: architecture names the owners and adapters around the Space model.

## Read the architecture map

- **Principles:** why operator-owned files, append-only history, and thin
  adapters are non-negotiable in [Architecture principles](principles/index.md).
- **Boundaries:** where portable Rust behavior ends and runtime-specific
  transport begins in [System boundaries](boundaries/index.md).
- **Security:** how node identity and Space authorization stay separate in
  [Security architecture](security/index.md).
- **Release:** what is shipped now and what remains future scope in
  [Release architecture](release/index.md).
- **API:** the REST, MCP, and operator-surface boundaries in
  [API architecture](api/rest.md).
- **Data model:** portable Space persistence, forms, entries, and storage
  coordinates in [Data model architecture](data-model/overview.md).
- **Testing:** repository validation, CI lanes, and release-grade checks in
  [Testing architecture](testing/strategy.md).
- **Quality:** error contracts and fail-closed behavior in
  [Quality architecture](quality/error-handling.md).
- **Normative contracts:** implementation-facing architecture specifications
  live under [architecture contracts](contracts/overview.md).

The groups are intentionally explanatory entry points. The executable
requirements and implementation references remain in `docs/spec/`.
