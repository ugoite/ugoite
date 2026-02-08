# Requirements Documentation

This directory contains machine-readable requirement definitions for IEapp.

## Files

| File | Category | Prefix |
|------|----------|--------|
| [storage.yaml](storage.yaml) | Storage & Data Model | REQ-STO-* |
| [entry.yaml](entry.yaml) | Entry Management | REQ-ENTRY-* |
| [index.yaml](index.yaml) | Indexer | REQ-IDX-* |
| [integrity.yaml](integrity.yaml) | Data Integrity | REQ-INT-* |
| [security.yaml](security.yaml) | Security | REQ-SEC-* |
| [api.yaml](api.yaml) | REST API | REQ-API-* |
| [frontend.yaml](frontend.yaml) | Frontend UI | REQ-FE-* |
| [e2e.yaml](e2e.yaml) | End-to-End | REQ-E2E-* |
| [ops.yaml](ops.yaml) | Operations & Dev Environment | REQ-OPS-* |
| [form.yaml](form.yaml) | Form Management | REQ-FORM-* |
| [links.yaml](links.yaml) | Link Management | REQ-LNK-* |
| [search.yaml](search.yaml) | Search | REQ-SRCH-* |

## Requirement Format

Each YAML file follows this structure:

```yaml
category: Category Name
prefix: REQ-XXX

requirements:
  - id: REQ-XXX-001
    title: Short descriptive title
    description: |
      Full description of what the system MUST/SHOULD do.
    related_spec:
      - architecture/overview.md#section
      - stories/core.yaml#STORY-001
    priority: high | medium | low
    status: implemented | planned | deprecated
    tests:
      pytest:
        - file: path/to/test.py
          tests:
            - test_function_name
      vitest:
        - file: path/to/test.ts
          tests:
            - test name in describe block
      e2e:
        - file: e2e/test.ts
          tests:
            - test name
```

## Requirement ID Convention

- `REQ-STO-###` - Storage & data model requirements
- `REQ-ENTRY-###` - Entry management requirements
- `REQ-IDX-###` - Indexer requirements
- `REQ-INT-###` - Integrity requirements
- `REQ-SEC-###` - Security requirements
- `REQ-API-###` - API requirements
- `REQ-FE-###` - Frontend requirements
- `REQ-E2E-###` - End-to-end requirements
- `REQ-FORM-###` - Form management requirements
- `REQ-LNK-###` - Link requirements
- `REQ-SRCH-###` - Search requirements

## Test Mapping

Each requirement lists the tests that verify it:

- **pytest**: Python tests in `ieapp-cli/tests/` and `backend/tests/`
- **vitest**: TypeScript tests in `frontend/src/**/*.test.ts(x)`
- **e2e**: End-to-end tests in `e2e/`

## Verification Tests

Automated tests in `docs/tests/` verify requirements consistency:

### `test_requirements.py`

```python
def test_all_requirements_have_tests():
    """Every requirement must have at least one associated test."""
    # Loads all YAML files and checks tests array is non-empty

def test_all_tests_reference_valid_requirements():
    """Tests that claim to verify requirements must reference valid REQ-* IDs."""
    # Scans test files for REQ-* mentions and validates against YAML

def test_no_orphan_tests():
    """Flag tests that don't map to any requirement."""
    # Reports tests not listed in any requirement's tests array

def test_requirement_ids_are_unique():
    """Ensure no duplicate requirement IDs."""

def test_required_fields_present():
    """All requirements have id, title, description, tests."""
```

## Adding New Requirements

1. Determine the appropriate category (storage, entry, etc.)
2. Find the next available ID number in that file
3. Add the requirement with all required fields
4. Add tests that verify the requirement
5. Update the test files to reference the requirement ID
6. Run `docs/tests/test_requirements.py` to verify

## Coverage Report

Generate a coverage report with:

```bash
python docs/tests/generate_coverage_report.py
```

This produces:
- Total requirements by category
- Implemented vs planned requirements
- Test coverage percentage
- Orphan tests list

## Migration from 07_requirements.md

This YAML-based system replaces the previous `07_requirements.md` file (now removed after migration):

| Old | New |
|-----|-----|
| Markdown tables | YAML structures |
| Manual test lists | Parseable test references |
| Single file | Organized by category |
| Human-readable only | Machine + human readable |

The conversion was done during Milestone 2 (Full Configuration).
