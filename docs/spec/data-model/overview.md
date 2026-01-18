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
```

## Key Concepts

### Classes

Classes define note types with:
- **Template**: Fixed global template `# {class_name} + H2 columns`
- **Fields**: Derived from the Iceberg table schema
- **Types**: Iceberg column types mapped to note fields

### Properties Extraction

The write pipeline extracts properties from Markdown:

1. **Frontmatter**: YAML block at top of Markdown
2. **H2 Sections**: `## Field Name` headers (must be Class-defined)
3. **Auto Properties**: Computed values (word_count, etc.)

Precedence: Section > Frontmatter > Auto default

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
