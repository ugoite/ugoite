# Composition acceptance fixtures

The canonical `monthly-expense.ugcomp.yaml` belongs to the shared domain fixture
set under `crates/ugoite-domain/tests/fixtures/composition/`. The Browser E2E
uses the Browser save flow so its Composition is produced by the same public
surface as a user-created tool.

`acceptance-plan.json` describes the full B0–B7 plan and the reactive-variable
Studio journey. Planned selectors are locators, not release evidence. Scoped
Browser journey results and their JUnit artifacts are recorded under
`evidence/`; other selectors and release criteria remain unverified. The
Browser journey sources are
`e2e/composition-golden-journey.test.ts` and
`e2e/composition-studio-journey.test.ts`.

The Form and Saved SQL source ACL revocation selectors, plus the Space-isolation
selector in B7, are covered by
`e2e/composition-source-acl-isolation.test.ts`.

- `unknown-format-version.ugcomp.yaml` is a raw-payload candidate for
  unsupported-format recovery.
- `broken-source-reference.ugcomp.yaml` is a raw-payload candidate whose Form ID
  is absent from the Browser seed Space.
- `space-seed/manifest.json` records the seeded dataset and expected pages. Each
  run creates a unique server-backed Space, a parameterized Saved SQL query, and
  101 matching rows across two result pages, plus one excluded row.

The Browser test opens the seeded query, saves it with “Save as tool”, declares
typed date Variables, binds Saved SQL variable names explicitly, and places
ordinary Inputs with user-facing labels before saving. It then forces a delayed
response loss after the server commits. Retrying must reuse the same
`Idempotency-Key`, return the same receipt and exact revision, and leave one
history publication. A fresh browser context rediscovers that exact revision
from Home, sees the declared defaults in the labeled Inputs, and reads page one
and page two. Changing the Start date Input resets the result page and narrows
the rows. The journey does not connect a model or persist Browser Work.

The Studio journey declares a string Variable with a default, binds an Input
and Text to it, and binds date Inputs to the exact Saved SQL revision. It reads
the exact saved Composition revision to verify those bindings. A runtime edit
updates Text immediately without another SQL request. Reloading that revision
restores the declared default without publishing a second Composition
revision.

Run the delayed stale-response case separately with
`bash e2e/scripts/run-e2e-parity.sh composition-stale`. It uses the same
user-facing date labels and holds an earlier Saved SQL response while the Inputs
move to the current date window.

The CLI Core/Remote parity selector is implemented as a Rust CLI-process
integration test in `crates/ugoite-cli/tests/test_journey_remote.rs`; it uses
one shared filesystem-backed Space to compare exact-revision inspect,
parameterized `entry_query`, and raw export. Its fixture contains one
Composition revision, so older-than-latest selector behavior is tracked in issue
#3681. Its scope does not cover saved-SQL query execution or unknown/broken raw
recovery. The remaining recovery, complete cross-surface, and exact-candidate
release evidence selectors remain planned. The E2E seed does not implement a
second parser, resolver, or query engine.
