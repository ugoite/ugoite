---
title: "Spaces and storage"
description: Move, verify, migrate, and clean up operator-owned Space data safely.
sidebar:
  label: "Spaces & storage"
  order: 3
---

A Space is a portable directory, not a database row that can be replaced by a
derived index. Use this group whenever a change affects the location, contents,
or schema of Space data.

## Choose the operation

- [Space settings and storage](space-settings-storage.md) explains the portable
  boundary and connection checks.
- [Storage cleanup](storage-cleanup.md) explains what is derived and what must
  never be removed casually.
- [Storage migration](storage-migration.md) gives the manifest, verification,
  and rollback sequence for a format change.
