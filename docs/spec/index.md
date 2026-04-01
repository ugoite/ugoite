# Ugoite Specification Index

**Version**: 2.0.0 (Full Configuration)  
**Updated**: February 2026  
**Status**: Milestone 3 - In Progress

## Vision

**"Local-First, AI-Native Knowledge Space for the Post-SaaS Era"**

Ugoite is a knowledge management system built on three core principles:

| Principle | Description |
|-----------|-------------|
| **Low Cost** | No expensive cloud services required; runs on local storage |
| **Easy** | Markdown-first with automatic structure extraction |
| **Freedom** | Your data, your storage, your AI - no vendor lock-in |

---

## Quick Navigation

### Getting Started & Concepts
- [Core Concepts](../guide/concepts.md) - Plain-language introduction to spaces, entries, forms, and search
- [Container Quick Start](../guide/container-quickstart.md) - Fastest published browser path, with backend + frontend runtime and explicit login
- [CLI Guide](../guide/cli.md) - Lightest local-first path when you want direct filesystem access in `core` mode
- [Local Dev Auth/Login](../guide/local-dev-auth-login.md) - Canonical local sign-in and `/login` flow for source development

### Entry-Path Trade-offs

| Path | Best for | Trade-off |
| --- | --- | --- |
| [Container Quick Start](../guide/container-quickstart.md) | Fast visual evaluation of the shipped UI | Requires the backend/runtime stack plus explicit login before `/spaces` becomes useful |
| [CLI Guide](../guide/cli.md) in `core` mode | Lowest-friction local-first workflow | Terminal-first only; skips the browser shell and server-backed behavior |
| [Run from source](../guide/docker-compose.md) with `mise run dev` | Contributor work and full-surface debugging | Highest setup and auth overhead, but exercises backend, frontend, and docsite together |

### Architecture & Design
- [Architecture Overview](architecture/overview.md) - System design and component responsibilities
- [Technology Stack](architecture/stack.md) - Frontend, backend, and storage technologies
- [Architecture Decisions](architecture/decisions.md) - Key design decisions and rationale
- [Frontend–Backend Interface](architecture/frontend-backend-interface.md) - Behavioral contracts and boundaries
- [Future-Proofing](architecture/future-proofing.md) - Experimental direction (BYOAI, multi-platform core)

### Features & Stories
- [Features Registry](features/README.md) - API-level feature registry across modules
- [Ugoite SQL](features/sql.md) - SQL dialect for structured queries
- [Core Stories](stories/core.yaml) - Essential user scenarios
- [Advanced Stories](stories/advanced.yaml) - Power user and experimental features

### UI Specifications
- [UI Overview](ui/README.md) - Page-level UI specifications (YAML)

### Data Model
- [Data Model Overview](data-model/overview.md) - How data is stored and structured
- [Directory Structure](data-model/directory-structure.md) - Space layout conventions
- [SQL Sessions & Materialized Views](data-model/sql-sessions.md) - SQL execution metadata

### API Reference
- [REST API](api/rest.md) - HTTP endpoints for frontend integration
- [MCP Protocol](api/mcp.md) - AI agent interface via Model Context Protocol
- [OpenAPI Spec](api/openapi.yaml) - Machine-readable API definition

### Requirements
- [Requirements Overview](requirements/README.md) - How requirements are tracked
- Requirements by category: [storage](requirements/storage.yaml) | [entry](requirements/entry.yaml) | [index](requirements/index.yaml) | [integrity](requirements/integrity.yaml) | [security](requirements/security.yaml) | [api](requirements/api.yaml) | [frontend](requirements/frontend.yaml) | [e2e](requirements/e2e.yaml) | [ops](requirements/ops.yaml) | [form](requirements/form.yaml) | [links](requirements/links.yaml) | [search](requirements/search.yaml)

### Governance Taxonomy
- [Philosophy](philosophy/foundation.yaml) - Constitutional-level ideals
- [Policies](policies/policies.yaml) - Implementation policies across layers
- [Requirements with embedded governance metadata](requirements/api.yaml) - Requirement sets are embedded per requirement item
- [Specifications Catalog](specifications.yaml) - Flat machine-readable specification catalog

### Versions & Release Notes
- [Versions Overview](versions/index.md) - How versions, milestones, phases, and tasks fit together
- [v0.1 Release Stream](versions/v0.1.md) - Current foundational release stream and milestone summary
- [v0.2 Roadmap](versions/v0.2.md) - Planned capabilities beyond the v0.1 stream
- [Changelog](versions/changelog.md) - Channel entrypoint for stable, beta, and alpha release notes

### Security & Quality
- [Security Overview](security/overview.md) - Security strategy and threat model
- [Testing Strategy](testing/strategy.md) - TDD approach and test organization
- [CI/CD](testing/ci-cd.md) - Continuous integration setup
- [Error Handling](quality/error-handling.md) - Error-handling principles and resilience

### Product
- [Success Metrics](product/success-metrics.md) - How we measure progress

---

## Module Responsibility Matrix

| Module | Responsibility | Language |
|--------|----------------|----------|
| `ugoite-minimum` | Portable domain models, integrity primitives, storage traits | Rust |
| `ugoite-core` | OpenDAL/Iceberg adapter layer, persistence, Python bindings | Rust |
| `ugoite-cli` | Command-line interface for direct user interaction | Rust |
| `backend` | REST API, MCP server (delegates to ugoite-core) | Python (FastAPI) |
| `frontend` | UI rendering, optimistic updates (no data logic) | TypeScript (SolidStart) |

---

## Key Concepts

### Form
A **Form** defines the canonical field contract for an entry type. Forms specify:
- Required and optional fields (H2 headers)
- Field types (string, number, date, list, markdown)
- Fixed global template for new entries

### Entry
An **Entry** is stored as a row in an Iceberg table and can be reconstructed as Markdown:
- H2 sections map to Form-defined fields
- YAML frontmatter carries metadata (form, tags)
- Revision history is stored in the Form `revisions` table

Markdown remains the authoring surface, but once an entry is associated with a
Form, that Form governs which fields become canonical structured data.

### Space
A **Space** is a self-contained data directory with:
- Iceberg-managed Form tables and assets
- Derived indexes regenerated from Iceberg tables
- Portable across storage backends (local, S3, etc.)

---

## Development Resources

- [Machine-readable v0.1 tracker](../version/v0.1.yaml) - Current milestone/phase/task state for the v0.1 stream
- [Machine-readable v0.2 tracker](../version/v0.2.yaml) - Planned milestone/phase/task state for the v0.2 stream
- [Versions & Release Notes](versions/index.md) - Human-readable summary of what each version adds or changes
- [Contributing](../../AGENTS.md) - Development guidelines

---

## Change History

| Date | Version | Changes |
|------|---------|---------|
| 2026-03 | 2.1.0 | Added policy traceability docs and version/release-notes navigation |
| 2026-01 | 2.0.0 | Restructured for Full Configuration milestone; unified terminology to "Form" |
| 2025-12 | 1.0.0 | Initial MVP specification |
