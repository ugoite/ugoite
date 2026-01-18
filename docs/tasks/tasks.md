# Milestone 3: Markdown as Table

**Status**: 📋 Planned  
**Goal**: Store notes as Parquet-backed tables while preserving the current UI behavior

This milestone replaces the current Markdown-based storage with a Parquet table model, while keeping user experience unchanged. Notes become row-based records defined by Classes, and queryable via a domain-specific SQL.

---

## Constraints (MUST)

- **No migration path required**: We do not provide any conversion from the current storage format.
- **Breaking change is acceptable**: Existing users and data are out of scope.
- **Class-first**: Notes can only be created for a defined Class. The current “classless note” flow is removed.
- **Phase 1 UI lock**: Initial implementation must keep the UI behavior *exactly* as it is today. Only `ieapp-core` storage changes.

---

## Phase 1: Parquet storage for class-defined fields only

**Objective**: Replace note storage with Parquet in `ieapp-core`, limited to fields defined by the Class schema. H2 sections not in the Class are rejected.

### Key Tasks
- [ ] Design a Parquet layout for notes per Class (one table per Class).
- [ ] Define storage location and directory structure for Parquet files in workspaces.
- [ ] Update `ieapp-core` write path to persist note records to Parquet.
- [ ] Update `ieapp-core` read path to reconstruct Markdown content from Parquet fields.
- [ ] Enforce “Class-defined H2 only” validation in `ieapp-core`.
- [ ] Keep backend and frontend API contracts unchanged.
- [ ] Add/update tests in `ieapp-core` to validate Parquet round-trip.

### Legacy → TOBE (directory-structure) Delta
- **Remove per-note folders**: `notes/{note_id}/` with `meta.json`, `content.json`, and `history/` are no longer used.
- **Class-first layout**: `classes/` is now keyed by `class_id` (not by name), and each Class owns its storage.
- **Parquet shards**: `classes/{class_id}/notes/{idx}.parquet` stores current note rows; `revisions/{idx}.parquet` stores revision history.
- **Reconstruction source**: Markdown is reconstructed from Class-defined fields stored in Parquet (no free-form H2 storage in Phase 1).
- **No index JSON**: `index.json` and related index files are removed from TOBE; indexes are derived from Parquet as needed.

### Acceptance Criteria
- [ ] Notes are stored in Parquet tables per Class.
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

**Objective**: Define and implement an SQL dialect optimized for IEapp classes and Parquet storage.

### Key Tasks
- [ ] Define IEapp SQL syntax and capabilities (filter, sort, select, aggregate).
- [ ] Map SQL queries to Parquet scans in `ieapp-core`.
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
