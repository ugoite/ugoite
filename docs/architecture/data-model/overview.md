---
title: "Data model overview"
---

Ugoite treats user-owned Space files as the persistence boundary. A **Space** is
a portable ownership boundary below the configured storage root and an Iceberg
namespace. Deployment and storage operators may control that root without
becoming the Knowledge authority. Apache Iceberg owns one append-only table per
stable Form ID.

Node-local Space locators are kept separately at
`_ugoite/space-bindings/{space_id}.json`; they are not part of a portable Space
export.

## Authority layers

- **Authoring:** people and agents edit Markdown.
- **Domain contract:** a Form defines the typed H2 fields accepted for an Entry.
- **Persistence:** Catalog Head, its reachable immutable publication records,
  Iceberg metadata, and Iceberg revision tables are authoritative through the
  configured OpenDAL Space boundary.

The browser is currently server-backed. It does not own an independent local
Space database; the Rust server and core write to the configured OpenDAL
operator.

## Current storage roots

```text
spaces/{space_id}/
  meta.json
  settings.json
  _ugoite/catalog/        # Head plus immutable publication records
  _ugoite/derived/        # lazy, replaceable relation Heads and builds
  forms/                  # Iceberg-owned table locations
  assets/

users/{sha256(user_id)}/
  preferences.json
```

See [directory-structure.md](directory-structure.md) and
[directory-layout.yaml](https://github.com/ugoite/ugoite/blob/main/docs/architecture/data-model/directory-layout.yaml)
for the current repository-owned paths. These paths describe the current
encoding, not the v0.1 semantic compatibility floor; Iceberg-internal filenames
are deliberately not specified.

## Spaces

`meta.json` stores the Space identity, the durable `space_version: "0.1"`
compatibility identity, and integrity key material; it does not store a
physical storage descriptor. Product version, Space compatibility version, and
internal physical representation are independent. A Node-local binding, when
configured, is kept outside the `spaces/{space_id}` prefix and is merged only
into the runtime Space view. `settings.json` is created with `default_form: Entry`.
`default_form` is a required Space 0.1 storage/bootstrap field retained for
portable settings compatibility; it is not the browser's current Form-selection
authority. The browser-backed entry flow uses its explicit Form route instead.
Portable
membership, principal, policy, and authorization-audit state is stored in
`security/principals.json`. Legacy membership-shaped settings are unsupported,
and public Space patching cannot modify membership-managed keys.

The Space UID is the immutable remote identity. The slug is human-readable,
mutable metadata, and a local Space path is the Core-mode filesystem or
object-store locator; neither is an alternate remote identity.

Creating a Space also creates an `Entry` Form with a Markdown `Body` field. On
local Unix filesystems, the Space directories are set to owner-only mode and
metadata files to owner read/write mode.

## Forms and Entries

A Form currently persists only:

- `name`;
- `version`;
- `fields`;
- `allow_extra_attributes` (`deny`, `allow_json`, or `allow_columns`).

Form-level ACL fields are not persisted or enforced in this release. Field names
cannot collide with reserved metadata columns. A `row_reference` field must name
an existing, non-reserved target Form.

Each Form has an immutable UUID and one physical `form_<uuid>` Iceberg table.
The display name is mutable metadata. Field IDs are stable Iceberg field IDs;
the pre-v1 authoring/API contract keeps existing field names and IDs stable:
renaming or removing an existing field, or changing its type, is rejected.
Optional additions do not rewrite data. Before v1, add a new field when a
different name or type is needed; no migration compatibility path is exposed.

The table is an append-only revision log. Common columns include `entry_id`,
`revision_id`, `parent_revision_id`, `entry_version`, `operation`,
`committed_at`, `author_id`, `form_version`, `source_kind`, and `source_id`,
followed by Form fields. Entry attribution is system-managed: `author` is the
original creator, `updated_by` is the actor for the latest revision, and
`deleted_by` is the actor for the current tombstone only. Delete is a tombstone
and restore is another revision. This is a breaking pre-v1 physical-schema
change; a Space with the former attribution-free table schema must be recreated
rather than silently interpreted with incomplete attribution. Current state is
derived from the unique greatest version and is never a second source-of-truth
table. Equal greatest versions are a visible corruption/conflict and never
resolved by iteration order.

An Entry has no intrinsic title. A human-readable name exists only when the
Form defines it as a normal field (for example a `name` string field). Tables
created before this boundary may contain unclaimed physical columns; the
generic schema reader exposes those columns under their physical names without
migration or semantic interpretation. The stable `entry_id` is the canonical
identity and is never synthesized into a stored field.

Date, time, timestamp, UUID, and binary Form fields use their corresponding
Iceberg primitive types. Markdown, SQL, row references, and ordinary strings
remain Iceberg strings; binary entry values are base64 text at the domain
boundary and binary data in the table.

The timestamp types retain their distinct logical meanings. `timestamp` and
`timestamp_ns` are timezone-less wall-clock values and preserve the entered
local date-time. `timestamp_tz` and `timestamp_tz_ns` represent an instant,
require an offset-bearing RFC3339 value at the domain boundary, and are stored
normalized to UTC. The server never infers a timezone for a timezone-less value.

## Structured presentation mapping

When a structured Entry is rendered for a read-only content response, the
Form-defined fields are represented as:

```markdown
## {field_name}

{value}
```

There is no Entry-level heading or title. Form-defined fields remain the
authority, and a Markdown field's body is never reparsed as Entry metadata.
H2 sections are rendered according to the Form field type. Supported types are
exposed by `GET /spaces/{space_id}/forms/types`; the Rust Form implementation
is the source of truth. Unknown sections are rejected or retained according
to `allow_extra_attributes`.

## Search, query, and derived data

Keyword search scans current Entry data and the authorized internal AssetText
projection for Unicode-normalized, case-insensitive substring matches. Search
normalizes both the stored value and query with Unicode NFKC followed by Unicode
lowercase; it does not rewrite stored content. It returns Entries, applies
authorization before AssetText joins, and degrades to native Entry search when
the derived relation is unavailable. There is still no persistent inverted index
or relevance ranking. AssetText is an Iceberg-backed DerivedRelation, not a
Form, publication coordinate, ACL authority, or second history store.

`ugoite index stats` reports AssetText derived health. `ugoite index run` and
`ugoite index run --component asset-text` rebuild it by scanning current
authoritative Entry references; object listing is not a source set.

## Saved SQL and stateless SQL Query

Saved SQL is represented through the reserved SQL metadata Form and is durable
Knowledge. Executing `sql.query` or `sql.query.count` is disposable work: it
reads a fixed `PublicationRef`, returns bounded rows or a count, and keeps any
continuation only with the client. Query execution does not create a session,
result relation, or metadata file in the Space.

## Assets and integrity

Asset bytes have a low-level lifecycle independent of Form definitions. Upload
allocates a stable Asset ID and writes `assets/{asset_id}`; the response is an
`AssetReference` value containing only `asset_id`, `name`, `media_type`,
`size_bytes`, and `sha256`. A Form owns any reference through an
`asset_reference` field or a typed list of those values. Byte reads require an
explicit containing Form/Entry context; the exact-ID operation cannot
reconstruct logical name or media type. Deletion validates current references
against the exact Catalog Head and publishes an immutable `asset.delete`
publication. Asset reads treat that publication as unavailable only while it is
reachable from the authoritative Head; physical bytes are retained and no
automatic purge is part of v1.

Entry content and revisions carry checksums and HMAC signatures generated from
Space-local integrity material. Response-signing material may also be written
lazily to `spaces/{space_id}/hmac.json`; default Node/API responses use the
separate `response_hmac/default.json` at the configured operator root. The
Node-default material is not part of a Space export.

### Form-owned attachment editing

The browser exposes `asset_reference` and `list<asset_reference>` as ordinary
Form field controls. The scalar control accepts one uploaded reference; the
typed list preserves the displayed order and accepts zero or more references.
Neither control creates an Assets Form, a universal Entry attachment property,
or Asset metadata Entries.

Markdown-oriented Entry input represents these values as JSON in the field
section, preserving the complete `AssetReference` object. For example:

```json
{
  "asset_id": "019...",
  "name": "report.pdf",
  "media_type": "application/pdf",
  "size_bytes": 123456,
  "sha256": "..."
}
```

The editor treats byte upload and Entry revision commit as separate states: an
uploaded reference remains provisional until the normal Entry create/update
operation succeeds. Retrying a failed Entry save reuses that reference; closing
the editor does not delete bytes automatically. Removing a reference only
changes the Form-owned Entry value. Byte reads always use the containing
Form/Entry authorization context, and an unavailable byte is rendered as a
field-level state while the logical reference metadata remains visible.
