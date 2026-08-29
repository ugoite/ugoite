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
operation.

## Candidates

`mise run release:candidate` builds and packages from one exact source SHA, runs
the release-grade checks, and writes `candidate-manifest.json`. The manifest
records the version, source SHA, CI run identity, release-grade result, artifact
paths, digests, sizes, platforms, and container coordinates. A candidate exists
only when `verification.release_grade` is `passed`.

The candidate identity is the SHA-256 digest of the exact manifest bytes. The
manifest does not contain its own identity, so two attempts for the same
prepared version remain distinguishable. Failed attempts do not advance
`version.txt`.

`mise run release:verify-candidate` verifies those exact bytes, the source
version, all recorded artifact digests, and the candidate eligibility without
building or packaging anything.

## Promotion

`mise run release:promote -- --candidate <manifest> --candidate-id <id>` takes
the verified candidate as its only release subject. Promotion uses the exact CLI
archives, npm tarball, Helm archive, release Compose assets, and container
digest recorded by the manifest. It does not compile, package, or repackage
them. It publishes immutable versioned identities first, verifies them, and
finalizes the stable GitHub Release. After the published quick-start checks, the
separate `mise run release:promote:aliases` task updates mutable aliases such as
`latest`.

Each publication is idempotent: a missing identity is published, a matching
identity is verified and skipped, and a different identity aborts. An immutable
public version with corrupted content is never overwritten or reused. A
transient failure can safely rerun the same candidate when its artifact storage
is still available.

Git SHA identifies source; artifact digest identifies bytes; candidate-manifest
digest identifies a verified candidate; SemVer identifies the published
compatibility promise. None of these identities is interchangeable.
