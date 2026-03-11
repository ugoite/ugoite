# Changelog

This page summarizes what each release stream adds, changes, or plans to add.
For task-level status, follow the machine-readable files under `docs/version/`.

## v0.2 (planned)

### Added

- Planned query-driven user-controlled views
- Planned shareable view definitions stored with the product data model
- Planned production-ready MCP resource and tool coverage
- Planned AI workflow automation, vector search, voice transcription, and
  computational entries

### Changed

- Shifts the release story from "foundational platform hardening" toward
  "user-controlled experiences and AI-native workflows"

### Planned

- Design, implementation, and testing work for the `user-controlled-view` and
  `ai-enabled-and-ai-used` milestones

## v0.1 (in progress)

### Added

- Local-first MVP across backend, frontend, CLI, and storage abstraction
- REST and MCP foundations plus sandboxed execution support
- Shared Rust `ugoite-core` architecture and unified Form terminology
- YAML-based requirements and automated requirement traceability
- Iceberg-backed entries, Ugoite SQL, redesigned space UI, theme switching, and
  sample data generation

### Changed

- Entry storage moved to a Form-first Iceberg-backed model
- Repository terminology was normalized to Space / Form / Entry / Asset
- Product specs became more machine-readable and more tightly coupled to tests

### Planned

- Release-preparation work such as deployable images, quick-start docs, and
  final release communication

### In Progress

- User-management completion for authentication, authorization, memberships,
  service accounts, and audit coverage
