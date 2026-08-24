---
title: "Operations"
sidebar:
  order: 4
---

> Release note: Node account, Passkey, and session details on this page are
> future/reference architecture, not supported v0.1 product promises.

```bash
export UGOITE_NODE_SECRET_KEY="$(head -c 32 /dev/urandom | base64)"
docker compose up -d
docker compose ps
docker compose logs -f ugoite
```

Resolve the source Compose port with `docker compose port ugoite 8000`; release
Compose uses `${UGOITE_PORT:-8000}`.

## Backup

Recovery has three inputs. Keep them identifiable instead of treating them as
one portable directory.

### Space storage: portable

For every Space, preserve the complete prefix in its configured OpenDAL
backend. A Space is an Iceberg namespace: its Catalog Head, reachable
publication records, Iceberg metadata, manifests, data files, entries, assets,
saved SQL, and Space authorization state belong together. Stop writes and use
either a complete prefix copy or the backend's native consistent snapshot.
Catalog Head object versioning is required for disaster recovery. Test that the
restored Space opens with the current Ugoite format; older or incomplete layouts
are unsupported.

Do not select files from an object listing, rebuild Iceberg metadata, or
reconstruct Catalog Head. Head and the Iceberg-owned immutable files are the
authority and must be captured by the complete prefix/snapshot operation.

### Node control store: node-local

The default local layout stores Node accounts, Passkeys, sessions, credentials,
bindings, and related control state below `_ugoite/nodes/{node-id}`. If
`UGOITE_NODE_CONTROL_URI` is set, the control store is instead a separate
OpenDAL backend/prefix. Back up that complete configured backend/prefix as a
separate recovery input. Node control state is not part of a portable Space
move.

### Node secret: separate

Preserve the value supplied by `UGOITE_NODE_SECRET_KEY` or the file supplied by
`UGOITE_NODE_SECRET_FILE` outside the control-store namespace. A `/data`
snapshot does not include an environment value or a separately mounted secret.
Without the same node secret, encrypted control state cannot be recovered.

When all storage is in the default local layout, `/data` contains the Space and
default Node control-store files. `/data` is a complete recovery set only when
the node secret is also retained in that deployment layout; a separate control
backend or external secret always needs its own backup.

### Restore and move

Restore each configured backend/prefix while writes are stopped, restore the
node secret before starting the node, and verify `/health`, authentication,
Space listing, and a representative read/write/restore path. Moving a Space
moves the Space prefix only. Re-establish node-local account bindings on the
destination through normal setup. If a local `POST /spaces` request fails after
the Space scaffold or owner is durable, retry the same slug as the Node
administrator; the create path reuses that immutable Space and repairs its
missing Node binding instead of creating a second Space.

## Upgrade

1. Back up every configured Space prefix, the Node control-store prefix, and the
   node secret using the boundaries above.
2. pull/build the new single image;
3. start it against the same mount;
4. verify `/health`, login, Space listing, and a representative
   read/write/restore path.

AssetText refresh admission markers are an internal storage protocol. During a
rolling upgrade, stop old server workers before allowing the new process to
drain refresh markers; old workers may still emit the legacy fixed-name marker
without participating in the new admission lock. After the old workers are
drained, run `ugoite index run` once per affected Space if `index stats` reports
stale derived state.

The repository does not require a separate worker, queue, or database service.
