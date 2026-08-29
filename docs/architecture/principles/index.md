---
title: "Architecture principles"
description: The invariants that keep Ugoite's Knowledge portable while Work and Experience remain replaceable.
sidebar:
  label: "Principles"
  order: 1
---

This group answers why Ugoite has a portable Space directory, append-only
history, derived indexes, and several thin runtime adapters. It also defines
which things persist, which things may disappear, and which things must never
become a second Knowledge authority.

- [Architecture North Star](north-star.md) describes the product and storage
  invariants.
- [Knowledge, Work, and Experience](knowledge-work-experience.md) explains the
  boundary between durable Knowledge, disposable Work, and replaceable tools.
- [Control surfaces](control-surfaces.md) maps the operator, CLI, server,
  browser, and specification surfaces.

After this group, read [System boundaries](../boundaries/index.md) to see where
those principles become package and runtime boundaries.
