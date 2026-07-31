---
title: "Operations"
---

```bash
export UGOITE_NODE_SECRET_KEY="$(head -c 32 /dev/urandom | base64)"
docker compose up -d
docker compose ps
docker compose logs -f ugoite
```

Resolve the source Compose port with `docker compose port ugoite 8000`; release
Compose uses `${UGOITE_PORT:-8000}`.

## Backup

The directory mounted at `/data` is authoritative. Quiesce writes or take a
storage-consistent snapshot, copy the complete tree, and test that it opens with
a compatible Ugoite version. Entries, forms, history, assets, saved SQL, and
membership data must be preserved; indexes can be rebuilt.

## Upgrade

1. Back up `/data`.
2. pull/build the new single image;
3. start it against the same mount;
4. verify `/health`, login, Space listing, and a representative
   read/write/restore path.

The repository does not require a separate worker, queue, or database service.
