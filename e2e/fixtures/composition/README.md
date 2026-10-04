# Composition acceptance fixtures

The canonical `monthly-expense.ugcomp.yaml` belongs to the shared domain fixture
set under `crates/ugoite-domain/tests/fixtures/composition/`. The Browser E2E
uses the Browser save flow so its Composition is produced by the same public
surface as a user-created tool.

`acceptance-plan.json` describes the full B0–B6 plan. Its planned selectors are
locators, not release evidence. One scoped Browser journey result and its JUnit
artifact are recorded under `evidence/`; the other selectors and release
criteria remain unverified. The Browser journey source is in
`e2e/composition-golden-journey.test.ts`.

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

The CLI Core/Remote parity selector is implemented as a Rust CLI-process
integration test in `crates/ugoite-cli/tests/test_journey_remote.rs`; it uses
one shared filesystem-backed Space to compare exact-revision inspect,
parameterized `entry_query`, and raw export. Its fixture contains one
Composition revision, so older-than-latest selector behavior is tracked in
issue #3681. Its scope does not cover saved-SQL query execution or
unknown/broken raw recovery. The remaining recovery, complete cross-surface,
and exact-candidate release evidence selectors remain planned. The E2E seed
does not implement a second parser, resolver, or query engine.
