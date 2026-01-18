# Milestone 3: Markdown as Table

**Status**: 📋 Planned  
**Goal**: Store notes as Iceberg-backed tables while preserving the current UI behavior

This milestone replaces the current Markdown-based storage with an Apache Iceberg table model (official Rust crate + OpenDAL), while keeping user experience unchanged. Notes become row-based records defined by Classes, and queryable via a domain-specific SQL.

---

## Constraints (MUST)

- **No migration path required**: We do not provide any conversion from the current storage format.
- **Breaking change is acceptable**: Existing users and data are out of scope.
- **Class-first**: Notes can only be created for a defined Class. The current “classless note” flow is removed.
- **Phase 1 UI lock**: Initial implementation must keep the UI behavior *exactly* as it is today. Only `ieapp-core` storage changes.

---

## Phase 1: Iceberg storage for class-defined fields only

**Objective**: Replace note storage with Apache Iceberg in `ieapp-core`, limited to fields defined by the Class schema. H2 sections not in the Class are rejected.

### Key Tasks
- [ ] Define Iceberg table layout and schema per Class (`notes`, `revisions`).
- [ ] Define `classes/` as the Iceberg-managed root and document ownership rules.
- [ ] Standardize Class name → Iceberg table name mapping (no class_id directories).
- [ ] Update `ieapp-core` write path to persist note records via Iceberg (official Rust crate + OpenDAL).
- [ ] Update `ieapp-core` read path to reconstruct Markdown content from Iceberg fields.
- [ ] Enforce “Class-defined H2 only” validation in `ieapp-core`.
- [ ] Keep backend and frontend API contracts unchanged.
- [ ] Add/update tests in `ieapp-core` to validate Iceberg round-trip.

### Legacy → TOBE (directory-structure) Delta
- **Remove per-note folders**: `notes/{note_id}/` with `meta.json`, `content.json`, and `history/` are no longer used.
- **Iceberg-managed classes root**: `classes/` is the Iceberg-managed root; Iceberg owns all subfolders and table metadata.
- **Table naming**: Class name is the Iceberg table name; no class_id directories are created.
- **Class definitions in Iceberg**: Class fields and schemas live in Iceberg; no per-class JSON files.
- **Fixed template**: Default note template is global (`# {class_name}` with H2 columns), not per class.
- **Reconstruction source**: Markdown is reconstructed from Iceberg fields (no free-form H2 storage in Phase 1).
- **No index JSON**: `index.json` and related index files are removed from TOBE; indexes are derived from Iceberg as needed.

### Acceptance Criteria
- [ ] Notes are stored in Iceberg tables per Class.
- [ ] Notes can be read back with identical Markdown content (current UI behavior preserved).
- [ ] Non-Class H2 sections are rejected by `ieapp-core`.

---

## Phase 2: Optional extra attributes in Class schema

**Objective**: Allow Classes to declare whether extra attributes (non-registered H2 sections) are allowed, and how they are stored.

### Key Tasks
- [ ] Extend Class definition to include `allow_extra_attributes` with options (e.g., `deny`, `allow_json`, `allow_columns`).
- [ ] Update validation to enforce the new Class policy.
- [ ] Implement storage for extra attributes (JSON column or dynamic columns, as specified).
- [ ] Update documentation in `docs/spec/data-model/` for the new Class rules.
- [ ] Add tests that cover both “deny” and “allow” modes.

### Acceptance Criteria
- [ ] Class schema can explicitly allow or deny extra attributes.
- [ ] Extra attributes are stored deterministically.
- [ ] Validation behavior matches the Class policy.

---

## Phase 3: IEapp SQL (Domain-Specific SQL)

**Objective**: Define and implement an SQL dialect optimized for IEapp classes and Iceberg storage.

### Key Tasks
- [ ] Define IEapp SQL syntax and capabilities (filter, sort, select, aggregate).
- [ ] Map SQL queries to Iceberg scans in `ieapp-core`.
- [ ] Add query validation and error reporting.
- [ ] Integrate with existing REST/MCP query endpoints without API changes.
- [ ] Add tests for SQL parsing and execution.

### Acceptance Criteria
- [ ] Users can query Class data via IEapp SQL.
- [ ] SQL execution returns consistent, deterministic results.
- [ ] Query errors are clear and actionable.

---

## Definition of Done

- [ ] All phases completed with acceptance criteria met.
- [ ] Tests pass (unit, integration, e2e).
- [ ] Documentation updated and consistent with the new storage model.

---

## References

- [Roadmap](roadmap.md)
- [Specification Index](../spec/index.md)
- [Data Model Overview](../spec/data-model/overview.md)
