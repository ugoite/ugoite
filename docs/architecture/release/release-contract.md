---
title: "Versioning and release contract"
sidebar:
  order: 3
---

Ugoite treats version preparation, candidate generation, candidate verification,
and promotion as separate operations:

```text
prepare -> build/package -> verify -> promote
```

The repository owns the prepared product version in `version.txt`. Cargo, npm,
and Helm metadata are projections. Run `mise run version:sync` after changing
the canonical file and `mise run version:check` to verify every projection,
including `Cargo.lock`.

Ugoite uses one lockstep product version for the CLI, container, npm installer,
Helm chart, and other supported product artifacts. Normal feature, fix,
refactor, and documentation changes do not update version metadata.

Before 1.0, `0.Y` is a compatibility epoch. A compatible release advances the
patch (`0.1.4` to `0.1.5`); a breaking release advances the minor and resets the
patch (`0.1.4` to `0.2.0`). The `release:prepare compatible|breaking` interface
is intentionally pre-1.0 only. Reaching `1.0.0` is a product decision based on
the supported compatibility contract, not on a version counter.

Compatibility is judged at supported user-visible surfaces: portable Space data,
documented CLI behavior, configuration and environment variables, REST and
OpenAPI, deployment and Helm interfaces, and supported upgrade paths. Supported
surfaces are release contracts; experimental/preview surfaces are explicitly
excluded from that guarantee; internal APIs and private storage layout are not
product compatibility surfaces. This v1-before-release repository does not add
readers or migrations unless a later feature explicitly requires them.

Stable `v<version>` Git tags are the published-version ledger. Historical
alpha/beta tags are not used to calculate the next stable version. The first
stable release promotes the already prepared `0.1.0` without another prepare
operation. The prepared `0.1.1` release line is a compatible patch and keeps the
Space compatibility identity at `0.1`; it does not require migration or
re-encoding.

Release platform support is tiered. The current Tier 1 set is the four CLI
targets (`x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`,
`x86_64-apple-darwin`, and `aarch64-apple-darwin`) plus the Linux `amd64` and
`arm64` container image; npm and Helm are platform-neutral. Tier 2 is reserved
for targets validated best-effort in nightly or manual runs and is currently
empty. Targets in neither tier are unsupported. See
[Release platform support](platform-support.md) for the promotion criteria and
the current assignment.

## Candidates

`mise run release:candidate` builds and packages from one exact source SHA and
writes `candidate-manifest.json`. Hosted candidate generation first requires the
selected SHA to be reachable from `main` and to have a successful `ci-required`
check. That source-qualification result is recorded by check-run identity; the
candidate workflow does not rerun the merge gate or full E2E suite. The manifest
records the version, source SHA, candidate run identity, source `ci-required`
check-run identity, artifact paths, digests, sizes, platforms, and container
coordinates. The candidate manifest contains release artifact identity only; it
does not contain verification state.

The candidate identity is the SHA-256 digest of the exact manifest bytes. The
manifest does not contain its own identity, so two attempts for the same
prepared version remain distinguishable. Failed attempts do not advance
`version.txt`.

`mise run release:verify-candidate` verifies those exact bytes, the source
version, all recorded artifact digests, and the candidate eligibility without
building or packaging anything. Publish receives only the candidate workflow run
ID; it derives `candidate_id` from the exact manifest bytes and verifies that
the manifest's `ci_run_id` matches the supplied run ID. The publish preflight
then runs `release:verify-candidate-assets` against the exact CLI archives,
npm/Helm archives, and container `repository@digest`; this is the minimum
product contract for the candidate that will actually be promoted. It writes a
run-scoped `verification-receipt-<verification_run_id>.json` sidecar containing
the candidate ID, candidate run, immutable verifier workflow SHA, verification
run ID, policy, and result. The receipt is evidence attached to the candidate;
it is not included in the candidate manifest digest. Run-scoped names preserve
prior evidence when a failed publication is retried.

## Promotion

`mise run release:promote -- --candidate <manifest> --candidate-run-id <run>`
takes the verified candidate as its only release subject. The candidate ID is
always derived from the exact manifest bytes; the run ID is the only operator
input and must match `manifest.ci_run_id`. Promotion requires a matching passed
verification receipt. Promotion uses the exact CLI archives, npm tarball, Helm
archive, release Compose assets, and container digest recorded by the manifest.
It does not compile, package, or repackage them. It publishes immutable
versioned identities first, verifies them, and finalizes the stable GitHub
Release. GitHub Immutable Releases is enabled for future releases, and promotion
verifies that the finalized release reports `isImmutable=true`. After the
post-publish distribution check, the separate `mise run release:promote:aliases`
task updates mutable aliases such as `latest`.

The publish workflow checks out its verifier and release-note code at
`github.workflow_sha`, never at the moving `main` ref. The workflow state is
explicit: candidate, verified, publishing, versioned-published,
distribution-verified, and announced. A failed later step leaves already
published versioned identities intact and rerunning the same candidate resumes
that promotion.

The distribution check verifies the released assets, npm and Helm coordinates,
container health, and CLI installer before the mutable aliases are changed. The
published release manifest lists only GitHub Release assets in `files`; npm and
Helm package digests are recorded in typed `npm_package` and `helm_chart`
projections so registry artifacts are not mistaken for Release assets during
distribution verification.

Each publication is idempotent: a missing identity is published, a matching
identity is verified and skipped, and a different identity aborts. An immutable
public version with corrupted content is never overwritten or reused. A
transient failure can safely rerun the same candidate when its artifact storage
is still available.

Promotion has explicit operational states:

```text
candidate -> verified -> publishing -> versioned-published
          -> distribution-verified -> announced
```

The states are workflow boundaries, not a second release database. A failed run
is resumed with the same candidate; versioned artifacts are never deleted or
overwritten. Release notes and mutable aliases are updated only after
`distribution-verified`. Broad browser E2E remains a PR, nightly, or explicit
release-impact check rather than a publish or post-publish gate.

Git SHA identifies source; artifact digest identifies bytes; candidate-manifest
digest identifies a verified candidate; SemVer identifies the published
compatibility promise. None of these identities is interchangeable.
