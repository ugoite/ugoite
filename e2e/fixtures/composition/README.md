# Composition acceptance fixtures

This directory reserves the E2E Space-seed layout and surface acceptance shape
for Composition. The canonical `monthly-expense.ugcomp.yaml` document belongs
to the shared domain fixture set under
`crates/ugoite-domain/tests/fixtures/composition/`; E2E must consume that
fixture rather than maintain a second copy. Space seed and expected-result
payloads wait for the typed document contract and parser limits to be reviewed.

`acceptance-plan.json` is a plan, not test evidence. Its selectors are reserved
locators for the B0–B6 work and do not claim that those tests or product
surfaces exist. The only current executable check verifies that the plan stays
complete and explicitly pending.

The shared and E2E-specific document fixtures are:

- `monthly-expense.ugcomp.yaml`: canonical dashboard and parameter fixture
  shared with domain and WASM tests.
- `unknown-format-version.ugcomp.yaml`: E2E raw-inspection, export, and
  history-recovery fixture for a version the current runtime cannot execute.
- `broken-source-reference.ugcomp.yaml`: E2E raw-inspection and export fixture
  for a Composition whose source reference no longer resolves.
- `space-seed/manifest.json`: E2E seed identity and composition membership,
  without duplicating the YAML payload.
- `expected/`: frozen canonical bytes and expected diagnostic codes after the
  domain format is agreed.

The Browser journey runs with model connection disabled. It saves one
Space-owned Composition, closes and reopens the Browser, changes a parameter,
and advances a paged result. It checks transient Work state and stale-response
suppression without introducing a TypeScript parser or client-side query
engine. The CLI acceptance shape covers `list`, `inspect`, `lint`, `save`,
`query`, `export`, and `import`; exact serialized output remains pending the
shared DTO contract.
