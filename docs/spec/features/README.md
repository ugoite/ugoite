# Features Registry

This directory contains the feature definitions for IEapp.

## Files

- [features.yaml](features.yaml) - Registry manifest and conventions
- [spaces.yaml](spaces.yaml) - Space APIs
- [entries.yaml](entries.yaml) - Entry APIs
- [forms.yaml](forms.yaml) - Form APIs
- [assets.yaml](assets.yaml) - Asset APIs
- [links.yaml](links.yaml) - Link APIs
- [search.yaml](search.yaml) - Search + structured query APIs
- [sql.md](sql.md) - IEapp SQL dialect

## Purpose

The features registry serves multiple purposes:

1. **Structural Consistency**: Ensures all modules follow the same naming conventions
2. **Navigation**: Helps developers find related code across modules
3. **Automated Verification**: Tests can verify that paths match the registry

## Registry Structure

The registry is API-operation oriented.

Each operation entry includes:

- **ID & Method**: Unique identifier and HTTP method.
- **Backend & Frontend**: URL path, implementation file, and function/component.

**Frontend path semantics**: The frontend path is the UI route path (no `/api` prefix).
It should mirror the backend path to keep functionality aligned and discoverable.
- **ieapp-core**: Internal logic implementation (Rust).
- **ieapp-cli**: Command-line interface usage and implementation.

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
    ieapp_core:
      file: ieapp-core/src/entry.rs
      function: create_entry
    ieapp_cli:
      command: ieapp entry create
      file: ieapp-cli/src/ieapp/cli.py
      function: cmd_entry_create
```

## Verification Tests

Tests verify:

1. All declared paths exist in the codebase
2. No undeclared feature modules exist
3. Naming conventions are consistent

## Project Lifecycle

When implementing features:

1. Update the registry to match implementation
2. Run verification tests to confirm alignment
