---
title: "Architecture principles"
description: The invariants that shape Ugoite's local-first product and codebase.
sidebar:
  label: "Principles"
  order: 1
---

This group answers **why** Ugoite has a portable Space directory, append-only
history, derived indexes, and several thin runtime adapters.

- [Architecture North Star](north-star.md) describes the product and storage
  invariants.
- [Control surfaces](control-surfaces.md) maps the operator, CLI, server,
  browser, and specification surfaces.

After this group, read [System boundaries](../boundaries/index.md) to see where
those principles become package and runtime boundaries.
