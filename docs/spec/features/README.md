# Features Registry

This directory contains the feature definitions for IEapp.

## Files

- [features.yaml](features.yaml) - Registry manifest and conventions
- [workspaces.yaml](workspaces.yaml) - Workspace APIs
- [notes.yaml](notes.yaml) - Note APIs
- [classes.yaml](classes.yaml) - Class APIs
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

Each operation entry includes:

- **ID & Method**: Unique identifier and HTTP method.
- **Backend & Frontend**: URL path, implementation file, and function/component.
- **ieapp-core**: Internal logic implementation (Rust).
- **ieapp-cli**: Command-line interface usage and implementation.

Example:

```yaml
apis:
  - id: note.create
    method: POST
    backend:
      path: /workspaces/{workspace_id}/notes
      file: backend/src/app/api/endpoints/note.py
      function: create_note_endpoint
    frontend:
      path: /api/workspaces/{workspace_id}/notes
      file: frontend/src/lib/client.ts
      function: noteApi.create
    ieapp_core:
      file: ieapp-core/src/note.rs
      function: create_note
    ieapp_cli:
      command: ieapp note create
      file: ieapp-cli/src/ieapp/cli.py
      function: cmd_note_create
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
