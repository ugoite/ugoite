---
title: "Operate Ugoite"
description: Keep a running Ugoite node healthy, secure, and portable.
sidebar:
  label: "Overview"
  order: 1
---

Operations follow one rule: the complete operator-owned data root is the source
of truth. Back up and move that boundary; treat indexes and query sessions as
derived data.

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

1. Back up the complete data root before upgrades or storage changes.
2. Verify `/health`, authentication, Space listing, and a representative
   read/write/restore path after a change.
3. Keep the node secret and the operator-owned files available together; losing
   either can make encrypted recovery material unusable.
