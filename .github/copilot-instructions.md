# Code Review Guidelines for Copilot

## Requirements & Test Traceability
- **Every new test must reference a requirement** in `docs/spec/requirements/*.yaml` (REQ-*). Link the test name/docstring to the related REQ-* identifier.
- **Every new requirement must have tests**. Do not add requirements without corresponding test coverage.

## Linting & Code Quality
- **Do not use `# noqa` comments**. All linter exemptions must be configured in `pyproject.toml` under `[tool.ruff.lint] ignore` only.
- **No future-proofing aliases**. When modifying existing functions, maintain backward compatibility only if actively required. Do not create unnecessary wrapper functions.

## Architecture: Backend & CLI Separation
**Strict responsibility boundary:**
- **ieapp-cli**: Handles all filesystem I/O via `fsspec`. User requests are materialized to disk here only.
- **backend**: Pure API layer that calls ieapp-cli. No filesystem operations, no business logic duplication.
- **Violation check**: If the same logic exists in both modules or backend writes directly to fs, flag as architectural violation.

## General Best Practices
- **Test coverage**: Aim for >80% code coverage (excepting integration stubs).
- **Type safety**: Complete type hints in Python; full types in TypeScript.
- **CI verification**: All changes must pass local test suite (`mise run test`) and match `.github/workflows/` requirements exactly.
- **PR clarity**: Keep diffs focused; batch related changes only.