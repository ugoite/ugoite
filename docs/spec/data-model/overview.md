---
title: 'Data model overview'
---

Ugoite treats operator-controlled files as the persistence boundary. A **Space** is a portable ownership boundary below the configured storage root and an Iceberg namespace. Apache Iceberg owns one append-only table per stable Form ID.

## Authority layers

- **Authoring:** people and agents edit Markdown.
- **Domain contract:** a Form defines the typed H2 fields accepted for an Entry.
- **Persistence:** Catalog Head, its reachable immutable publication records,
  Iceberg metadata, and Iceberg revision tables are authoritative through the
  configured OpenDAL Space boundary.

The browser is currently server-backed. It does not own an independent local Space database; the Rust server and core write to the configured OpenDAL operator.

## Current storage roots

```text
spaces/{space_id}/
  meta.json
  settings.json
  _ugoite/catalog/        # Head plus immutable publication records
  forms/                  # Iceberg-owned table locations
  assets/
  sql_sessions/

users/{sha256(user_id)}/
  preferences.json
```

See [directory-structure.md](directory-structure.md) and [directory-layout.yaml](https://github.com/ugoite/ugoite/blob/main/docs/spec/data-model/directory-layout.yaml) for the exact repository-owned paths. Iceberg-internal filenames are deliberately not specified.

## Spaces

`meta.json` stores the Space identity, storage descriptor, and integrity key material. `settings.json` is created with `default_form: Entry`; membership and invitation objects are added lazily by the collaboration service. Public Space patching cannot modify membership-managed keys.

Creating a Space also creates an `Entry` Form with a Markdown `Body` field. On local Unix filesystems, the Space directories are set to owner-only mode and metadata files to owner read/write mode.

## Forms and Entries

A Form currently persists only:

- `name`;
- `version`;
- `fields`;
- `allow_extra_attributes` (`deny`, `allow_json`, or `allow_columns`).

Form-level ACL fields are not persisted or enforced in this release. Field names cannot collide with reserved metadata columns. A `row_reference` field must name an existing, non-reserved target Form.

Each Form has an immutable UUID and one physical `form_<uuid>` Iceberg table.
The display name is mutable metadata. Field IDs are stable Iceberg field IDs;
rename keeps the ID, optional additions do not rewrite data, and changing the
type of an existing field is rejected. Before v1, create a new field when a
different type is needed; no migration compatibility path is exposed.

The table is an append-only revision log. Common columns include `entry_id`,
`revision_id`, `parent_revision_id`, `entry_version`, `operation`,
`committed_at`, `author_id`, `form_version`, `source_kind`, and `source_id`,
followed by Form fields. Delete is a tombstone and restore is another revision.
Current state is derived from the unique greatest version and is never a second
source-of-truth table. Equal greatest versions are a visible corruption/conflict
and never resolved by iteration order.

Date, time, timestamp, UUID, and binary Form fields use their corresponding
Iceberg primitive types. Markdown, SQL, row references, and ordinary strings
remain Iceberg strings; binary entry values are base64 text at the domain
boundary and binary data in the table.

## Markdown mapping

The global template is:

```markdown
# {title}

## {field_name}
{value}
```

H2 sections are parsed according to the Form field type. Supported types are exposed by `GET /spaces/{space_id}/forms/types`; the Rust Form implementation is the source of truth. Unknown sections are rejected or retained according to `allow_extra_attributes`.

## Search, query, and derived data

The current keyword search scans non-deleted Entry rows for a case-insensitive
substring. It does **not** use a persistent inverted index or relevance ranking.
The target read path is an authorized, snapshot-pinned DataFusion plan over
Iceberg data; that transition is tracked separately and must not add a JSON
materialization table or a second history store.

`ugoite index stats` is available in core/local mode. Reindex and per-entry index update return an explicit “not implemented in this release” error, so persistent live-index/watch-loop behavior remains planned.

## Saved SQL and SQL sessions

Saved SQL is represented through the reserved SQL metadata Form. A query session
writes `sql_sessions/{session_id}/meta.json` with one reproducible
`SpaceCheckpoint`; row and count requests use that coordinate and bounded
deterministic pagination. Session metadata remains derived state, not an
alternate Catalog or result store. See
[sql-sessions.md](sql-sessions.md).

## Assets, links, and integrity

Assets are stored as runtime-generated `{asset_id}_{safe_original_name}` files under the Space. Deletion is blocked while an Entry still references the Asset.

Internal links use canonical `ugoite://entry/{entry_id}` and `ugoite://asset/{asset_id}` URIs. Entry content and revisions carry checksums and HMAC signatures generated from Space-local integrity material. Response-signing material may also be written lazily to `hmac.json`.
