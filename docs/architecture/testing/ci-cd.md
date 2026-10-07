---
title: "CI and release gates"
---

The required build/test gate is `.github/workflows/ci.yml`. Separate workflows
run CodeQL, validate required pull-request body sections/issue links, promote
docsite Pages, and publish versioned non-docsite release artifacts.

| Event                               | Hosted CI behavior                                                                                                     |
| ----------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| pull request to `main`              | change-selected validation; uncertain or cross-cutting changes run every lane                                           |
| merge queue to `main`               | every validation lane against the exact queued source SHA                                                               |
| push to `main`                      | merge gate plus shared-cache refresh and verified artifact upload                                                      |
| manual `Release Candidate` dispatch | builds and verifies the exact selected source SHA and stores a candidate bundle                                        |
| manual `Release Publish` dispatch   | verifies an operator-selected candidate bundle and promotes its exact artifacts without rebuilding                     |

Root task composition:

- `build:*`: deterministic compile/build steps with declared inputs and outputs;
- `test:*`: authoritative assertions that may reuse `build:*` outputs but always
  execute when called; focused frontend/docsite tasks remain useful during
  development;
- `test`: the canonical non-E2E suite, including Rust and tooling tests, the
  frontend coverage gate, and the normal docsite behavior/type suite;
- `test:rust`: the canonical Rust test interface; cargo-nextest runs unit,
  integration, binary, and library tests, followed by
  `cargo test --workspace --doc --locked` for doctests;
- `test:smoke`: the focused Rust smoke interface uses the same
  nextest-plus-doctest split before the frontend smoke task;
- `test:frontend:coverage`: the frontend V8 coverage assertion with its
  package-owned hard threshold for the portable Rust/WASM protocol boundary;
- `test:docsite`: the normal docsite behavior/type test suite;
- `test:docsite:coverage`: an explicit developer-convenience V8 report for
  authored docsite source, without a hard threshold or merge-gate role;
- `package:*`: staging under `target/artifacts/` only; packaging must fail if
  required build outputs are absent;
- `verify:*`: checks packaged outputs without rebuilding them;
- `ci`: formatting check, lint, architecture/OpenAPI/type checks, and `test`;
- `ci:artifacts`: build/package/verify, a focused docsite-navigation E2E lane,
  E2E smoke plus Form-owned Asset acceptance, the mobile browser visual
  regression suite, owner recovery, portable-Space acceptance, and version
  validation;
- `ci:artifacts:prepare`: build, package, verify, and version-check the current
  source without running E2E;
- `ci:artifacts:load`: verify the same-run manifest, source SHA, CI run ID, and
  selected CLI or image digests, then selectively load or extract those files;
- `ci:lane:e2e-smoke-mobile`, `ci:lane:e2e-owner`, and
  `ci:lane:e2e-portable`: separate hosted E2E groups that consume the prepared
  build artifact;
- `ci:merge`: `ci` plus `ci:artifacts`;
- `ci:cp1:fixtures`: hosted fixtures entry that builds the fixture seeder
  once, prepares one run-scoped bundle per fixture set, and stages the seeder
  for consumer download; query and export bundles upload as separate
  artifacts with no cross-run cache;
- `ci:lane:cp1-query`: hosted query consumer that restores only the query
  bundle to a unique temp root and runs the fixed 1,200 + 800 Entry,
  two-Space browser query-lifecycle assertions against the verified release
  image through the Compose runner; its Playwright report fails when any test
  is skipped, and it never builds the server, WASM, or frontend;
- `ci:lane:cp1-export`: hosted export consumer that restores only the export
  bundle and runs the separate bounded 1,000-row SQL export acceptance check
  with the verified release CLI; it never rebuilds the CLI;
- `ci:lane:cp1-acceptance`: the local integrated entry that runs the same
  shared fixture preparation, query, and export scripts in one process;
  hosted CI runs the fixtures/query/export trio instead, gated as a unit by
  the single cp1 plan flag;
- `ci:cp1:prepare-fixtures`: build the fixture verifier once, seed the two
  query Spaces and separate export Space, verify persisted contents, integrity,
  Forms, and owner state through the canonical service, then write
  source- and run-bound archives for the acceptance consumers;
- `ci:impact`: a standalone, conservative diff planner used by hosted CI to
  select pull-request lanes while preserving full main-push and merge-group
  validation;
- `ci:lane:docsite-nav`: the standalone navigation/link E2E lane used by hosted
  CI for early docsite feedback;
- `ci:release`: release artifact build/package/verification plus npm
  packaging/verification; it does not rerun `ci:merge` or full E2E.

Hosted CI uses the `ci:impact` diff planner to select pull-request lanes and
schedules selected lanes in parallel. Docs-only pull requests run Web and
docsite navigation; frontend-only pull requests run Web and artifact/E2E
verification; Rust changes also run both Rust lanes. Main pushes and merge
groups always run every lane. Large diffs, global or unclassified paths,
missing SHAs, unsupported events, and diff failures fall back to the full lane
plan. The artifact build runs once and publishes runtime image, CLI, and
manifest artifacts for pull requests, merge groups, and main pushes. Smoke /
mobile and owner E2E download only the manifest and runtime image; portable E2E
downloads the manifest, image, and CLI. The shared loader verifies the source
SHA, run ID, selected digests and sizes, and safe paths before loading only the
requested inputs. The combined E2E artifact is no longer uploaded. Each job
reports download bytes and duration, manifest verification, CLI extraction
when selected, and Docker image load time so transfer overhead can be measured
during the pilot. `ci-required` runs on `ubuntu-slim`, checks every
planned lane against its result, and fails closed on missing results,
unexpected skips, and unexpected executions. The three quality lanes run only
`mise run ci:lane:rust-check`, `mise run ci:lane:rust-test`, and
`mise run ci:lane:web`; they do not duplicate repository validation commands in
GitHub Actions. The artifact producer runs `mise run ci:artifacts:prepare`;
the three E2E groups use their corresponding Mise lane task and keep the
Playwright worker count at one. The standalone
docsite-navigation lane runs through `mise run ci:lane:docsite-nav` so broken
links and navigation can fail before the artifact build completes.

The split-E2E pilot decision (#3307) is to keep the three independent E2E
consumer jobs. Fourteen successful merge-group runs on 2026-09-28 showed
workflow-to-gate times of 28–46 minutes with the consumers starting within
about two minutes of artifact availability and finishing in 2–8 minutes, while
the then-single `ci-cp1-acceptance` lane held the critical path at 27–42
minutes; after the CP1 fixtures/query/export trio landed, merge-group run
37278654095 gated in under 19 minutes with the parallel consumers finishing
within about eight minutes of artifact availability. Consolidating the suites
into one job would not move `ci-required` p95 by the one-minute pilot
threshold — the gate was CP1-bound, not E2E-bound — while it would serialize
three browser suites onto one runner and lose per-lane failure isolation. Keep
the independent jobs; revisit only with new same-topology comparison data.
Rust test sharding stays out of scope until runtime, compile/link, cache, and
shard-comparison data show a clear improvement.

Pull requests also run a separate `ci-pr-context-report` job. It checks out
the exact PR base and head commits, writes Mitase PR-context JSON and Markdown
reports, and uploads both as an artifact. This report job is an additional
PR-only lane; `ci-required` validates its success (or skip for non-PR events).
The report keeps Binding-derived direct impact and upstream specifications
separate from the path-triggered `review.always` context. The latter covers
frontend implementation/configuration and `docs/spec/ui/**` with POL-006 and
PHIL-INTERACTION-001, and bounded Knowledge authority/compatibility surfaces
with PHIL-000, POL-000, POL-016, and POL-017. These are reviewer prompts; they
do not mechanically decide compatibility or replace the PR's Knowledge
Compatibility Review and its Evidence/Decision.

The required Rust suite covers the memory and filesystem implementations. The
`mise run test:s3-storage` task also runs as a required CI lane for Rust test
changes against a pinned Silo S3 endpoint (the community-maintained MinIO fork).
Locally, it runs against an
explicitly configured S3-compatible deployment backend and does not start an
emulator. Set `UGOITE_S3_TEST_ENDPOINT` and
`UGOITE_S3_TEST_BUCKET`, with credentials supplied through the standard AWS
environment variables, and use a dedicated test bucket because recovery
fixtures leave their uniquely scoped objects in place. The task checks the
publication probe, exact read,
create-if-absent, stale-write rejection, and one-winner concurrent Head CAS,
then exercises a Space through the server-side `UgoiteService` and reconstructs
its storage service to verify Entry, Form, Asset, and reverted Change history
recovery. This proves the tested backend configuration only; it does not
certify every S3-compatible provider. Runtime startup continues to verify the
backend selected by the deployment before shared publication is admitted, and
an unverified remote store remains read-only.

Rust-compiling lanes restore the Rust registry/git dependency cache without
caching `target/`; `ci-rust-check` is the sole Cargo dependency archive writer,
while `ci-rust-test`, `ci-web`, and `artifact-build` are restore-only. `ci-web`
and `artifact-build` may restore the Deno cache, but only `ci-web` writes it.
sccache owns compiler artifact reuse in all Rust-compiling lanes: it is
read-only for pull requests and merge queues and writes only on successful
`main` pushes. Playwright browser and BuildKit caches remain separately keyed
and are refreshed only after successful pushes to `main`. Successful `main` runs
upload the verified artifact set using the logical names
`ugoite-runtime-image`, `ugoite-cli-linux`, `ugoite-helm-chart`, and
`ugoite-artifact-manifest`. The docsite is built and verified on `main` but is
not uploaded for promotion; Pages deploys stable docs only from published
release tags (see below).

Every browser lane pins `ubuntu-24.04` and the Playwright browser/runtime
versions in `e2e/deno.json` plus `deno.lock`, and times its OS-dependency
install step (`browser-deps`) into the lane summary so future setup regressions
are diagnosable (#3438). The 26m42s OS-package install seen in one owner run
was an archive-mirror outlier against typical sub-minute installs (33–44 s
across the three E2E jobs in merge-group run 37278654095); OS dependencies
are not skipped because ephemeral runners need them, and a prebuilt runner
image remains a follow-up only if timed setup data shows the install
dominating the gate again.

The `ci:impact` planner selects CP1 acceptance for changes to EntryQuery, SQL
query/count, or SQL export implementation and acceptance paths. Merge groups
and main pushes run it unconditionally. It uses a fixed-seed 1,200/800
two-Space browser fixture for stale-result, count, cancellation, retry, and
pagination assertions, then checks bounded 1,000-row export completion. Its
100,000-row/RSS/p95 performance measurements stay outside the required lane.
The required CP1 lane uses only these minimal structural fixtures. Larger
fixed-count volumes (the retired 10,000-row/two-Space lane and any larger
probes) belong to scheduled or profile-only measurement; they are never merge
gates and never release contracts (#3324).

The hosted runtime image uses Dockerfile's `runtime-prebuilt` target. It copies
the canonical frontend and Rust release outputs into the image instead of
compiling them again inside Docker. E2E tasks require the already loaded
`ugoite:e2e` image and never invoke an image build. The default Dockerfile
target remains a portable source build for direct Docker and Compose use.
The hosted portable-Space E2E fixture also reuses the release CLI from that
artifact build. Its source-SHA sidecar must match the checked-out source before
the fixture runs; standalone local E2E continues to build through `cargo run`.
Seed and measured-step durations and exit status are emitted even when a step
fails.

The CP1 query and SQL export measurements also write aggregate profiles to
`target/cp1-profiling/` and upload them with their job evidence (the fixtures
job uploads its preparation profiles; each consumer uploads its own). Seed
profiles split Space creation, optional owner initialization, Form upserts,
sample Markdown rendering, draft conversion, and mutation batch calls; process
resource logs report elapsed time, user/system CPU, and maximum RSS where the
host utility provides them. Reports compare the outer seed process wall with
the generator wall and sum timed child processes against the script wall, so
the positive remainder is visible without attributing it to a specific build
step. If timer resolution makes the summed child duration exceed the script
wall, the report records that excess separately instead of presenting a
negative remainder. Reports also include fixture file counts and logical
bytes. Values that cannot be separated with the current interfaces are marked
unmeasured in the JSON; these measurements do not add a performance
pass/fail threshold or change the CP1 acceptance assertions. Fixture archives
are checked against the current fixture specification, source SHA, workflow
run ID, digest, and size before extraction; each consumer repeats the canonical
readback in a fresh private root. The fixture manifest records the verified
Space UIDs, counts, Form distribution, owner mode, generator fingerprint, and
measurement schema. There is no cross-run fixture cache.

Seed profiling stays instrumentation-only over the single shared sample-data
generation path: there are no profiling-only seed paths and no CP1-only
engine, so no profiled/unprofiled semantics oracle is added (#3344).
Seed-phase profiles are diagnostic evidence, not optimization targets:
fixture-seed micro-optimization is out of the product roadmap and returns
only with real product-bottleneck evidence from a user-facing path, never
from fixture wall time alone (#3403).

The query measurement can run its existing seeded fixture through either the
direct host runner (default) or `run-e2e-compose.sh` by setting
`UGOITE_QUERY_MEASURE_RUNNER=compose`. Compose mode consumes the caller-owned
fixture root, returns its filesystem ownership to the invoking UID/GID after
the server stops, and shares the same Playwright task and zero-test/zero-skip
JUnit gate as the direct runner.
The required CP1 lane continues to use the direct runner until the later CI
consumer migration.

## Release contract

`version.txt` is the only prepared-version authority. `version:sync` updates
Cargo, npm, Helm, and `Cargo.lock` projections; `version:check` verifies them.
Stable `v<version>` tags are the published-version ledger. Historical alpha and
beta tags are excluded from stable-version calculation.

The first stable release used the already prepared `0.1.0` as-is and is recorded
by the published `v0.1.0` GitHub Release. Later pre-1.0 releases use
`release:prepare compatible|breaking`, which compares the prepared version with
the latest stable tag before updating projections. A compatible change advances
the patch; a breaking change advances the minor. Preparation never creates a
tag, release, or registry artifact. The current published stable release is
`0.2.0`; its scope is recorded in
[`v0.2.0 release note`](https://github.com/ugoite/ugoite/blob/main/docs/version/releases/v0.2.0.md). Candidate
creation and promotion remain separate operator-controlled workflows for
future releases. Stable publication requires a non-empty,
versioned manual note at `docs/version/releases/v<version>.md`; the note is
validated at candidate preflight and read from the exact candidate source when
published.

`Release Candidate` checks out one exact source SHA, verifies that it is
reachable from `main` and has a successful `ci-required` check, then builds and
packages the candidate artifact set without rerunning the merge gate or full E2E
suite. It stores the result plus a schema-versioned `candidate-manifest.json`.
The manifest records version, source SHA, candidate run identity, source
`ci-required` check-run identity, artifact digests, and platform information.
Its candidate ID is the SHA-256 of the exact manifest bytes; the manifest does
not contain that ID or verification state. Publish preflight records verifier
identity and policy separately in a run-scoped
`verification-receipt-<verification_run_id>.json` sidecar.

`Release Publish` accepts only a candidate run ID, downloads the candidate
artifact, derives its candidate ID from the exact manifest bytes, and invokes
`release:verify-candidate` before promotion. Its verifier is checked out at
`github.workflow_sha`, not from a moving default-branch ref. The publish
preflight starts the exact candidate container digest and runs the exact CLI
archive before promotion. The promotion job contains no compile, build, pack,
package, or repackage step. It publishes the exact CLI archives, npm tarball,
Helm archive, release Compose assets, and container digest from the manifest.
The promotion also attaches a run-scoped verification receipt containing the
candidate ID, candidate run, verifier workflow SHA, verification run ID, policy,
and result; this evidence is separate from candidate identity. Missing
identities are published, matching identities are verified and skipped, and
mismatches abort. Immutable versioned identities are verified before the GitHub
Release is finalized. A separate distribution check verifies released assets,
registry artifacts, container health, and the CLI installer; mutable aliases are
updated only after that check and release-note publication.

Candidate verification and distribution verification are separate. The former
checks staged bytes and exact candidate runtime inputs; the latter checks
published bytes, registry identities, container health, and installer
availability. After distribution verification, the publish workflow applies the
candidate's manual Markdown note as the GitHub Release body and only then
promotes mutable aliases. Historical alpha and beta changelog files are not
active release channels. Neither publish stage runs browser Playwright E2E. Both workflows
keep a top-level `permissions: {}` boundary and grant only job-scoped
permissions.

Detailed provenance evidence and planner-ref recovery remain follow-up work, not
additional v0.1 release authorities.

The focused docsite-navigation lane is intentionally separate from smoke/full
runtime E2E. It builds the docsite through the canonical `build:docsite` inputs,
restores the versioned Playwright browser cache, keeps the explicit
browser-install step as a cache-miss fallback, previews the static artifact, and
verifies Starlight navigation semantics before the heavier runtime-backed smoke
suite runs.

The canonical `mitase:check` task invokes `scripts/mitase check .`. The wrapper
reads `tools/mitase.lock.toml`, selects the host target, downloads the exact
Mitase `v0.2.4` release archive when it is not cached, verifies its SHA-256,
checks the packaged binary version, and then execs it. The default path does not
build Mitase from Git; `MITASE_BIN` remains available as an explicit local
development override.

The required `ci-required` aggregator runs after all quality, artifact,
docsite-navigation, CP1 fixtures/query/export, and impact-report lanes on pull requests,
merge queues, and pushes to `main`. It checks the planned results for
`ci-rust-check`, `ci-rust-test`, `ci-s3-shared-authorization`, `ci-web`,
`artifact-build`, the three E2E consumer jobs, `ci-docsite-nav`, and the `ci-cp1-fixtures`, `ci-cp1-query`,
and `ci-cp1-export` trio (planned and required as a unit by the single cp1
flag), and fails on
unexpected skips or executions. The PR
context report must succeed for pull requests and is accepted as skipped for
other events. The canonical
`test` Mise task and `ci:lane:web` run the normal docsite test suite; only
frontend coverage remains a hard coverage gate. The docsite coverage task is
intentionally separate developer convenience and is not a merge-quality
metric. The active `main only pr` repository ruleset must require the
`ci-required` status-check context; a successful push-to-`main` run alone is not
merge enforcement.

The other required merge-queue checks preserve their event-specific contracts:
the PR body gate resolves the associated pull request from the temporary queue
ref and keeps the existing Dependabot exemption, while CodeQL still analyzes the
queued source but does not upload SARIF to the temporary ref.

The root Mise graph is the repository quality contract: `ci` composes
`fmt:check`, `lint`, `check`, and `test`; `lint` composes Rust and Deno lint
tasks; `check` composes Rust, Deno, and repository contract checks. Hosted lane
tasks are packing adapters only and are covered by
`tools/coverage_gates_test.ts`, which explicitly asserts their semantic
composition and workflow entrypoints. `mise run ci` and `mise run ci:merge`
remain the developer-facing canonical interfaces. Frontend's unit coverage gate
explicitly covers the portable Rust/WASM protocol boundary in
`frontend/src/lib/ugoite-client/protocol.ts`; UI behavior remains covered by
behavior tests and E2E. The optional docsite coverage report includes authored
`src/**/*.{js,mjs,ts,tsx}` while excluding test files, `src/env.d.ts`, and
Astro's framework-only `src/content.config.ts`.

Deployable artifacts are staged below `target/artifacts/` with a
machine-readable `manifest.json` and `SHA256SUMS`. Candidate runs additionally
publish an exact `candidate-manifest.json` and candidate bundle. The release
workflow emits installer-compatible CLI archives named
`ugoite-v<version>-<target>.tar.gz` plus per-file checksums and the release
Compose assets, publishes `@ugoite/ugoite` to GitHub Packages, publishes
`ghcr.io/ugoite/ugoite:<version>`, and pushes the Helm chart to
`oci://ghcr.io/ugoite/charts`.

`.github/workflows/docsite-pages.yml` renders and deploys stable docs only from
a published release tag. It checks out the exact tag bytes, verifies that
`version.txt` and the versioned release note at that tag match the tag, builds
the docsite with the current Pages origin/base metadata, validates the static
output, then deploys. Main-branch CI never deploys to the site root.

Build reuse and test-result caching are different concepts. `sources`/`outputs`
may skip deterministic `build:*` work when inputs are unchanged, but they are
never evidence that a test passed.

Current artifact layout:

```text
target/artifacts/
  manifest.json
  SHA256SUMS
  candidate-manifest.json  # candidate bundle only
  verification-receipt-<verification_run_id>.json  # publish evidence
  docker-compose.release.yaml(.sha256)  # candidate/release assets
  docsite/
  cli/
  helm/
  image/
  npm/
```

Build identity currently includes explicit environment such as `DOCSITE_ORIGIN`,
`DOCSITE_BASE`, and `UGOITE_IMAGE_TAG`. To force a clean rebuild, remove
`target/rust`, `target/wasm`, `target/artifacts`, `frontend/.output`,
`docsite/dist`, and `frontend/src/lib/generated/ugoite_wasm.wasm`.

All task names are root `mise.toml` tasks; package-scoped task syntax is
invalid.
