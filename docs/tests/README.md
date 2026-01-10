# Documentation Consistency Tests

This directory contains tests that verify documentation consistency and requirements traceability.

## Purpose

These tests ensure:

1. **Requirements Coverage**: Every requirement has at least one test
2. **Test Traceability**: Tests reference valid requirements
3. **Feature Registry Consistency**: API registries match code (`docs/spec/features/features.yaml` + per-kind YAML files)
4. **Documentation Cross-References**: Links between documents are valid
5. **Story Format Validation**: User stories follow the required YAML format

## Test Files (To Be Implemented)

| File | Purpose |
|------|---------|
| `test_requirements.py` | Verify requirement → test mapping |
| `test_features.py` | Verify feature paths exist in codebase |
| `test_stories.py` | Validate story YAML format |
| `test_docs_links.py` | Check cross-references between docs |

## Running Tests

```bash
# From repository root
cd docs/tests
python -m pytest

# Or with verbose output
python -m pytest -v
```

## Test Details

### test_requirements.py

```python
def test_all_requirements_have_tests():
    """
    Load all YAML files from docs/spec/requirements/
    Verify each requirement has at least one test in its tests array
    """
    pass

def test_all_tests_reference_valid_requirements():
    """
    Scan test files for REQ-* mentions
    Verify each reference exists in requirements YAML
    """
    pass

def test_no_orphan_tests():
    """
    Find tests not listed in any requirement's tests array
    Report as warnings (not failures)
    """
    pass

def test_requirement_ids_are_unique():
    """
    Ensure no duplicate requirement IDs across all files
    """
    pass
```

### test_features.py

```python
def test_feature_paths_exist():
    """
    Load docs/spec/features/features.yaml (manifest)
    Load each referenced per-kind registry YAML
    Verify each (file, function/component) reference exists in the codebase
    """
    pass

def test_no_undeclared_feature_modules():
    """
    Find API operations in code that are not declared in the registry
    Report as warnings
    """
    pass
```

### test_stories.py

```python
def test_story_format_valid():
    """
    Validate YAML structure of all story files
    Required fields: id, title, as_a, i_want, so_that, acceptance_criteria
    """
    pass

def test_story_requirements_exist():
    """
    Verify all REQ-* references in stories exist
    """
    pass
```

### test_docs_links.py

```python
def test_markdown_links_valid():
    """
    Parse all .md files for relative links
    Verify targets exist
    """
    pass

def test_yaml_spec_references_valid():
    """
    Parse related_spec fields in YAML files
    Verify referenced files and sections exist
    """
    pass
```

## Adding New Tests

1. Create test file following `test_*.py` naming
2. Use `pytest` conventions
3. Load YAML files using `pyyaml`
4. Use `pathlib` for cross-platform path handling
5. Document test purpose in docstrings

## Dependencies

```txt
# requirements.txt for docs/tests
pytest>=7.0
pyyaml>=6.0
```

## CI Integration

These tests should run as part of CI:

```yaml
# .github/workflows/docs-ci.yml
jobs:
  docs-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-python@v5
        with:
          python-version: '3.12'
      - run: pip install pytest pyyaml
      - run: cd docs/tests && pytest -v
```

## Coverage Report

Generate a coverage report with:

```bash
python generate_coverage_report.py
```

This produces:
- Total requirements by category
- Implemented vs planned requirements  
- Test coverage percentage
- Orphan tests list
- Missing documentation cross-references
