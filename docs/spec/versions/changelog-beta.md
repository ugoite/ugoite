# Beta Channel Changelog

Beta releases are the broad prerelease validation track between alpha
experimentation and stable support. When `Release Publish` runs with
`channel=beta`, it prepends repository-rendered notes from
`../../version/changelog/beta.yaml` before the GitHub-generated commit summary.

## Current Focus

### Added

- Release quick-start verification across the published browser stories and CLI
  authentication flow
- Deterministic beta image tags and exact beta release asset downloads
- Wider prerelease validation for operator onboarding and install guidance

### Changed

- Beta release communication is separated from stable and alpha notes so
  prerelease reviewers can focus on near-stable risk
- Beta notes emphasize rollout guidance and known validation gaps before a
  stable cut

### Planned

- Broader beta feedback loops for user-management completion and release polish
- Future beta summaries for capabilities graduating toward supported releases

## Source Files

- [Machine-readable beta changelog](../../version/changelog/beta.yaml)
- [Versions overview](index.md)
