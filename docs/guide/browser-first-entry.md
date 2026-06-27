---
title: 'Create the first browser entry'
---

The shipped browser is server-backed.

1. Start Ugoite with `mise run dev` or Docker Compose.
2. Open the URL printed by the launcher, or the configured release Compose port.
3. Sign in through development mock OAuth when enabled.
4. Select a Space for which the identity is a member.
5. Select or create a Form, then create an Entry.

The browser sends REST requests to the Rust server. Entry data and revisions are written under `UGOITE_ROOT`; browser storage is not authoritative in this release.

Creating Spaces through REST requires `ManageSpace` on reserved `admin-space`, so startup bootstrap is the normal fresh-development path.

## Form field choices

Use short text/number/date fields for values that should sort, filter, or validate predictably. Use a Markdown field for longer notes, lists, links, and formatted prose. Both remain part of the Form-defined Entry; the distinction is about editing and query behavior.

## Main browser surfaces

- **Dashboard** summarizes the selected Space.
- **Entries** lists and edits content and revision history.
- **Forms** defines typed fields used by Entries.
- **Search** performs keyword and structured query workflows.
- **SQL** stores reusable SQL definitions and opens query results.
- **Assets** uploads and manages Space-owned files.
- **Settings** shows Space storage and membership controls allowed by the current role.
