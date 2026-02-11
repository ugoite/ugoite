# Code Review Guidelines for Copilot

## Requirements & Test Traceability
- **Every new test must reference a requirement** in `docs/spec/requirements/*.yaml` (REQ-*). Link the test name/docstring to the related REQ-* identifier.
- **Every new requirement must have tests**. Do not add requirements without corresponding test coverage.

## Linting & Code Quality
- **Do not use `# noqa` comments**. All linter exemptions must be configured in `pyproject.toml` under `[tool.ruff.lint] ignore` only.
- **No future-proofing aliases**. When modifying existing functions, maintain backward compatibility only if actively required. Do not create unnecessary wrapper functions.

## Architecture: Backend & Core Separation
**Strict responsibility boundary:**
- **ugoite-core**: Handles all logic and filesystem I/O.
- **backend**: Pure API layer that calls ugoite-core. No business logic duplication.
- **ugoite-cli**: CLI tool for direct user interaction, also calling ugoite-core.
- **Violation check**: If business logic exists in the backend or ugoite-cli instead of the core, flag as architectural violation.

## General Best Practices
- **Test coverage**: Aim for >80% code coverage (excepting integration stubs).
- **Type safety**: Complete type hints in Python; full types in TypeScript.
- **CI verification**: All changes must pass local test suite (`mise run test`) and match `.github/workflows/` requirements exactly.
- **PR clarity**: Keep diffs focused; batch related changes only.