# Milestone 3: Markdown as Table

**Status**: ✅ Done  
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
- [x] Define Iceberg table layout and schema per Class (notes + revisions tables).
- [x] Define `classes/` as the Iceberg-managed root and document ownership rules.
- [x] Standardize Class name → Iceberg table name mapping (no class_id directories).
- [x] Update `ieapp-core` write path to persist note records via Iceberg (official Rust crate + OpenDAL).
- [x] Update `ieapp-core` read path to reconstruct Markdown content from Iceberg fields.
- [x] Enforce “Class-defined H2 only” validation in `ieapp-core`.
- [x] Keep backend and frontend API contracts unchanged.
- [x] Add/update tests in `ieapp-core` to validate Iceberg round-trip.

### Legacy → TOBE (directory-structure) Delta
- **Remove per-note folders**: `notes/{note_id}/` with `meta.json`, `content.json`, and `history/` are no longer used.
- **Iceberg-managed classes root**: `classes/` is the Iceberg-managed root; Iceberg owns all subfolders and table metadata.
- **Table naming**: Class name is the Iceberg table name; no class_id directories are created.
- **Class definitions in Iceberg**: Class fields and schemas live in Iceberg; no per-class JSON files.
- **Fixed template**: Default note template is global (`# {class_name}` with H2 columns), not per class.
- **Reconstruction source**: Markdown is reconstructed from Iceberg fields (no free-form H2 storage in Phase 1).
- **No index JSON**: `index.json` and related index files are removed from TOBE; indexes are derived from Iceberg as needed.

### Acceptance Criteria
- [x] Notes are stored in Iceberg tables per Class.
- [x] Notes can be read back with identical Markdown content (current UI behavior preserved).
- [x] Non-Class H2 sections are rejected by `ieapp-core`.

---

## Phase 2: Optional extra attributes in Class schema

**Objective**: Allow Classes to declare whether extra attributes (non-registered H2 sections) are allowed, and how they are stored.

### Key Tasks
- [x] Extend Class definition to include `allow_extra_attributes` with options (e.g., `deny`, `allow_json`, `allow_columns`).
- [x] Update validation to enforce the new Class policy.
- [x] Implement storage for extra attributes (JSON column or dynamic columns, as specified).
- [x] Update documentation in `docs/spec/data-model/` for the new Class rules.
- [x] Add tests that cover both “deny” and “allow” modes.

### Acceptance Criteria
- [x] Class schema can explicitly allow or deny extra attributes.
- [x] Extra attributes are stored deterministically.
- [x] Validation behavior matches the Class policy.

---

## Phase 3: IEapp SQL (Domain-Specific SQL)

**Objective**: Define and implement an SQL dialect optimized for IEapp classes and Iceberg storage.

### Key Tasks
- [x] Define IEapp SQL syntax and capabilities (filter, sort, select, aggregate).
- [x] Map SQL queries to Iceberg scans in `ieapp-core`.
- [x] Add query validation and error reporting.
- [x] Integrate with existing REST/MCP query endpoints without API changes.
- [x] Add tests for SQL parsing and execution.

### Acceptance Criteria
- [x] Users can query Class data via IEapp SQL.
- [x] SQL execution returns consistent, deterministic results.
- [x] Query errors are clear and actionable.

---

## Phase 4: Metadata Columns, Rich Types, Link URIs, SQL Joins

**Objective**: Expand the Iceberg-backed data model with reserved metadata columns,
rich content column types with Markdown-friendly parsing, canonical IEapp link URIs,
and broadened IEapp SQL join capabilities.

### Key Tasks
- [x] Define metadata vs content column ownership rules and reserved names.
- [x] Prevent user-defined class fields from using metadata column names.
- [x] Make metadata column list extensible for future system-owned fields.
- [x] Expand content column types to additional Iceberg primitives (time, timestamp_tz, timestamp_ns, uuid, binary, etc.).
- [x] Update Markdown parsing to produce typed values (including bullet-list parsing for list fields).
- [x] Introduce IEapp URI scheme for in-note links (note, attachment, extensible kinds) and normalize links on write/read.
- [x] Extend IEapp SQL to support richer JOIN clauses (RIGHT/FULL/CROSS, USING/NATURAL).
- [x] Update shared SQL lint/completion rules to reflect JOIN support and base tables.
- [x] Add tests for metadata column validation, rich type parsing, link URI normalization, and JOIN execution.
- [x] Update frontend UX to enforce class-first note creation and surface class validation warnings.
- [x] Add frontend guardrails for reserved metadata column names and list-friendly field types.

### Acceptance Criteria
- [x] Metadata columns are reserved and cannot be used as user-defined Class fields.
- [x] Content columns support expanded Iceberg types with deterministic Markdown parsing.
- [x] IEapp link URIs are normalized and persisted consistently.
- [x] IEapp SQL supports JOIN queries across notes, links, and attachments.
- [x] Frontend note creation is class-first, and validation feedback is visible in the editor UX.
- [x] Class creation/editing UI blocks reserved metadata column names.

---

## Phase 5: SQL Class (Metadata Class) + CRUD

**Objective**: Define and implement a system-owned SQL Class to persist SQL queries
and variables with full CRUD support, while preventing user-defined Classes from
using the reserved SQL class name.

### Key Tasks
- [x] Define the SQL Class schema as a metadata Class with reserved name protection.
- [x] Add SQL variable object-list type and validation rules in the data model spec.
- [x] Extend REST API and ieapp-core with SQL CRUD operations.
- [x] Add tests covering SQL CRUD and reserved SQL Class name rejection.

### Acceptance Criteria
- [x] SQL Class is system-owned; users cannot create a Class with the SQL name.
- [x] SQL records store SQL text and a list of typed variables (type, name, description).
- [x] SQL CRUD operations are available via API and core bindings.
- [x] Tests confirm reserved class name enforcement and SQL CRUD behavior.

---

## Definition of Done

- [x] All phases completed with acceptance criteria met.
- [x] Tests pass (unit, integration, e2e).
- [x] Documentation updated and consistent with the new storage model.

---

## References

- [Roadmap](roadmap.md)
- [Specification Index](../spec/index.md)
- [Data Model Overview](../spec/data-model/overview.md)
