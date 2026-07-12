---
title: 'Directory structure'
---

`directory-layout.yaml` is the machine-readable inventory for repository-owned paths. Apache Iceberg owns the subtree beneath `forms/`; documentation and tests must not depend on its internal filenames.

## Workspace layout

```text
spaces/
  {space_id}/
    meta.json
    settings.json
    forms/
    assets/
    materialized_views/
    sql_sessions/

users/
  {sha256(user_id)}/
    preferences.json
```

## Space bootstrap

`create_space` creates the Space directory, all four child directories, `meta.json`, and `settings.json`, then bootstraps the `Entry` Form.

`meta.json` currently contains:

```json
{
  "id": "space-main",
  "name": "space-main",
  "created_at": 1762000000.123,
  "storage": {"type": "local", "root": "/data"},
  "hmac_key_id": "key-...",
  "hmac_key": "base64-encoded-secret",
  "last_rotation": "2026-03-11T10:00:00Z"
}
```

The typed public `SpaceMeta` view exposes the identity, timestamp, and storage object; the raw Space response also merges `settings.json`.

`settings.json` starts as:

```json
{"default_form": "Entry"}
```

Membership operations add `members`, `member_invitations`, and `membership_version` lazily. UI theme, locale, and selected-Space preferences are user-scoped and belong in `users/{sha256(user_id)}/preferences.json`, not Space settings.

## Lazy paths

| Trigger | Path | Current meaning |
|---|---|---|
| Response signing | `spaces/{space_id}/hmac.json` | response-signing key material |
| Saved SQL/session use | `spaces/{space_id}/materialized_views/{sql_id}/meta.json` | metadata placeholder; no result rows |
| SQL session creation | `spaces/{space_id}/sql_sessions/{session_id}/meta.json` | query/session metadata |
| Asset upload | `spaces/{space_id}/assets/{asset_id}_{safe_name}` | binary object |
| Preference update | `users/{sha256(user_id)}/preferences.json` | portable user UI preferences |

## Iceberg-managed Form storage

A Form definition and its single append-only revision table are stored through
the Catalog-backed Rust Iceberg layer. The table identity is derived from the
stable Form UUID, not its display name. The repository specifies logical schema
and table-property keys but leaves Iceberg metadata/data filenames unspecified.

Portable local Spaces keep `forms/catalog-pointers.v1.json` as the explicit
Catalog pointer manifest. A new process registers tables from this file; it
never scans metadata filenames or selects the numerically greatest file.

Revision rows contain stable entry/revision identity, optimistic version and
parent lineage, operation/tombstone state, commit time, author and provenance,
Form version, typed Form columns, and extension metadata. A projection of the
latest non-conflicting revision provides current Entry responses.

## Portability

A complete storage root can be backed up or moved as an operator-controlled unit. Copying only one Space preserves that Space’s content and metadata but not user-scoped preferences stored under `users/`. Storage migration is an operator procedure; PATCHing a Space storage descriptor does not move existing files.
