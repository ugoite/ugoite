# IEapp Terminology Guide

**Version**: 1.0  
**Last Updated**: February 2, 2026  
**Audience**: Contributors, users, and AI agents integrating with IEapp

---

## Purpose

This guide defines the core terminology used throughout IEapp to ensure consistent understanding across:
- Code (frontend, backend, CLI, core)
- Documentation (specifications, API docs, README)
- User interfaces
- External integrations (MCP protocol)

---

## Core Concepts

### Workspace

**Definition**: A self-contained, isolated data directory that serves as the top-level container for all knowledge management content.

**Structure**:
```
workspaces/
  {workspace_id}/
    meta.json          # Workspace metadata
    settings.json      # User preferences
    classes/           # Iceberg tables for note storage
    attachments/       # Binary file storage
```

**Key Properties**:
- **Isolation**: Each workspace is independent; data doesn't leak between workspaces
- **Portability**: Can be stored on local filesystem, S3, or other fsspec-compatible backends
- **Registry**: All workspaces are listed in `global.json`

**Related Terms**: Contains Notes (via Classes), Attachments, Links, Settings

**Examples**:
- Personal knowledge base: `workspace-personal`
- Project workspace: `workspace-project-alpha`
- Team shared workspace: `workspace-team-design`

---

### Class

**Definition**: A user-defined schema that specifies the structure and types of fields for a category of Notes.

**Purpose**: Acts as a template and validation system for Notes

**Components**:
- **Name**: Unique identifier (e.g., "Meeting", "Task", "Person")
- **Fields**: Named properties with types (date, string, list, etc.)
- **Template**: Fixed global template pattern: `# {class_name}` + H2 headers for fields
- **Version**: Schema version for migration support
- **Extra Attributes Policy**: Controls handling of unknown H2 sections

**Relationship with Notes**:
```
Class = Template/Schema
Note = Instance/Record

Class "Meeting"              Note (instance)
├─ Template: "# Meeting"    ├─ title: "Weekly Sync"
├─ Fields:                   ├─ class: "Meeting"
│   ├─ Date (date)          ├─ Date: "2025-11-29"
│   ├─ Attendees (list)     ├─ Attendees: ["Alice", "Bob"]
│   └─ Notes (markdown)     └─ Notes: "Discussed..."
```

**Reserved Class Names**: `SQL` (used for saved queries)

**Storage**: Stored as Iceberg table metadata; each Class has `notes` and `revisions` tables

**Related Terms**: Note (instance of), Field (defines), Template (provides)

---

### Note

**Definition**: The fundamental knowledge unit in IEapp; a Markdown document with structured properties defined by a Class.

**Characteristics**:
- **Format**: Markdown with YAML frontmatter
- **Structure**: H2 sections map to Class-defined fields
- **Typing**: Belongs to exactly one Class (via `class` field)
- **Versioning**: Each update creates a new Revision
- **Identity**: Unique `note_id` (UUID)
- **Referencing**: Can link to other Notes and reference Attachments

**Markdown Structure**:
```markdown
---
class: Meeting
tags: [team, weekly]
---

# Weekly Team Sync

## Date
2025-11-29

## Attendees
- Alice
- Bob

## Notes
We discussed the Q4 roadmap and decided to prioritize feature X.

Link to previous meeting: [Last week](ieapp://note/uuid-123)
Presentation slides: [deck.pdf](ieapp://attachment/uuid-456)
```

**Data Model**:
```typescript
interface Note {
  id: string;              // Unique identifier
  title?: string;          // Extracted from H1
  content: string;         // Full Markdown
  class?: string;          // Associated Class name
  tags?: string[];         // User-defined tags
  links?: NoteLink[];      // References to other Notes
  attachments?: Attachment[]; // Referenced binary files
  revision_id: string;     // Current version
  created_at: string;      // Creation timestamp
  updated_at: string;      // Last modification timestamp
  canvas_position?: {      // Optional spatial position
    x: number;
    y: number;
  };
}
```

**Storage**: Stored in Iceberg tables (one table per Class)

**API Endpoints**:
```
POST   /workspaces/{ws_id}/notes           # Create
GET    /workspaces/{ws_id}/notes           # List
GET    /workspaces/{ws_id}/notes/{id}      # Get
PUT    /workspaces/{ws_id}/notes/{id}      # Update
DELETE /workspaces/{ws_id}/notes/{id}      # Delete
GET    /workspaces/{ws_id}/notes/{id}/history  # Get versions
```

**MCP URI**: `ieapp://note/{note_id}` or `ieapp://{workspace_id}/notes/{note_id}`

**Related Terms**: Class (defines structure), Revision (version history), Link (connections), Attachment (references), Field (properties)

---

### Attachment

**Definition**: A binary file (image, audio, PDF, etc.) stored in a workspace that can be referenced by Notes.

**Characteristics**:
- **Format**: Any binary file type
- **Storage**: Independent directory (`attachments/` in workspace)
- **Naming**: UUID-based with original extension preserved
- **Referencing**: Notes link via `ieapp://attachment/{id}` URIs
- **Reference Counting**: Cannot be deleted if still referenced by any Note
- **No Versioning**: Unlike Notes, Attachments don't maintain revision history

**Data Model**:
```typescript
interface Attachment {
  id: string;        // Unique identifier (UUID)
  name: string;      // Original filename
  path: string;      // Storage path (relative to workspace)
}
```

**Storage Path Pattern**:
```
workspaces/
  {workspace_id}/
    attachments/
      {uuid}.{extension}
```

**API Endpoints**:
```
POST   /workspaces/{ws_id}/attachments     # Upload
GET    /workspaces/{ws_id}/attachments     # List
GET    /workspaces/{ws_id}/attachments/{id} # Download
DELETE /workspaces/{ws_id}/attachments/{id} # Delete
```

**MCP URI**: `ieapp://attachment/{attachment_id}`

**Related Terms**: Note (references from), Workspace (contains)

---

### Revision

**Definition**: An immutable historical snapshot of a Note at a specific point in time.

**Purpose**: Version control for Notes; enables time-travel and conflict resolution

**Characteristics**:
- **Immutability**: Once created, never modified
- **Append-Only**: New revisions are appended to Iceberg `revisions` table
- **Parent Chain**: Each revision links to its parent (forming a DAG)
- **Checksums**: SHA-256 hash for integrity verification
- **Signatures**: HMAC for tamper detection

**Update Flow**:
```
1. Client reads Note, gets current revision_id: "rev-001"
2. User edits Note
3. Client sends update with parent_revision_id: "rev-001"
4. Server validates parent matches current head
5. If match: creates new revision "rev-002" and updates Note
6. If mismatch: returns HTTP 409 Conflict with current revision
```

**API Endpoints**:
```
GET  /workspaces/{ws_id}/notes/{id}/history              # List revisions
GET  /workspaces/{ws_id}/notes/{id}/history/{rev_id}    # Get specific revision
POST /workspaces/{ws_id}/notes/{id}/restore              # Restore old revision
```

**Storage**: Stored in Iceberg `{class}/revisions` table

**Related Terms**: Note (versioned entity), Integrity (verification)

---

### Link

**Definition**: A typed connection between Notes, or from Notes to Attachments, represented using internal URIs.

**Types**:
1. **Note Links**: Connections between Notes
2. **Attachment References**: From Notes to Attachments

**URI Schemes**:
```
ieapp://note/{note_id}           # Canonical form
ieapp://attachment/{id}          # Canonical form
```

**Link Kinds** (for Note-to-Note):
- `related`: General association
- `parent`: Hierarchical parent
- `child`: Hierarchical child
- `reference`: Citation or external reference

**Usage in Notes**:
```markdown
# Project Status

## Related Documents
- [Project Proposal](ieapp://note/uuid-abc)
- [Budget Spreadsheet](ieapp://attachment/uuid-def)
```

**Enforcement**:
- Cannot delete Note if other Notes link to it (returns HTTP 409)
- Cannot delete Attachment if any Note references it (returns HTTP 409)

**Related Terms**: Note (connected by), Attachment (referenced by)

---

### Field

**Definition**: A named property in a Class that defines the type, requirements, and default value for content in Notes.

**Components**:
- **Name**: Property identifier (must not conflict with reserved metadata columns)
- **Type**: Iceberg-compatible type (string, date, list, markdown, etc.)
- **Required**: Whether field must be present in every Note
- **Default**: Optional default value if not provided

**Supported Types**: string, markdown, integer, long, float, double, number, date, time, timestamp, boolean, uuid, binary, list, object_list

**Reserved Field Names**: id, note_id, title, class, tags, links, attachments, created_at, updated_at, revision_id, parent_revision_id, deleted, deleted_at, author, canvas_position, integrity, workspace_id, word_count

**Markdown Mapping**:
```markdown
# Note Title

## Field Name      ← H2 header becomes field name
Field value        ← Content becomes field value
```

**Related Terms**: Class (defines), Note (provides values), Type (constrains)

---

## Relationships Diagram

```
┌─────────────────────────────────────────────────────────┐
│                    Workspace                            │
│                 (Isolation Boundary)                    │
│                                                         │
│  ┌─────────────────┐          ┌────────────────────┐   │
│  │    Classes      │          │   Attachments      │   │
│  │  (Templates)    │          │  (Binary Files)    │   │
│  │                 │          │                    │   │
│  │  ┌──────────┐   │          │  ┌──────────────┐  │   │
│  │  │ Meeting  │   │          │  │  audio.m4a   │  │   │
│  │  ├──────────┤   │          │  │  slides.pdf  │  │   │
│  │  │ Task     │   │          │  │  diagram.png │  │   │
│  │  └──────────┘   │          │  └──────────────┘  │   │
│  └────────┬─────────┘                    ▲              │
│           │                              │              │
│           │ defines                      │              │
│           │ structure                    │ references   │
│           ▼                              │              │
│  ┌────────────────────────────────────────┐             │
│  │            Notes                       │             │
│  │         (Instances)                    │             │
│  │                                        │             │
│  │  ┌──────────────────────────────────┐ │             │
│  │  │ Note: "Weekly Team Sync"         │ │             │
│  │  │ class: Meeting                   │─┼─────────────┘
│  │  │ Date: 2025-11-29                 │ │
│  │  │ Attachments: [audio.m4a]         │ │
│  │  └────────────┬─────────────────────┘ │
│  │               │                        │
│  │               │ Links (references)     │
│  │               ▼                        │
│  │  ┌──────────────────────────────────┐ │
│  │  │ Note: "Previous Meeting"         │ │
│  │  │ class: Meeting                   │ │
│  │  └──────────────────────────────────┘ │
│  │                                        │
│  │  Each Note has:                        │
│  │  • Revisions (version history)         │
│  │  • Fields (from Class)                 │
│  │  • Tags (user-defined)                 │
│  │  • Links (to other Notes)              │
│  │  • Attachment references               │
│  └────────────────────────────────────────┘
│                                                         │
└─────────────────────────────────────────────────────────┘
```

---

## Comparison Table

| Aspect | Note | Attachment | Class | Revision |
|--------|------|-----------|-------|----------|
| **Purpose** | Knowledge content | Binary files | Schema/template | Version history |
| **Format** | Markdown | Binary | YAML/JSON schema | Snapshot |
| **Versioning** | Yes (revisions) | No | Schema versions | N/A |
| **Structure** | Class-defined fields | Metadata only | Field definitions | Same as Note |
| **Editing** | User edits content | Immutable after upload | Admin/power users | Read-only |
| **Storage** | Iceberg tables | Filesystem | Iceberg metadata | Iceberg table |
| **Linking** | Can link to others | Linked from Notes | N/A | N/A |
| **Deletion** | Blocked if linked to | Blocked if referenced | Blocked if Notes exist | Never deleted |

---

## FAQ

### Why "Note" and not "Document" or "Object"?

**Answer**: 
- "Note" is widely understood in knowledge management tools (Notion, Obsidian, Evernote)
- Conveys the informal, Markdown-first nature of content
- Distinguishes from "Document" (more formal) and "Object" (too generic)
- Consistent with Class being the "type" and Note being the "instance"

### Why "Attachment" and not "Asset" or "File"?

**Answer**:
- "Attachment" clearly indicates the binary file is *attached to* Notes
- "Asset" is ambiguous (could be web assets, Notes themselves, etc.)
- "File" is accurate but doesn't convey the relationship to Notes
- Follows common pattern in email, messaging, and note-taking apps

### Can a Note exist without a Class?

**Answer**: Technically yes (via default Class or no class field), but best practice is to always assign a Class for structure and validation.

### Can I rename a Class?

**Answer**: Not directly; you must:
1. Create new Class with desired name
2. Migrate all Notes to new Class
3. Delete old Class

### What happens if I delete a Class with existing Notes?

**Answer**: Deletion fails (HTTP 409 Conflict). You must first delete or reclassify all Notes.

---

## Additional Resources

- **Architecture**: `docs/spec/architecture/overview.md`
- **Data Model**: `docs/spec/data-model/overview.md`
- **API Reference**: `docs/spec/api/rest.md`
- **Requirements**: `docs/spec/requirements/`
- **Terminology Evaluation**: `docs/terminology-evaluation.md` (or `-en.md` for English)

---

**Maintained by**: IEapp Development Team  
**Contributions**: Submit PRs to update this guide as terminology evolves
