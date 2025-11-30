# ieapp Development Guidelines

## Documentation
For project structure, architecture, and detailed specifications, please refer to:
- `README.md`
- `docs/` directory

## Code Quality & CI Verification

**Code Formatting, Linting, and Type Checking are MANDATORY.**

You must ensure that your changes pass all CI checks.
**ALWAYS** check `.github/workflows/` and run the corresponding commands locally to verify your code before finishing a task.

### Key Checks (See Workflows for exact commands)
- **Python**: `ruff` (format & lint), `ty` (type check)
- **Frontend**: `biome`

**Ensure the GitHub Actions commands run successfully on your local machine.**

<!-- MANUAL ADDITIONS START -->
## WEB SEARCH RECOMMENDATION
You should search on web for best practices and guidelines in nowadays (2025) anytime you start a new feature or technology or research tasks.
<!-- MANUAL ADDITIONS END -->
