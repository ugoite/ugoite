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
the pre-v1 authoring/API contract keeps existing field names and IDs stable:
renaming or removing an existing field, or changing its type, is rejected.
Optional additions do not rewrite data. Before v1, add a new field when a
different name or type is needed; no migration compatibility path is exposed.

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

The timestamp types retain their distinct logical meanings. `timestamp` and
`timestamp_ns` are timezone-less wall-clock values and preserve the entered
local date-time. `timestamp_tz` and `timestamp_tz_ns` represent an instant,
require an offset-bearing RFC3339 value at the domain boundary, and are stored
normalized to UTC. The server never infers a timezone for a timezone-less
value.

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

## Assets and integrity

Asset bytes have a low-level lifecycle independent of Form definitions. Upload
allocates a stable Asset ID and writes `assets/{asset_id}`; the response is an
`AssetReference` value containing only `asset_id`, `name`, `media_type`,
`size_bytes`, and `sha256`. A Form owns any reference through an
`asset_reference` field or a typed list of those values. Byte reads require an
explicit containing Form/Entry context; the exact-ID operation cannot
reconstruct logical name or media type. Deletion publishes an Asset lifecycle
marker through the Catalog Head CAS before removing the byte, so a concurrent
reference commit or deletion conflicts rather than leaving a current reference
to missing bytes.

Entry content and revisions carry checksums and HMAC signatures generated from
Space-local integrity material. Response-signing material may also be written
lazily to `hmac.json`.

### Form-owned attachment editing

The browser exposes `asset_reference` and `list<asset_reference>` as ordinary
Form field controls. The scalar control accepts one uploaded reference; the
typed list preserves the displayed order and accepts zero or more references.
Neither control creates an Assets Form, a universal Entry attachment property,
or Asset metadata Entries.

Markdown-oriented Entry input represents these values as JSON in the field
section, preserving the complete `AssetReference` object. For example:

```json
{"asset_id":"019...","name":"report.pdf","media_type":"application/pdf","size_bytes":123456,"sha256":"..."}
```

The editor treats byte upload and Entry revision commit as separate states:
an uploaded reference remains provisional until the normal Entry create/update
operation succeeds. Retrying a failed Entry save reuses that reference; closing
the editor does not delete bytes automatically. Removing a reference only
changes the Form-owned Entry value. Byte reads always use the containing
Form/Entry authorization context, and an unavailable byte is rendered as a
field-level state while the logical reference metadata remains visible.
