# Milestone 3: Markdown as Table

**Status**: 🟡 In Progress  
**Goal**: Store entries as Iceberg-backed tables while preserving the current UI behavior

This milestone replaces the current Markdown-based storage with an Apache Iceberg table model (official Rust crate + OpenDAL), while keeping user experience unchanged. Entries become row-based records defined by Forms, and queryable via a domain-specific SQL.

---

## Constraints (MUST)

- **No migration path required**: We do not provide any conversion from the current storage format.
- **Breaking change is acceptable**: Existing users and data are out of scope.
- **Form-first**: Entries can only be created for a defined Form. The current “formless entry” flow is removed.
- **Phase 1 UI lock**: Initial implementation must keep the UI behavior *exactly* as it is today. Only `ieapp-core` storage changes.

---

## Phase 1: Iceberg storage for form-defined fields only

**Objective**: Replace entry storage with Apache Iceberg in `ieapp-core`, limited to fields defined by the Form schema. H2 sections not in the Form are rejected.

### Key Tasks
- [x] Define Iceberg table layout and schema per Form (entries + revisions tables).
- [x] Define `forms/` as the Iceberg-managed root and document ownership rules.
- [x] Standardize Form name → Iceberg table name mapping (no form_id directories).
- [x] Update `ieapp-core` write path to persist entry records via Iceberg (official Rust crate + OpenDAL).
- [x] Update `ieapp-core` read path to reconstruct Markdown content from Iceberg fields.
- [x] Enforce “Form-defined H2 only” validation in `ieapp-core`.
- [x] Keep backend and frontend API contracts unchanged.
- [x] Add/update tests in `ieapp-core` to validate Iceberg round-trip.

### Legacy → TOBE (directory-structure) Delta
- **Remove per-entry folders**: `entries/{entry_id}/` with `meta.json`, `content.json`, and `history/` are no longer used.
- **Iceberg-managed forms root**: `forms/` is the Iceberg-managed root; Iceberg owns all subfolders and table metadata.
- **Table naming**: Form name is the Iceberg table name; no form_id directories are created.
- **Form definitions in Iceberg**: Form fields and schemas live in Iceberg; no per-form JSON files.
- **Fixed template**: Default entry template is global (`# {form_name}` with H2 columns), not per form.
- **Reconstruction source**: Markdown is reconstructed from Iceberg fields (no free-form H2 storage in Phase 1).
- **No index JSON**: `index.json` and related index files are removed from TOBE; indexes are derived from Iceberg as needed.

### Acceptance Criteria
- [x] Entries are stored in Iceberg tables per Form.
- [x] Entries can be read back with identical Markdown content (current UI behavior preserved).
- [x] Non-Form H2 sections are rejected by `ieapp-core`.

---

## Phase 2: Optional extra attributes in Form schema

**Objective**: Allow Forms to declare whether extra attributes (non-registered H2 sections) are allowed, and how they are stored.

### Key Tasks
- [x] Extend Form definition to include `allow_extra_attributes` with options (e.g., `deny`, `allow_json`, `allow_columns`).
- [x] Update validation to enforce the new Form policy.
- [x] Implement storage for extra attributes (JSON column or dynamic columns, as specified).
- [x] Update documentation in `docs/spec/data-model/` for the new Form rules.
- [x] Add tests that cover both “deny” and “allow” modes.

### Acceptance Criteria
- [x] Form schema can explicitly allow or deny extra attributes.
- [x] Extra attributes are stored deterministically.
- [x] Validation behavior matches the Form policy.

---

## Phase 3: IEapp SQL (Domain-Specific SQL)

**Objective**: Define and implement an SQL dialect optimized for IEapp forms and Iceberg storage.

### Key Tasks
- [x] Define IEapp SQL syntax and capabilities (filter, sort, select, aggregate).
- [x] Map SQL queries to Iceberg scans in `ieapp-core`.
- [x] Add query validation and error reporting.
- [x] Integrate with existing REST/MCP query endpoints without API changes.
- [x] Add tests for SQL parsing and execution.

### Acceptance Criteria
- [x] Users can query Form data via IEapp SQL.
- [x] SQL execution returns consistent, deterministic results.
- [x] Query errors are clear and actionable.

---

## Phase 4: Metadata Columns, Rich Types, Link URIs, SQL Joins

**Objective**: Expand the Iceberg-backed data model with reserved metadata columns,
rich content column types with Markdown-friendly parsing, canonical IEapp link URIs,
and broadened IEapp SQL join capabilities.

### Key Tasks
- [x] Define metadata vs content column ownership rules and reserved names.
- [x] Prevent user-defined form fields from using metadata column names.
- [x] Make metadata column list extensible for future system-owned fields.
- [x] Expand content column types to additional Iceberg primitives (time, timestamp_tz, timestamp_ns, uuid, binary, etc.).
- [x] Update Markdown parsing to produce typed values (including bullet-list parsing for list fields).
- [x] Introduce IEapp URI scheme for in-entry links (entry, asset, extensible kinds) and normalize links on write/read.
- [x] Extend IEapp SQL to support richer JOIN clauses (RIGHT/FULL/CROSS, USING/NATURAL).
- [x] Update shared SQL lint/completion rules to reflect JOIN support and base tables.
- [x] Add tests for metadata column validation, rich type parsing, link URI normalization, and JOIN execution.
- [x] Update frontend UX to enforce form-first entry creation and surface form validation warnings.
- [x] Add frontend guardrails for reserved metadata column names and list-friendly field types.

### Acceptance Criteria
- [x] Metadata columns are reserved and cannot be used as user-defined Form fields.
- [x] Content columns support expanded Iceberg types with deterministic Markdown parsing.
- [x] IEapp link URIs are normalized and persisted consistently.
- [x] IEapp SQL supports JOIN queries across entries, links, and assets.
- [x] Frontend entry creation is form-first, and validation feedback is visible in the editor UX.
- [x] Form creation/editing UI blocks reserved metadata column names.

---

## Phase 5: SQL Form (Metadata Form) + CRUD

**Objective**: Define and implement a system-owned SQL Form to persist SQL queries
and variables with full CRUD support, while preventing user-defined Forms from
using the reserved SQL form name.

### Key Tasks
- [x] Define the SQL Form schema as a metadata Form with reserved name protection.
- [x] Add SQL variable object-list type and validation rules in the data model spec.
- [x] Extend REST API and ieapp-core with SQL CRUD operations.
- [x] Add tests covering SQL CRUD and reserved SQL Form name rejection.

### Acceptance Criteria
- [x] SQL Form is system-owned; users cannot create a Form with the SQL name.
- [x] SQL records store SQL text and a list of typed variables (type, name, description).
- [x] SQL CRUD operations are available via API and core bindings.
- [x] Tests confirm reserved form name enforcement and SQL CRUD behavior.

---

## Phase 5.5: SQL Session Redesign

**Objective**: Redefine SQL session handling so it remains stateless beyond
OpenDAL storage, without relying on RDBs or external job queues.

### Key Tasks
- [x] Sessions store **metadata only** in `meta.json` (no result rows).
- [x] `create_sql` creates corresponding **materialized view metadata** under
	`spaces/{space_id}/materialized_views/`.
- [x] SQL updates/deletes synchronize materialized view metadata refresh/removal.
- [x] Session metadata stores snapshot ID and paging hints for fast re-queries.
- [x] Sessions are short-lived (target: ~10 minutes) and shareable across API servers.
- [x] Update `docs/spec` data model, API, and SQL docs to reflect the redesign.

### Acceptance Criteria
- [x] SQL sessions store metadata only and are re-executable.
- [x] `materialized_views/` lifecycle is synchronized with saved SQL.
- [x] Session metadata includes snapshot and paging details.
- [x] Specs in `docs/spec` are updated consistently.

---

## Phase 6: UI Redesign Spec + Validation

**Objective**: Define page-level UI specs for the new simplified space UI,
and add automated validation that frontend tests load and verify the spec.

### Key Tasks
- [x] Define page-level YAML specs under `docs/spec/ui/pages/`.
- [x] Add an implementation status flag to each page spec (default: unimplemented).
- [x] Document the UI spec entry point in the spec index.
- [x] Add frontend tests that load all UI spec YAML files and validate links and component types.

### Acceptance Criteria
- [x] Each space UI page is defined in a YAML spec.
- [x] Specs include shared space UI chrome (top tabs + settings button).
- [x] Frontend tests validate page links and component type registry.

---

## Phase 7: UI Redesign Implementation (Planned)

**Objective**: Implement the new UI described in the page-level YAML specs.

### Key Tasks
- [x] Build the new space-wide layout with floating top tabs and settings button.
- [x] Implement the dashboard view with prominent space name.
- [x] Implement query list, query create, and query variable input flows.
- [x] Implement object (entries) view with grid list and entry detail navigation.
- [x] Implement form grid view with search/sort/filter, copy-paste grid, and CSV export.
- [x] Wire bottom view tabs between object and grid.
- [x] Connect UI components to existing APIs without changing backend contracts.

### Acceptance Criteria
- [x] UI matches the new simplified layout and navigation model.
- [x] All workflows are functional with existing backend APIs.

---

## Phase 7.5: UI Polish (Added)

**Objective**: Keep the existing UI structure while adding mobile responsiveness and unified theming, with a settings icon that lets users switch UI themes.

### Key Tasks
- [ ] Mobile responsiveness (top bar/nav/cards/forms)
- [ ] Define theme tokens using the recommended Tailwind v4 `@theme` pattern
- [ ] Unify colors/shadows/radii/sizing via theme tokens (no `@apply`)
- [ ] Add a settings icon in the top bar to switch UI themes
- [ ] UI themes: `materialize` / `classic` / `pop`
- [ ] Independent `light` / `dark` tone switching
- [ ] Persist selection state (localStorage)

### Acceptance Criteria
- [ ] All screens are usable on mobile without layout breakage
- [ ] Theme switching updates the entire UI immediately
- [ ] Theme consistency is preserved without `@apply`
- [ ] Theme selection is available from the settings icon

---

## Phase 7.6: Sample Data Space Generator

**Objective**: Provide a dynamic sample-data generator that creates a new space with 3–6 forms and roughly 5,000 entries, using a neutral, meaningful story that demonstrates the app without touching privacy, hierarchy, or ideology.

### Key Tasks
- [ ] Define a neutral, operational scenario (non-personal data) and form schema set (3–6 forms)
- [ ] Implement dynamic sample data generation in `ieapp-core` (seeded randomness, configurable entry count)
- [ ] Add REST API endpoint to create a sample-data space with a few parameters
- [ ] Add CLI command to generate a sample-data space
- [ ] Add UI flow to generate a sample-data space (name, scenario, size, seed)
- [ ] Add tests and requirement mappings (core + API + CLI + frontend)

### Acceptance Criteria
- [ ] A sample-data space can be generated from UI/CLI/API with a few inputs
- [ ] The generated space contains 3–6 forms and approximately 5,000 entries by default
- [ ] The data is dynamically generated (seeded, not fixed)
- [ ] Scenario content avoids privacy, hierarchy, and ideology

---

## Phase 8: Terminology Rebrand (Space/Form/Entry/Asset)

**Objective**: Rename the core terminology across specs, docs, code, file paths, and data model
without migration, removing the old labels entirely before production.

### Rebrand Plan
- **Scope**: Entire repository (docs/spec, API, frontend, backend, ieapp-core, ieapp-cli, tests, e2e, scripts, data-model paths)
- **Owner**: @tohboeh5 / Agent
- **Verification**:
	- `mise run test` passes
	- `mise run e2e` passes
	- No legacy terms remain after string search

### Work Plan (Phase 8 execution)
- [ ] Update spec terminology cross-check (docs/spec index, data-model, API, UI specs)
- [ ] Rename API routes/endpoints + payload fields (backend + OpenAPI + MCP docs)
- [ ] Rename core storage paths and Iceberg/OpenDAL references
- [ ] Rename frontend UI routes, components, copy, and types
- [ ] Rename CLI commands and help text
- [ ] Rename tests, fixtures, and test data to match new terms
- [ ] Sweep and remove legacy terms from repo (string + filename search)
- [ ] Validate with `mise run test` and `mise run e2e`

### Key Tasks
- [ ] Add a rebrand plan with scope, owner, and verification steps.
- [ ] Update specs under `docs/spec/` to use Space/Form/Entry/Asset terminology.
- [ ] Update docs, UI copy, and API descriptions to the new terms.
- [ ] Rename code symbols, route paths, filenames, and datamodel references (OpenDAL/Iceberg) to match.
- [ ] Remove all legacy terms from the repository (pre-rebrand labels).
- [ ] Update tests and fixtures to match new names and semantics.
- [ ] Verify `mise run test` and `mise run e2e` pass.

### Acceptance Criteria
- [ ] No legacy terms remain in the repository (including file names and data paths).
- [ ] All specs and docs consistently use Space/Form/Entry/Asset.
- [ ] Tests pass for unit, integration, and e2e.

---

## Phase 9: Full Repository Rebrand Completion (Legacy → Space/Form/Entry/Asset)

**Objective**: Remove all legacy terms and complete the repository-wide rename to
Space/Form/Entry/Asset across docs/spec, code, API, UI, tests, fixtures, and
storage paths (OpenDAL/Iceberg).

### Work Summary (Phase 9 execution)
- [ ] Sweep docs/spec for legacy terms and update references, examples, and diagrams.
- [ ] Rename API routes, payload fields, and OpenAPI/MCP docs to new terms.
- [ ] Rename code symbols, modules, and file paths across backend, frontend, ieapp-core, ieapp-cli.
- [ ] Update storage paths, datamodel references, and OpenDAL/Iceberg layouts.
- [ ] Update tests, fixtures, and test data to new terms and semantics.
- [ ] Verify no legacy terms remain via full-repo search.
- [ ] Confirm `mise run test` and `mise run e2e` pass before push.

### Acceptance Criteria
- [ ] Zero occurrences of legacy terms in repository (including file names and data paths).
- [ ] Specs/docs/API/UI all use Space/Form/Entry/Asset consistently.
- [ ] All tests (unit/integration/e2e) pass.

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
