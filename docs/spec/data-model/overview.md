# Data Model Overview

This document describes the high-level data model of Ugoite, including its storage principles and directory structure.

## Terminology Distinction

To ensure clarity, Ugoite distinguishes between the **System Data Model** and user-defined **Forms**:

- **System Data Model**: The underlying architecture of how data is handled, stored, and retrieved (e.g., "Filesystem = Database", directory structure, row-level integrity).
- **Entry Forms**: User-defined table schemas stored in Iceberg; templates are fixed globally. Formerly known as "Schemas".

## Principles

Ugoite's data model is built on these principles:

| Principle | Description |
|-----------|-------------|
| **Filesystem = Database** | Spaces are directory trees; Iceberg tables are file-backed |
| **Form-on-Read** | Entries are reconstructed from Form-defined fields in Iceberg |
| **Append-Only Integrity** | Revisions are appended in Iceberg; history is immutable |
| **Table-Backed Storage** | Entries live in Apache Iceberg tables via OpenDAL |

## Directory Structure

See [directory-structure.md](directory-structure.md) for the full space layout.

```
global.json                    # Space registry
spaces/
  {space_id}/
    meta.json                  # Space metadata
    settings.json              # Space settings
    forms/                     # Iceberg-managed Form tables (layout not specified)
    assets/                    # Binary files
    materialized_views/        # SQL materialized view metadata (no rows)
    sql_sessions/              # SQL query sessions (metadata only)
```

## Key Concepts

### Forms

Forms define entry types with:
- **Template**: Fixed global template `# {form_name} + H2 columns`
- **Fields**: Content columns derived from the Iceberg table schema
- **Types**: Iceberg column types mapped to entry fields
- **Extra Attributes Policy**: `allow_extra_attributes` controls non-registered H2 sections

### Metadata vs Content Columns

Ugoite separates columns into two ownership categories:

- **Metadata columns (system-owned)**: Reserved fields created and managed by Ugoite.
  Users **cannot** define Form fields with these names.
- **Content columns (user-owned)**: Form-defined fields stored in the Iceberg `fields` struct.

Reserved metadata column names include (case-insensitive):

`id`, `entry_id`, `title`, `form`, `tags`, `links`, `assets`,
`created_at`, `updated_at`, `revision_id`, `parent_revision_id`,
`deleted`, `deleted_at`, `author`, `canvas_position`, `integrity`,
`space_id`, `word_count`.

The metadata column list is treated as an internal system contract and may expand
over time; Form creation MUST reject any field name that conflicts with a
reserved metadata column name.

### Metadata Forms

Ugoite also reserves **metadata Form names** for system-owned tables. Users cannot
create or update Forms with these names. The reserved metadata Form list is
case-insensitive and may expand over time.

Reserved metadata Form names include:

`SQL`

### SQL Materialized Views

Saved SQL (created via `create_sql`) has a corresponding **materialized view
metadata** record under `spaces/{space_id}/materialized_views/`. The metadata is
created, updated, and deleted **in lockstep** with the SQL record. The metadata
record is the only persisted query artifact; SQL session results are not stored.

Materialized views are currently metadata-only placeholders. The on-disk layout
for future Iceberg-managed views beneath `materialized_views/` is intentionally
unspecified.

### SQL Sessions (Metadata Only)

SQL sessions are short-lived (target: ~10 minutes) and store **metadata only**
under `spaces/{space_id}/sql_sessions/{session_id}/meta.json`. They do **not**
persist result rows. Session metadata includes a logical view snapshot ID and
paging strategy so that each request can re-run the SQL query quickly against
the current entries tables.

This keeps the system stateless beyond OpenDAL storage (no RDB, no external job
queue, no NFS), while still allowing multiple API servers to serve the same
session.

### Properties Extraction

The write pipeline extracts properties from Markdown:

1. **Frontmatter**: YAML block at top of Markdown
2. **H2 Sections**: `## Field Name` headers (must be Form-defined)
3. **Auto Properties**: Computed values (word_count, etc.)

Precedence: Section > Frontmatter > Auto default

Extra H2 sections are handled by the Form policy:
- `deny`: reject entries with unknown H2 sections
- `allow_json`: store unknown sections in `extra_attributes`
- `allow_columns`: accept unknown sections and store in `extra_attributes`

### Content Column Types & Markdown Parsing

Content column types map to Iceberg primitives and are parsed from Markdown
using Markdown-friendly rules:

- **string**, **markdown** → stored as strings
- **number**, **double** → parsed as $f64$
- **float** → parsed as $f32$
- **integer** → parsed as $i32$
- **long** → parsed as $i64$
- **boolean** → parsed from `true/false`, `yes/no`, `on/off`, `1/0`
- **date** → parsed as `YYYY-MM-DD`
- **time** → parsed as `HH:MM:SS` or `HH:MM:SS.ssssss`
- **timestamp** → parsed as RFC3339 (`2025-01-01T12:34:56Z`)
- **timestamp_tz** → parsed as RFC3339 and normalized to UTC
- **timestamp_ns** → parsed as RFC3339 with nanosecond precision
- **timestamp_tz_ns** → parsed as RFC3339 with nanosecond precision and normalized to UTC
- **uuid** → parsed as a canonical UUID string
- **row_reference** → stored as a string reference (e.g. entry ID or `ugoite://entry/{entry_id}`)
  and MUST declare a `target_form` in the Form field definition. References resolve against
  the target Form's `entry_id` metadata column.
- **binary** → parsed from `base64:` or `hex:` strings and stored as canonical `base64:`
- **list** → parsed from Markdown bullet lists (e.g. `- item`)
- **object_list** → parsed from a JSON array of objects (each object must include
  `type`, `name`, and `description` as strings)

If a list is provided as plain lines, each non-empty line becomes an item.
Type casting errors are reported during validation.

### Link URIs

Entries can contain Ugoite-internal links using the `ugoite://` scheme. The URI
kind determines the link target and is designed to be extensible:

- `ugoite://entry/{entry_id}`
- `ugoite://asset/{asset_id}`

Ugoite normalizes equivalent forms (e.g. `ugoite://entries/{id}`,
`ugoite://assets/{id}`, `ugoite://entry?id=...`) to canonical URIs on write.
This keeps Markdown stable while allowing new link kinds in future milestones.

### Versioning

Every save creates a new revision row in the Iceberg `revisions` table:

1. Client sends update with `parent_revision_id`
2. Server validates parent matches current head
3. New revision row is appended via Iceberg
4. `entries` table updated to new head

Conflicts return HTTP 409 with current revision.

## Indices

Materialized indexes (search, embeddings, stats) are derived from Iceberg tables
and can be regenerated. The Iceberg-managed layout is the only source of truth.

## Integrity

All data is signed with HMAC:
- Key stored in `global.json`
- Signature stored alongside entry and revision rows
- Checksum (SHA-256) for tamper detection

## Extra Attributes Storage

When allowed, unknown H2 sections are persisted in the `extra_attributes` column
as a deterministic JSON object. On read, `fields` and `extra_attributes` are
merged to reconstruct Markdown and properties.
