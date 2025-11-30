# ieapp Development Guidelines

## Documentation
For project structure, architecture, and detailed specifications, please refer to:
- `README.md`
- `docs/` directory

## Code Quality & CI Verification

**ALWAYS** run the following mise commands before finishing a task:
```bash
mise run format
mise run lint
mise run test
```

These commands will format, lint, and test both frontend and backend code.

- **Python**: `ruff` (format & lint), `ty` (type check)
- **Frontend**: `biome`

## WEB SEARCH RECOMMENDATION
You should search on web for best practices and guidelines in nowadays (2025) anytime you start a new feature or technology or research tasks.
