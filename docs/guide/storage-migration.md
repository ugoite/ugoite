---
title: 'Storage migration'
---

1. Record the running version, Catalog configuration, and source snapshot IDs.
2. Stop writes and back up the complete workspace/Catalog metadata.
3. Generate a versioned migration manifest and run verification in dry-run mode.
4. Verify Form/Field ID mapping, entry/revision/tombstone counts, latest values,
   timestamps, author/provenance, asset/ACL references, and representative queries.
5. Materialize new `form_<uuid>` tables without deleting or overwriting source tables.
6. Re-run the manifest comparison and only then switch the Catalog pointer.
7. Verify `/health`, authentication, membership, history, assets, SQL, delete,
   restore, and a concurrent-write conflict.
8. Retain the source snapshot/backup read-only until the rollback window closes.

Rollback switches the Catalog back to the recorded source snapshot or restores
the complete backup; it never attempts a reverse row rewrite. Do not migrate
through historical split-stack paths or a separate authoritative SQL database.
