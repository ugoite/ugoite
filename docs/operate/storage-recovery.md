---
title: "Storage and Recovery"
description: Back up complete Space and node recovery inputs and verify a restore safely.
sidebar:
  order: 5
---

Ugoite has one portable Knowledge authority and three recovery inputs. Treat
them as distinct so a deployment restore does not accidentally omit node
identity or the encryption root.

## The recovery set

### Space prefix: portable Knowledge

For every Space, preserve the complete configured prefix: Catalog Head, reachable
publication chain, Iceberg metadata, manifests, data files, Entries, Forms,
Assets, saved SQL, and Space authorization state. Use a complete prefix copy or
the storage backend's native consistent snapshot while writes are stopped.

Do not choose files from an object listing, rebuild Iceberg metadata, or
reconstruct the Catalog Head. A complete Space prefix is the portable move unit
and remains authoritative after a move.

### Node control-store prefix: node-local control state

The default local layout stores accounts, Passkeys, sessions, credentials, and
bindings below the node control prefix. If `UGOITE_NODE_CONTROL_URI` points to
another OpenDAL backend, back up that complete configured prefix separately.
It is not part of a portable Space move.

### Node secret: separate encryption root

Preserve the value supplied by `UGOITE_NODE_SECRET_KEY` or the file supplied by
`UGOITE_NODE_SECRET_FILE` outside the control-store namespace. A `/data`
snapshot does not include an environment value or a separately mounted secret.
Without the same secret, encrypted control state cannot be recovered.

## Backup procedure

1. Stop or quiesce all writers.
2. Record every configured Space backend/prefix and the Node control-store
   backend/prefix.
3. Capture each complete Space prefix and the complete control-store prefix.
4. Preserve the node secret in the deployment secret store or another
   owner-controlled backup.
5. Record the Ugoite version and configuration needed to open the restore.

The usual `/data` mount contains both Space storage and the default local Node
control store only when the default layout is in use. It is a complete recovery
set only when the node secret is retained too.

## Restore or move a Space

1. Stop writes on the source and destination.
2. Copy or restore the complete Space prefix without changing object names or
   reconstructing derived files.
3. Restore the node control store and node secret when restoring the deployment,
   or perform normal setup when moving only a Space to another node.
4. Start the destination and run the [health and diagnostics](health-diagnostics.md)
   checks.
5. Verify authentication, Space listing, representative Entry reads and writes,
   history, and restore before deleting the old copy.

## Verify recovery

A recovery is successful when the restored Space opens, its Forms and Entries
read back, Asset references resolve, and a representative edit followed by
history and restore produces a new append-only revision. Search indexes and SQL
sessions may be rebuilt; they are not evidence that the authoritative Space is
intact.

## What remains durable?

Space Catalog Head, reachable publications, Entry and Form history, Asset bytes,
saved SQL, memberships, ACLs, attribution, and authorization audit state remain
Knowledge. Search indexes, query sessions, previews, open tabs, login sessions,
and execution progress are derived or node-local state and may be recreated.

## If recovery fails

Keep the original complete prefixes and node secret unchanged. Report an
unsupported or incomplete layout explicitly; do not rewrite it in place. A
missing derived relation is repaired with the supported index command after the
authoritative Space opens. Use [Troubleshooting](troubleshooting.md) for
symptom-first diagnosis.

## Related

- [Configure](configure.md)
- [Upgrade and Compatibility](upgrade-compatibility.md)
- [Space compatibility contract](../architecture/contracts/space-compatibility.md)
