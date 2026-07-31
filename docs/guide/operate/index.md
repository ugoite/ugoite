---
title: "Operate Ugoite"
description: Keep a running Ugoite node healthy, secure, and portable.
sidebar:
  label: "Overview"
  order: 1
---

Operations start by naming the recovery inputs. Ugoite does not make one
directory the portable authority for every kind of state:

- **Space storage** is the portable unit. Each Space is an Iceberg namespace
  reached through its configured OpenDAL backend. Its Catalog Head, publication
  chain, Iceberg metadata, manifests, data files, and Space authorization state
  move together.
- **Node control state** is node-local. The default local layout keeps it below
  `_ugoite/nodes/{node-id}` beside the local Space storage, while
  `UGOITE_NODE_CONTROL_URI` can place the complete control-store prefix in a
  different OpenDAL backend. It is not part of a portable Space move.
- **The node secret** is a separate recovery input. `UGOITE_NODE_SECRET_KEY`
  or `UGOITE_NODE_SECRET_FILE` supplies encryption-root material outside the
  control-store namespace. Losing it makes encrypted Node control state
  unusable.

The usual `/data` mount is enough for the storage backends only when the
deployment uses the default local layout and the node secret is preserved with
that layout. An environment variable or mounted secret is not copied by a
`/data` snapshot, and a separate `UGOITE_NODE_CONTROL_URI` backend must be
backed up separately.

## Find the procedure

- **Server operations:** use [health checks](server/backend-healthcheck.md),
  [node administration](server/node-administration.md), and the
  [operations runbook](server/operations.md).
- **Authentication and automation identities:** read
  [Authentication and authorization](auth/index.md), then the
  [Agent identities](auth/service-accounts.md) reference.
- **Spaces and storage:** use [Space settings and storage](storage/index.md) for
  moves, migrations, and cleanup.

## Safe operating sequence

1. Stop writes, then back up every configured Space backend/prefix and the
   complete Node control-store backend/prefix; preserve the node secret
   separately.
2. Verify `/health`, authentication, Space listing, and a representative
   read/write/restore path after a change.
3. For a Space move, move only the complete Space prefix. Do not reconstruct
   Iceberg files or Catalog Head from an object listing.
