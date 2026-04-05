# Documentation Consistency Tests

This directory contains the documentation consistency suite that runs through
`mise run test:docs` locally and the Python CI workflow in GitHub Actions.

## Purpose

These tests ensure:

1. **Requirements Coverage**: Every requirement has at least one test
2. **Test Traceability**: Tests reference valid requirements
3. **Feature Registry Consistency**: API registries match code (`docs/spec/features/features.yaml` + per-kind YAML files)
4. **Guide + Workflow Consistency**: Developer docs and shell snippets stay aligned with shipped commands and CI
5. **Domain Contract Checks**: API, authz, MCP, storage, version, governance, and Helm docs stay aligned with the current implementation
6. **Artifact Hygiene**: Repository guard scripts stay wired and enforce a clean tree

## Current Test Files

| File | Purpose |
|------|---------|
| `test_requirements.py` | Verify requirement → test mappings and REQ references across backend/frontend/docsite/Rust suites |
| `test_features.py` | Verify feature registry paths and referenced symbols exist in the codebase |
| `test_guides.py` | Verify developer guides, workflow docs, release docs, and shell snippets stay aligned and parse with `bashlex` |
| `test_api_docs.py` | Verify REST/OpenAPI docs stay aligned with implemented API contracts |
| `test_authz_docs.py` | Verify authorization and ACL docs describe the shipped mutation boundaries |
| `test_mcp_docs.py` | Verify MCP docs describe the current resource safety contract |
| `test_storage_docs.py` | Verify storage architecture/layout docs match runtime behavior and schemas |
| `test_versions.py` | Verify version docs, changelog sources, and README version links stay aligned |
| `test_spec_governance.py` | Verify philosophy/policy/specification/requirement taxonomy links stay complete |
| `test_helm_chart.py` | Verify Helm chart docs/topology stay aligned with repository-owned deployment artifacts |
| `test_cli_language_docs.py` | Verify top-level docs and SBOM metadata classify `ugoite-cli` consistently as Rust |
| `test_artifact_hygiene.py` | Verify repository hygiene guard scripts catch dirty-tree artifacts correctly |

## Running Tests

```bash
# CI-aligned shortcut
mise run test:docs

# Equivalent direct invocation
report="$(mktemp)"
trap 'rm -f "$report"' EXIT
uv run --with pytest --with pyyaml --with bashlex pytest docs/tests -v -W error --junitxml="$report"
python3 scripts/check_pytest_no_skips.py "$report" "docs tests"
```

`bashlex` is required because `test_guides.py` parses documented shell blocks
instead of only checking for string fragments.

## Requirement-aware Naming Convention

Requirement traceability is enforced by:

1. Listing test names in `docs/spec/requirements/*.yaml`
2. Including `REQ-*` IDs in test docstrings or comments

Python tests currently use descriptive names, with requirement IDs in docstrings:

```python
def test_all_requirements_have_tests() -> None:
    """REQ-API-005: Requirements must list tests and files must exist."""
    ...
```

Rust tests in `ugoite-core/tests/` follow a requirement-aware naming pattern:

```rust
async fn test_entry_req_entry_001_create_entry_basic() -> anyhow::Result<()> {
    // REQ-ENTRY-001
    ...
}
```

When adopting this pattern, replace hyphens in `REQ-XXX-001` with underscores to
form `req_xxx_001`, and keep `feature` aligned to the domain under test (e.g.
api, space, entry, form, core, cli).

Representative examples from the current suite:

```python
def test_all_requirements_have_tests() -> None:
    """REQ-API-005: Requirements must list tests and files must exist."""
    ...

def test_docs_req_entry_001_create_entry_payload_uses_content_field() -> None:
    """REQ-ENTRY-001: create-entry docs use the implemented `content` request field."""
    ...
```

## Adding New Tests

1. Extend an existing domain-specific `test_*.py` file when possible, or add a new one under this directory.
2. Keep the test runnable through `mise run test:docs`.
3. Add the test name to `docs/spec/requirements/*.yaml` whenever it verifies a `REQ-*`.
4. Use `pyyaml`, `pathlib`, and `bashlex` consistently with the existing suite.
5. Prefer concrete contract checks over placeholder "to be implemented" stubs.

### Requirement-aware Naming

For pytest tests that validate a specific requirement, use:

```python
# Pattern: test_<feature>_<requirement_id>_<description>
# Example (REQ-API-005):
def test_docs_req_api_005_all_requirements_have_tests():
    """REQ-API-005: Requirements Traceability Automation"""
    ...
```

Details:

- Replace hyphens in `REQ-XXX-001` with underscores to form `req_xxx_001`.
- `feature` should reflect the domain under test (e.g., docs, api, entry).

## Dependencies

- `pytest`
- `pyyaml`
- `bashlex`

## CI Integration

The Python CI workflow runs the docs consistency suite with the same dependency
set used locally:

```bash
uv run --with pytest --with pyyaml --with bashlex pytest docs/tests -v -W error --junitxml=docs-pytest.xml
python3 scripts/check_pytest_no_skips.py docs-pytest.xml "docs tests"
```

See `.github/workflows/python-ci.yml` and `docs/spec/testing/ci-cd.md` for the
current CI-aligned command line.
