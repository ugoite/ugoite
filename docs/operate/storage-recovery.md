---
title: "Storage and Recovery"
description: Move Spaces safely and keep the three recovery inputs apart.
sidebar:
  order: 5
---

A portable recovery preserves three things separately: the Space prefix, the
Node control-store prefix, and the node secret. A `/data` directory copy alone
is incomplete when the control store or secret lives elsewhere.

## Procedures

- Complete-prefix moves, verification, and cleanup in
  [Space settings and storage](../guide/operate/storage/index.md),
  [space settings storage](../guide/operate/storage/space-settings-storage.md),
  and [storage cleanup](../guide/operate/storage/storage-cleanup.md).
- Node administration boundaries in
  [node administration](../guide/operate/server/node-administration.md).

## What became durable?

The Space prefix with its Catalog Head, publication chain, Iceberg objects, and
authorization state. The control-store prefix and node secret are separate
recovery inputs.

## Related

- [Operate overview](../guide/operate/index.md)
- [Troubleshooting](troubleshooting.md) for missing Spaces and rejected Entries.
