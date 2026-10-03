# Composition acceptance fixtures

The canonical `monthly-expense.ugcomp.yaml` belongs to the shared domain fixture
set under `crates/ugoite-domain/tests/fixtures/composition/`. The Browser E2E
uses the Browser save flow so its Composition is produced by the same public
surface as a user-created tool.

`acceptance-plan.json` describes the full B0–B6 plan. Its planned selectors are
locators, not release evidence. The Browser journey source is in
`e2e/composition-golden-journey.test.ts`; runtime evidence is recorded only
after the focused E2E command runs against the merged routes.

- `unknown-format-version.ugcomp.yaml` is a raw-payload candidate for
  unsupported-format recovery.
- `broken-source-reference.ugcomp.yaml` is a raw-payload candidate whose Form ID
  is absent from the Browser seed Space.
- `space-seed/manifest.json` records the seeded dataset and expected pages. Each
  run creates a unique server-backed Space, a parameterized Saved SQL query, and
  101 matching rows across two result pages, plus one excluded row.

The Browser test opens the seeded query, saves it with “Save as tool”, and
forces a delayed response loss after the server commits. Retrying must reuse the
same `Idempotency-Key`, return the same receipt and exact revision, and leave
one history publication. A fresh browser context rediscovers that exact revision
from Home, applies the Composition's declared parameter defaults, and reads page
one and page two. Changing the date parameter resets the result page and narrows
the rows. The journey does not connect a model or persist Browser Work.

The raw recovery candidates and the remaining CLI, recovery, and exact-candidate
release evidence selectors remain planned. The E2E seed does not implement a
second parser, resolver, or query engine.
