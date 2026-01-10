# Data Model Overview

## Principles

IEapp's data model is built on these principles:

| Principle | Description |
|-----------|-------------|
| **Filesystem = Database** | Every workspace is a directory tree; no hidden RDB |
| **Class-on-Read** | Markdown stays flexible; indexer projects it into typed objects |
| **Append-Only Integrity** | Writes never mutate history; revisions are appended |
| **Human-Readable** | JSON + Markdown format; easy to inspect and backup |

## Directory Structure

See [directory-structure.md](directory-structure.md) for the full workspace layout.

```
global.json                    # Workspace registry
workspaces/
  {workspace_id}/
    meta.json                  # Workspace metadata
    settings.json              # Workspace settings
    classes/                   # Class definitions (formerly "schemas")
      {class_name}.json
    index/                     # Materialized views
      index.json
      inverted_index.json
      stats.json
    attachments/               # Binary files
    notes/
      {note_id}/
        meta.json              # Note metadata
        content.json           # Note content + frontmatter
        history/
          index.json           # Revision list
          {revision_id}.json   # Each revision
```

## Key Concepts

### Classes (formerly "Schema")

Classes define note types with:
- **Template**: Default Markdown content for new notes
- **Fields**: Required and optional H2 headers
- **Types**: string, number, date, list, markdown

See [file-schemas.yaml](file-schemas.yaml) for the Class JSON schema.

### Properties Extraction

The indexer extracts properties from notes:

1. **Frontmatter**: YAML block at top of Markdown
2. **H2 Sections**: `## Field Name` headers
3. **Auto Properties**: Computed values (word_count, etc.)

Precedence: Section > Frontmatter > Auto default

### Versioning

Every save creates a new revision:

1. Client sends update with `parent_revision_id`
2. Server validates parent matches current head
3. New revision stored in `history/{revision_id}.json`
4. `content.json` updated to new head

Conflicts return HTTP 409 with current revision.

## Indices

| Index | Purpose | Used By |
|-------|---------|---------|
| `index.json` | Structured note records | List, query, MCP |
| `inverted_index.json` | Keyword posting lists | Search |
| `stats.json` | Aggregates (counts, etc.) | UI badges |

## Integrity

All data is signed with HMAC:
- Key stored in `global.json`
- Signature in each file's `integrity.signature`
- Checksum (SHA-256) for tamper detection
