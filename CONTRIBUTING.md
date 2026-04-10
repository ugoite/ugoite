# Contributor Workflow

Use this guide when you are changing code, docs, specs, or tests in Ugoite. It
connects the repository workflow, REQ-* traceability, and CI expectations in one
place so a local green change stays aligned with the shipped product docs.

## 1. Start from the canonical setup path

Use the repository-managed toolchain first:

```bash
mise run setup
```

That path installs the shared dependencies and runs `uvx pre-commit install`, so
local commits use the same pre-commit gate that CI enforces.

Common follow-up commands:

```bash
mise run dev
mise run test
```

Before adding a new workflow-specific command, check `.github/workflows/` and
prefer the exact local command shape CI already uses.

## 2. Decide which source of truth must change

When behavior changes, update the canonical layer first:

- Product or UX behavior: `docs/spec/features/`
- Requirement or traceability contract: `docs/spec/requirements/`
- User/operator instructions: `docs/guide/` and any matching README/docsite copy
- App/documentation navigation or landing surfaces: `docsite/src/`

Keep the architecture boundary explicit:

- `ugoite-core` owns business logic and filesystem I/O
- `backend` is the HTTP/MCP adapter layer
- `ugoite-cli` is the terminal interface

If a change adds or moves behavior between those layers, update the specs and
docs in the same branch instead of leaving the contract implied.

## 3. Keep REQ-* traceability complete

Every new or updated test must point to a requirement in
`docs/spec/requirements/*.yaml`.

- Add the matching `REQ-*` identifier to the test name or docstring
- Update the requirement entry so it lists the concrete test file and test name
- Do not add a new requirement without adding the tests that enforce it

The docs consistency suite in `docs/tests/` treats those mappings as part of the
contract, not optional commentary.

## 4. Keep guides and docsite surfaces in sync

If a user-visible flow changes, update both the prose guide and the docsite
surface that introduces it.

Typical follow-up files include:

- `docsite/src/lib/navigation.ts`
- `docsite/src/lib/navigation.test.ts`
- any relevant `/app/*` or `/getting-started` page
- `README.md` when the newcomer or contributor entry path changes

Do not leave a new guide, page, or application surface discoverable only from a
deep spec page.

## 5. Match local validation to CI

Run the checks that correspond to the surfaces you touched, and make sure they
match the current workflows under `.github/workflows/`.

Examples:

- `mise run test` for the repository-wide baseline
- `mise run test:docs` before pushing docs, spec, or REQ-traceability changes
- targeted `uv run pytest ...` for docs/backend/core changes when iterating
- `cd docsite && bun run test:coverage` for docsite regressions
- `cd frontend && biome ci . && node ./node_modules/vitest/vitest.mjs run --coverage --maxWorkers=1`
  when UI behavior changes

Use `mise run test:docs` when `README.md`, `CONTRIBUTING.md`, `docs/spec/`,
or `docs/tests/` change so the docs consistency suite catches guide drift and
REQ mapping regressions locally first.

If you add a new docsite page or navigation path, include the matching vitest or
docs regression so the route and copy stay wired.

## 6. Prepare the PR as one coherent change

Before opening the PR:

1. Re-read the relevant `docs/spec/` files.
2. Make sure the implementation, docs, and REQ-* test mappings all agree.
3. Keep the diff focused on one issue/feature/fix.

When in doubt, prefer the smallest change that keeps the specification,
implementation, and contributor-facing docs in sync.
