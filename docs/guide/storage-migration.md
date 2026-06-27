---
title: 'Storage migration'
---

1. Record the running version and configuration.
2. Stop writes.
3. Back up the complete workspace or Space directory.
4. Copy it with metadata and permissions preserved.
5. point `UGOITE_ROOT`, the Compose volume, or Helm PVC at the destination;
6. verify `/health`, authentication, membership, entries/history, assets, saved SQL, and a write/restore cycle;
7. retain the source read-only until validation completes.

The current Rust implementation opens the filesystem workspace directly. Do not migrate through historical split-stack paths or a separate authoritative SQL database.
