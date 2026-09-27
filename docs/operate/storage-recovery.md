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

For every Space, preserve the complete configured prefix: Catalog Head,
reachable publication chain, Iceberg metadata, manifests, data files, Entries,
Forms, Assets, saved SQL, and Space authorization state. Use a complete prefix
copy or the storage backend's native consistent snapshot while writes are
stopped.

Do not choose files from an object listing, rebuild Iceberg metadata, or
reconstruct the Catalog Head. A complete Space prefix is the portable move unit
and remains authoritative after a move.

### Node control-store prefix: node-local control state

The default local layout stores accounts, Passkeys, sessions, credentials, and
bindings below the node control prefix. If `UGOITE_NODE_CONTROL_URI` points to
another OpenDAL backend, back up that complete configured prefix separately. It
is not part of a portable Space move.

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
3. For a CLI-created portable Space moved to a new Node, copy only its
   `spaces/<SPACE_UID>` prefix; leave the source Node control store and Node
   secret behind. To restore the original Node instead, restore the complete
   configured control-store prefix and the same Node secret.
4. Start the destination. For a moved unclaimed Space, complete the new Node's
   normal one-time Passkey setup; setup claims the existing Space UID and
   preserves its Forms, Entries, and append-only history. Before setup,
   anonymous API access remains denied. Malformed, partially bootstrapped, or
   owner-backed ACL state is rejected for operator investigation. Then run the
   [health and diagnostics](health-diagnostics.md) checks.
5. Verify authentication, Space listing, representative Entry reads and writes,
   history, and restore before deleting the old copy.

Keep the source copy stopped while the destination is in use. Running both
copies as writable Nodes is not supported. The Space's HMAC key is stored in its
metadata and travels with the Space prefix. A Space-prefix move does not restore
Node accounts, Passkeys, sessions, bindings, or other Node control state; a
complete Node recovery separately requires the configured control-store prefix
and the same Node secret.

## Verify recovery

For a local core context, inspect the Space before and after copying it:

```bash
ugoite --context local space verify --deep --format json
```

`space verify` is read-only. It checks Space identity and version, the Catalog
Head and publication chain, Form identities, Entry revision chains and
integrity values, append-only audit links, and authorization state in a
separate Node-owned section. `--deep` reads each referenced Asset and checks
its SHA-256 against the Entry reference; without it, the verifier checks that
the object exists and has the recorded size.

The JSON report uses `schema_version: 1` and one of four statuses: `valid`,
`valid_with_rebuildable_derived_state`, `invalid`, or `incomplete`. An
incomplete report means required evidence could not be checked; it does not
mean the Space is valid. Invalid and incomplete reports exit nonzero. Derived
indexes remain disposable and the verifier never repairs them. Use
`ugoite index run` separately when a derived index needs rebuilding.

The top-level status and `valid` field describe portable Space Knowledge.
Node-owned authorization state has its own `sections.authorization.status` and
does not change the Space Knowledge result. The CLI exits nonzero if either
Knowledge verification or the authorization section is invalid or incomplete.

The verifier currently requires a local core context. It does not verify the
Node control-store prefix or Node secret; keep those as separate recovery
inputs as described above.

A recovery is successful when the restored Space opens, its Forms and Entries
read back, Asset references resolve, and a representative edit followed by
history and restore produces a new append-only revision. Search indexes and SQL
sessions may be rebuilt; they are not evidence that the authoritative Space is
intact.

The repository's `portable-space` E2E seeds a Space through the local CLI in a
temporary source root, verifies it, copies only `spaces/<SPACE_UID>` into an
empty destination root, and then starts a fresh Node there. It reads the
imported Form, Entry history, Asset bytes, and Saved SQL, appends a new Change,
checks that the imported revision is still present, and verifies the claimed
Space afterward. SHA-256 values for the preexisting immutable prefix objects
must remain unchanged. It also gives `space verify --deep` a deliberately partial
prefix and checks that verification rejects it without writing into it. Run it
with `bash e2e/scripts/run-e2e.sh portable-space` for the direct-process lane or
`bash e2e/scripts/run-e2e-compose.sh portable-space` for the container lane.
The server startup test `startup_resumes_portable_space_claim_without_changing_history`
covers recovery of a persisted pending claim across restart; malformed and
foreign authorization fixtures are reported separately by the verifier.

## Test a storage backend before trusting it

Test connectivity with the current binary before pointing a Space at a new
backend. `space test-connection` takes a storage config JSON document and
reports whether the backend is reachable; run
`ugoite space test-connection --help` for the exact JSON shape of the installed
version. A passing connection test does not validate Space content: reopen the
Space and complete the verification above before deleting the old copy.

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
