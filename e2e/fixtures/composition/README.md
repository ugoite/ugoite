# Composition acceptance fixtures

The canonical `monthly-expense.ugcomp.yaml` belongs to the shared domain fixture
set under `crates/ugoite-domain/tests/fixtures/composition/`. Browser E2E reads
that file directly and does not maintain another YAML copy.

`acceptance-plan.json` describes the full B0–B6 plan. Its planned selectors are
locators, not release evidence. The Browser journey source is in
`e2e/composition-golden-journey.test.ts`; runtime evidence is recorded only
after the focused E2E command runs against the merged routes.

- `unknown-format-version.ugcomp.yaml` is a raw-payload candidate for
  unsupported-format recovery.
- `broken-source-reference.ugcomp.yaml` is a raw-payload candidate whose Form ID
  is absent from the Browser seed Space.
- `space-seed/manifest.json` records the seeded dataset and expected pages. Each
  run creates a unique server-backed Space and substitutes the server's Form,
  field, Saved SQL, and revision identities into the shared fixture.

The Browser test publishes the seed Composition through the existing API and
checks that its receipt names the saved revision. A fresh browser context opens
it from Home, supplies the date parameters, and reads a two-row table page then
a one-row continuation page. It also checks the scalar metric comes from the
Saved SQL result and remains unchanged while the table pages. A second browser
context reopens the same exact revision from Home; parameter state starts empty
again. The journey does not call a model connection or persist Browser Work.

The raw recovery candidates and the remaining CLI, recovery, and exact-candidate
release evidence selectors remain planned. The E2E seed does not implement a
second parser, resolver, or query engine.
