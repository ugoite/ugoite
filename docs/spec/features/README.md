# Features Registry

This directory contains the feature definitions for IEapp.

## Files

- [features.yaml](features.yaml) - Registry manifest and conventions
- [system.yaml](system.yaml) - System endpoints (health)
- [workspaces.yaml](workspaces.yaml) - Workspace APIs
- [notes.yaml](notes.yaml) - Note APIs
- [schemas.yaml](schemas.yaml) - Schema/Class APIs
- [attachments.yaml](attachments.yaml) - Attachment APIs
- [links.yaml](links.yaml) - Link APIs
- [search.yaml](search.yaml) - Search + structured query APIs

## Purpose

The features registry serves multiple purposes:

1. **Structural Consistency**: Ensures all modules follow the same naming conventions
2. **Navigation**: Helps developers find related code across modules
3. **Automated Verification**: Tests can verify that paths match the registry

## Registry Structure

The registry is API-operation oriented.

Each operation entry is a row with these "columns":

- PATH (AS-IS): backend and frontend paths are identical except frontend is
  prefixed with `/api`.
- File definition location: where the operation is defined in each module.
- Function name (or component name): the symbol implementing it.

Example:

```yaml
apis:
  - id: note.create
    method: POST
    path: /workspaces/{workspace_id}/notes
    paths:
      backend: /workspaces/{workspace_id}/notes
      frontend: /api/workspaces/{workspace_id}/notes
    backend:
      file: backend/src/app/api/endpoints/workspaces.py
      function: create_note_endpoint
    frontend:
      file: frontend/src/lib/client.ts
      function: noteApi.create
    ieapp_cli:
      file: ieapp-cli/src/ieapp/notes.py
      function: create_note
```

## Notes

- This registry currently describes the AS-IS state. When the codebase is
  refactored (Milestone 2), the same format will be kept but the file/symbol
  values will change.

## Verification Tests

Tests in `docs/tests/test_features.py` verify:

1. All declared paths exist in the codebase
2. No undeclared feature modules exist
3. Naming conventions are consistent

## Migration Process

When migrating to the new structure:

1. Update code to match target paths in `features:`
2. Remove or update `current_paths:` section
3. Run verification tests to confirm alignment
