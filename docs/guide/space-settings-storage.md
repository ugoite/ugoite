# Space Settings & Storage

Use this guide the first time the browser shows a space storage summary or a
**Settings** link and you want to know whether you actually need to change
anything.

## What the storage summary is telling you

The dashboard storage summary answers "where is this space writing right now?"
It reflects the current storage topology reported by the backend for the active
space before you edit any future targets.

- The label tells you whether the current topology is local filesystem, remote
  object storage, or another backend-managed target.
- The URI shows the current root when the backend can report it.
- The summary is there so you can review the active setup before opening
  **Settings**.

## What the Saved Storage URI means today

In **Space Settings**, the **Saved Storage URI** field records connector
metadata for a future migration target.

- `file://` keeps the plan local-first and machine-owned.
- `s3://` records a remote object storage target that adds credentials, network
  reachability, and usage-cost questions.

Today that field is metadata only: saving it does **not** move existing entries
or assets, and it does **not** reroute live writes away from the storage summary
shown above.

## When to leave the defaults alone

If the current space is already writing where you expect and you are just
getting started, leave the defaults alone.

Good reasons to keep the defaults:

- you are creating the first space or first entry
- local filesystem storage already matches the low-cost, local-first path you
  want
- you do not have a concrete migration plan yet

## When to open Settings on purpose

Open **Settings** when you want to:

1. review the current storage summary before making an operational decision
2. test a future connector target with **Test Connection**
3. save connector metadata for a planned migration
4. rename the space or inspect other space-level configuration

## What to read next if you are changing storage

If you are preparing an actual cutover, continue with the
[Storage Migration Guide](storage-migration.md). If you are cleaning up old
local state instead of moving it, use
[Storage Cleanup](storage-cleanup.md).
