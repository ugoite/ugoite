# Contributor Workflow

Use this guide when you are changing code, docs, specs, or tests in Ugoite. It
connects the repository workflow, REQ-* traceability, and CI expectations in one
place so a local green change stays aligned with the shipped product docs.

Root `mise.toml` is the only managed-tool entry point and pins only Rust and
Deno. Use `mise run setup`, then `mise run ci` for pull requests,
`mise run ci:merge` for the merge/main gate, and `mise run ci:release` for
release artifacts. Required checks converge on `ci-required` and
`codeql-required`. See
[`docs/architecture/release-rearchitecture.md`](docs/architecture/release-rearchitecture.md)
for the target architecture and migration boundary.

## 1. Choose a supported contributor setup path

Ugoite supports two contributor setup paths that converge on the same local
commands, hooks, and CI-parity checks:

| Path | Choose it when | What it does for you |
| --- | --- | --- |
| Managed host toolchain | You are happy installing the repo toolchain on your machine or you are not using VS Code/Codespaces | Run `mise run setup` yourself to install the shared dependencies, the fast default hook chain, and the heavier pre-push coverage hook, so local commits stay quick by default. |
| Devcontainer / GitHub Codespaces | You want a reproducible VS Code/Codespaces workspace or do not want to install the full toolchain on your host | `.devcontainer/devcontainer.json` preinstalls `mise`, `gh`, `oathtool`, then runs `mise install`, `mise run setup`, and `deno task e2e:install:browsers` for you. |

If you are on the managed host toolchain path, start with:

```bash
mise run setup
```

That path installs the shared dependencies and prepares the browser test
runtime. The devcontainer runs that same bootstrap for you during container
creation.

Common follow-up commands inside either setup:

```bash
mise run dev
mise run test
mise run ci
```

Before adding a new workflow-specific command, check `.github/workflows/` and
prefer the exact local command shape CI already uses.

### Portable Rust / embedding path

Use `ugoite-domain` when the change must stay pure, portable, and small enough
for WASM-oriented or embedding consumers instead of depending on storage
adapters, indexing engines, Python bindings, or HTTP routes.

- Start with `ugoite-domain/README.md` for the responsibility boundary and
  `docs/spec/architecture/future-proofing.md` for the portability goals.
- Run `mise run //ugoite-domain:test` for the package-local Rust quality gates.
- Run `mise run //ugoite-domain:build:wasm` when the change touches portable
  APIs or other browser/embedding-facing surfaces.

If the change needs OpenDAL-backed workflows, backend endpoints, or CLI UX, keep
it in `ugoite-core`, `backend`, or `ugoite-cli` instead of pushing adapter logic
down into `ugoite-domain`.

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

  when UI behavior changes

Use `mise run test:docs` when `README.md`, `CONTRIBUTING.md`, `docs/spec/`,
or `docs/tests/` change so the docs consistency suite catches guide drift and
REQ mapping regressions locally first.

If you add a new docsite page or navigation path, include the matching vitest or
docs regression so the route and copy stay wired.

## 6. Match CI events to the review stage

Ugoite splits CI by event so the expensive gates only run when they add real
value:

| Event | What it covers |
| --- | --- |
| `pull_request` | `ci-required`, which runs `mise run ci` |
| `merge_group` | `ci-required`, which runs `mise run ci:merge` |
| `push` on `main` | `ci-required`, which runs `mise run ci:merge` |

push on `main` is reserved for fast, low-noise checks and post-merge automation.
`pull_request` keeps the normal developer feedback set.
`merge_group` is the final gate for expensive validation.

Keep `.github/workflows/ci.yml` and `mise.toml` as the executable source of
truth, and keep `docs/spec/testing/ci-cd.md` as the human-readable policy when
updating the gate shape.

When CodeQL or other branch-protection policy changes depend on GitHub ruleset
state, update the live repository ruleset as well as the checked-in JSON. PR
1559 switched `main only pr` to require the `CodeQL` summary check directly so the branch gate no longer depends on the separate code-quality rule, and merge queue plus `main` merge were re-verified after that repository-side update.

Local validation maps to the same CI event split: use `mise run ci` for pull
request parity, `mise run ci:merge` before merging or queueing, `mise run
ci:release` for release artifacts, and focused surface commands when only one
package changed.

## 7. Prepare the PR as one coherent change

Before opening the PR:

1. Re-read the relevant `docs/spec/` files.
2. Make sure the implementation, docs, and REQ-* test mappings all agree.
3. Keep the diff focused on one issue/feature/fix.

When in doubt, prefer the smallest change that keeps the specification,
implementation, and contributor-facing docs in sync.
