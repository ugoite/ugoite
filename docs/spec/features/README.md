# Features Registry

This directory contains the feature definitions for Ugoite.

## Inventory

The API feature inventory is authoritative in [features.yaml](features.yaml). The
docsite feature pages load their domains from that manifest, so the list below
must stay in sync with it.

- [features.yaml](features.yaml) - Registry manifest and conventions
- [spaces.yaml](spaces.yaml) - Space APIs
- [preferences.yaml](preferences.yaml) - Portable user preference APIs
- [entries.yaml](entries.yaml) - Entry APIs
- [forms.yaml](forms.yaml) - Form APIs
- [assets.yaml](assets.yaml) - Asset APIs
- [auth.yaml](auth.yaml) - Explicit login APIs
- [search.yaml](search.yaml) - Search + structured query APIs
- [sql.yaml](sql.yaml) - Saved SQL and SQL session APIs

## Supplemental References

- [links.yaml](links.yaml) - Legacy link API registry kept for traceability after
  the product moved to `row_reference` fields instead of dedicated link APIs
- [sql.md](sql.md) - Ugoite SQL dialect reference used by the saved SQL features
- [Frontend–Backend Interface](../architecture/frontend-backend-interface.md) -
  Canonical browser authoring-mode contract for Markdown, Web form, and Chat
  Q&A submission behavior
- [frontend.yaml](../requirements/frontend.yaml) - Frontend interaction
  requirements, including REQ-FE-037 for create-entry modes and REQ-FE-057 for
  the chat create-entry flow

## Purpose

The features registry serves multiple purposes:

1. **Structural Consistency**: Ensures all modules follow the same naming conventions
2. **Navigation**: Helps developers find related code across modules
3. **Automated Verification**: Tests can verify that paths match the registry

## Registry Structure

The registry is API-operation oriented.

That means it is the canonical inventory for backend/frontend/API path surfaces,
not the full catalog of browser authoring behaviors. Browser entry authoring
modes such as Markdown, Web form, and Chat Q&A are intentionally tracked in the
Frontend–Backend Interface contract plus the frontend requirements set instead
of this API manifest.

Each operation entry includes:

- **ID & Method**: Unique identifier and HTTP method.
- **Backend & Frontend**: URL path, implementation file, and function/component.

**Frontend path semantics**: The frontend path is the UI route path (no `/api` prefix).
It should mirror the backend path to keep functionality aligned and discoverable.
- **ugoite-core**: Internal logic implementation (Rust).
- **ugoite-cli**: Optional command-line interface usage and implementation when the API is exposed through the CLI.

Example:

```yaml
apis:
  - id: entry.create
    method: POST
    backend:
      path: /spaces/{space_id}/entries
      file: backend/src/app/api/endpoints/entry.py
      function: create_entry_endpoint
    frontend:
      path: /spaces/{space_id}/entries
      file: frontend/src/routes/spaces/[space_id]/entries.tsx
      function: SpaceEntriesRoute
    ugoite_core:
      file: ugoite-core/src/entry.rs
      function: create_entry
    ugoite_cli:
      command: ugoite entry create
      file: ugoite-cli/src/commands/entry.rs
      function: run (EntrySubCmd::Create)
```

If an API operation has no CLI surface yet, omit the `ugoite_cli` block instead of
pointing to an unrelated command.

## Verification Tests

Tests verify:

1. All declared paths exist in the codebase
2. No undeclared feature modules exist
3. Naming conventions are consistent

## Project Lifecycle

When implementing features:

1. Update the registry to match implementation
2. Run verification tests to confirm alignment
