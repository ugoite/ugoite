# Data model overview

Ugoite treats operator-controlled files as the persistence boundary. A **Space** is a self-contained directory below the configured storage root, while Apache Iceberg owns the physical table layout used for Forms, Entries, and revisions.

## Authority layers

- **Authoring:** people and agents edit Markdown.
- **Domain contract:** a Form defines the typed H2 fields accepted for an Entry.
- **Persistence:** Iceberg tables and the JSON metadata files listed below are authoritative on disk.

The browser is currently server-backed. It does not own an independent local Space database; the Rust server and core write to the configured OpenDAL operator.

## Current storage roots

```text
spaces/{space_id}/
  meta.json
  settings.json
  forms/                  # Iceberg-owned subtree
  assets/
  materialized_views/
  sql_sessions/

users/{sha256(user_id)}/
  preferences.json
```

See [directory-structure.md](directory-structure.md) and [directory-layout.yaml](directory-layout.yaml) for the exact repository-owned paths. Iceberg-internal filenames are deliberately not specified.

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

Each Form has logical `entries` and `revisions` Iceberg tables. Current Entry rows include identifiers, title, Form, tags, links, timestamps, typed fields, extra attributes, revision lineage, assets, integrity data, deletion state, and author. Each save appends a revision row; restore creates another revision rather than rewriting history.

## Markdown mapping

The global template is:

```markdown
# {title}

## {field_name}
{value}
```

H2 sections are parsed according to the Form field type. Supported types are exposed by `GET /spaces/{space_id}/forms/types`; the Rust Form implementation is the source of truth. Unknown sections are rejected or retained according to `allow_extra_attributes`.

## Search, query, and derived data

The current keyword search scans non-deleted Entry rows for a case-insensitive substring. It does **not** use a persistent inverted index or relevance ranking. Structured query and Ugoite SQL execute against current Entry data.

`ugoite index stats` is available in core/local mode. Reindex and per-entry index update return an explicit “not implemented in this release” error, so persistent live-index/watch-loop behavior remains planned.

## Saved SQL and SQL sessions

Saved SQL is represented through the reserved SQL metadata Form. A query session writes only `sql_sessions/{session_id}/meta.json`; row and count requests re-run the SQL against current readable data. Materialized-view records are metadata placeholders, not persisted result tables. See [sql-sessions.md](sql-sessions.md).

## Assets, links, and integrity

Assets are stored as runtime-generated `{asset_id}_{safe_original_name}` files under the Space. Deletion is blocked while an Entry still references the Asset.

Internal links use canonical `ugoite://entry/{entry_id}` and `ugoite://asset/{asset_id}` URIs. Entry content and revisions carry checksums and HMAC signatures generated from Space-local integrity material. Response-signing material may also be written lazily to `hmac.json`.
