# Data Model Overview

This document describes the high-level data model of IEapp, including its storage principles and directory structure.

## Terminology Distinction

To ensure clarity, IEapp distinguishes between the **System Data Model** and user-defined **Classes**:

- **System Data Model**: The underlying architecture of how data is handled, stored, and retrieved (e.g., "Filesystem = Database", directory structure, row-level integrity).
- **Note Classes**: User-defined table schemas stored in Iceberg; templates are fixed globally. Formerly known as "Schemas".

## Principles

IEapp's data model is built on these principles:

| Principle | Description |
|-----------|-------------|
| **Filesystem = Database** | Workspaces are directory trees; Iceberg tables are file-backed |
| **Class-on-Read** | Notes are reconstructed from Class-defined fields in Iceberg |
| **Append-Only Integrity** | Revisions are appended in Iceberg; history is immutable |
| **Table-Backed Storage** | Notes live in Apache Iceberg tables via OpenDAL |

## Directory Structure

See [directory-structure.md](directory-structure.md) for the full workspace layout.

```
global.json                    # Workspace registry
workspaces/
  {workspace_id}/
    meta.json                  # Workspace metadata
    settings.json              # Workspace settings
    classes/                   # Iceberg-managed Class tables (layout not specified)
    attachments/               # Binary files
    sql_sessions/              # SQL query sessions (results + progress)
```

## Key Concepts

### Classes

Classes define note types with:
- **Template**: Fixed global template `# {class_name} + H2 columns`
- **Fields**: Content columns derived from the Iceberg table schema
- **Types**: Iceberg column types mapped to note fields
- **Extra Attributes Policy**: `allow_extra_attributes` controls non-registered H2 sections

### Metadata vs Content Columns

IEapp separates columns into two ownership categories:

- **Metadata columns (system-owned)**: Reserved fields created and managed by IEapp.
  Users **cannot** define Class fields with these names.
- **Content columns (user-owned)**: Class-defined fields stored in the Iceberg `fields` struct.

Reserved metadata column names include (case-insensitive):

`id`, `note_id`, `title`, `class`, `tags`, `links`, `attachments`,
`created_at`, `updated_at`, `revision_id`, `parent_revision_id`,
`deleted`, `deleted_at`, `author`, `canvas_position`, `integrity`,
`workspace_id`, `word_count`.

The metadata column list is treated as an internal system contract and may expand
over time; Class creation MUST reject any field name that conflicts with a
reserved metadata column name.

### Metadata Classes

IEapp also reserves **metadata Class names** for system-owned tables. Users cannot
create or update Classes with these names. The reserved metadata Class list is
case-insensitive and may expand over time.

Reserved metadata Class names include:

`SQL`

### Properties Extraction

The write pipeline extracts properties from Markdown:

1. **Frontmatter**: YAML block at top of Markdown
2. **H2 Sections**: `## Field Name` headers (must be Class-defined)
3. **Auto Properties**: Computed values (word_count, etc.)

Precedence: Section > Frontmatter > Auto default

Extra H2 sections are handled by the Class policy:
- `deny`: reject notes with unknown H2 sections
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
- **binary** → parsed from `base64:` or `hex:` strings and stored as canonical `base64:`
- **list** → parsed from Markdown bullet lists (e.g. `- item`)
- **object_list** → parsed from a JSON array of objects (each object must include
  `type`, `name`, and `description` as strings)

If a list is provided as plain lines, each non-empty line becomes an item.
Type casting errors are reported during validation.

### Link URIs

Notes can contain IEapp-internal links using the `ieapp://` scheme. The URI
kind determines the link target and is designed to be extensible:

- `ieapp://note/{note_id}`
- `ieapp://attachment/{attachment_id}`

IEapp normalizes equivalent forms (e.g. `ieapp://notes/{id}`,
`ieapp://attachments/{id}`, `ieapp://note?id=...`) to canonical URIs on write.
This keeps Markdown stable while allowing new link kinds in future milestones.

### Versioning

Every save creates a new revision row in the Iceberg `revisions` table:

1. Client sends update with `parent_revision_id`
2. Server validates parent matches current head
3. New revision row is appended via Iceberg
4. `notes` table updated to new head

Conflicts return HTTP 409 with current revision.

## Indices

Materialized indexes (search, embeddings, stats) are derived from Iceberg tables
and can be regenerated. The Iceberg-managed layout is the only source of truth.

## Integrity

All data is signed with HMAC:
- Key stored in `global.json`
- Signature stored alongside note and revision rows
- Checksum (SHA-256) for tamper detection

## Extra Attributes Storage

When allowed, unknown H2 sections are persisted in the `extra_attributes` column
as a deterministic JSON object. On read, `fields` and `extra_attributes` are
merged to reconstruct Markdown and properties.
