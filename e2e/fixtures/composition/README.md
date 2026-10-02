# Composition acceptance fixtures

This directory reserves the E2E Space-seed layout and surface acceptance shape
for Composition. The canonical `monthly-expense.ugcomp.yaml` document belongs
to the shared domain fixture set under
`crates/ugoite-domain/tests/fixtures/composition/`; E2E must consume that
fixture rather than maintain a second copy. The raw recovery candidates below
use the merged typed shape; Space seed and expected-result payloads still wait
for parser limits and integration contracts to be reviewed.

`acceptance-plan.json` is a plan, not test evidence. Its selectors are reserved
locators for the B0–B6 work and do not claim that those tests or product
surfaces exist. The only current executable check verifies that the plan stays
complete and explicitly pending.

The shared and E2E-specific document fixtures are:

- `monthly-expense.ugcomp.yaml`: canonical dashboard and parameter fixture
  shared with domain and WASM tests.
- `unknown-format-version.ugcomp.yaml`: raw-payload candidate for unsupported
  format recovery. It is not a parser or E2E result fixture yet.
- `broken-source-reference.ugcomp.yaml`: raw-payload candidate with a Form ID
  absent from a future Space seed. It is not a resolver or E2E result fixture
  yet.
- `space-seed/manifest.json`: E2E seed identity and composition membership,
  without duplicating the YAML payload.
- `expected/`: frozen canonical bytes and expected diagnostic codes after the
  domain format is agreed.

The raw candidates use the merged typed document shape, but their presence
does not freeze restricted parser rules, establish a Space seed contract, or
record runtime evidence. The seed manifest and expected outputs remain
planned.

The Browser journey runs with model connection disabled. It saves one
Space-owned Composition, closes and reopens the Browser, changes a parameter,
and advances a paged result. It checks transient Work state and stale-response
suppression without introducing a TypeScript parser or client-side query
engine. The CLI acceptance shape covers `list`, `inspect`, `lint`, `save`,
`query`, `export`, and `import`; exact serialized output remains pending the
shared DTO contract.
