---
title: 'Directory structure'
---

`directory-layout.yaml` is the machine-readable inventory for repository-owned
paths. Apache Iceberg owns table data and metadata locations beneath the Space;
documentation and tests must not depend on its internal filenames. Every
Iceberg-persisted location uses the canonical logical coordinate
`ugoite://{space_uid}/{space-relative-key}`. The active operator resolves that
coordinate to the Space prefix at I/O time; its physical warehouse URI is never
written into portable Space or Iceberg metadata.

## Workspace layout

```text
spaces/
  {space_id}/
    meta.json
    settings.json
    _ugoite/
      catalog/
        head.json
        publications/
      derived/
        relations/{relation_id}/
          head.json
          builds/{build_id}/                       # Iceberg-owned current/garbage builds
    forms/
    assets/
    sql_sessions/

_ugoite/
  space-bindings/{space_id}.json    # Node-local Space locator
  space-patches/{space_id}.json     # Node-local patch journal

users/
  {sha256(user_id)}/
    preferences.json

response_hmac/
  default.json                 # Node-default response-signing material
```

## Space bootstrap

`create_space` creates the Space directory, the authoritative child directories,
`meta.json`, and `settings.json`, then bootstraps the `Entry` Form. Derived
relation directories are lazy and are not part of bootstrap.

`meta.json` currently contains:

```json
{
  "schema_version": 3,
  "space_id": "space-main",
  "space_uid": "019c1234-5678-7abc-8def-0123456789ab",
  "slug": "space-main",
  "id": "space-main",
  "name": "space-main",
  "created_at": 1762000000.123,
  "hmac_key_id": "key-...",
  "hmac_key": "base64-encoded-secret",
  "last_rotation": "2026-03-11T10:00:00Z"
}
```

The typed `SpaceMeta` view exposes only portable identity and creation metadata;
it never contains a physical backend binding. The raw runtime response may
merge `settings.json` and a Node-local `storage_config` binding. The binding is
stored outside the `spaces/{id}` prefix and is not part of a Space copy.

`settings.json` starts as:

```json
{"default_form": "Entry"}
```

Portable membership, principal, policy, human-approval, approval-audit-outbox,
and authorization-audit state is stored in
`spaces/{space_id}/security/principals.json`. Membership-shaped keys in
`settings.json` are legacy markers and are rejected rather than upgraded. UI
locale and selected-Space preferences are user-scoped and belong in
`users/{sha256(user_id)}/preferences.json`, not Space settings.

## Lazy paths

| Trigger | Path | Current meaning |
|---|---|---|
| Response signing | `spaces/{space_id}/hmac.json` | response-signing key material |
| Node response signing | `response_hmac/default.json` | Node-default response-signing key material; not part of a Space export |
| SQL session creation | `spaces/{space_id}/sql_sessions/{session_id}/meta.json` | query/session metadata |
| Asset upload | `spaces/{space_id}/assets/{asset_id}` | binary object; the key is derived from the stable asset ID |
| Derived rebuild | `spaces/{space_id}/_ugoite/derived/relations/{relation_id}/head.json` | current build coordinate; non-authoritative and replaceable |
| Preference update | `users/{sha256(user_id)}/preferences.json` | portable user UI preferences |

## Catalog and Iceberg-managed Form storage

A Form definition and its single append-only revision table are stored through
the Catalog-backed Rust Iceberg layer. The table identity is derived from the
stable Form UUID, not its display name. The repository specifies logical schema
and table-property keys but leaves Iceberg metadata/data filenames unspecified.

`_ugoite/catalog/head.json` is the only mutable catalog authority. It carries
the Space identity, format version, catalog/form-registry generations, Form table
coordinates, the complete active Pin map, checksum, and current publication
coordinate. A process opens a Space by reading this Head exactly with its OpenDAL
ETag and loading only the referenced immutable Iceberg metadata.

`_ugoite/catalog/publications/<generation>-<encoded-command-id>.json` records the
complete next Head, its checksum, the preceding Head and publication
coordinates, command identity/digest, optional immutable Change metadata, and
affected-table Iceberg coordinates.
Records are immutable and authoritative only when reachable from Head. Listing
objects never reconstructs catalog state or publication order. Missing/corrupt
Head or reachable publication evidence is an explicit failure; old pointer
manifests and layout readers are unsupported rather than migrated.

Pins are stored inline in `_ugoite/catalog/head.json`. Each active name carries
only a `PublicationRef` (generation, logical publication URI, and publication
checksum) plus creator metadata. A Pin is valid only when that exact
publication is reachable from the current Head; no separate coordinate file or
coordinate registry is consulted.

Revision rows contain stable entry/revision identity, optimistic version and
parent lineage, operation/tombstone state, commit time, original creator
(`author`), latest mutation actor (`updated_by`), delete actor (`deleted_by` on
a tombstone), and other provenance, Form version, typed Form columns, and
extension metadata. A projection of the
latest non-conflicting revision provides current Entry responses.

## Portability

A complete Space prefix can be backed up or moved as a user-owned, portable
unit through storage controlled by the user or their chosen operator. Stop
writes and use a complete prefix copy or backend-native
consistent snapshot; object listing must not be used to reconstruct Catalog
Head or Iceberg state. Node control state and Space storage bindings are
node-local and are not part of a
Space move. A complete Node recovery set also preserves the configured Node
control-store prefix, the Node-default `response_hmac/default.json`, and the
node secret separately. PATCHing a Space storage binding changes the Node-local
locator only; it does not move existing files.
