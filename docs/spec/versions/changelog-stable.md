# Stable Channel Changelog

Stable releases are the operator-facing source of truth for supported Ugoite
behavior. When `Release Publish` runs with `channel=stable`, it prepends
repository-rendered notes from `../../version/changelog/stable.yaml` before the
GitHub-generated commit summary.

## Current Focus

### Added

- Local-first MVP behavior across backend, frontend, CLI, and storage abstraction
- Deployable backend/frontend release images plus exact-version quick-start assets
- Predictable CLI archives and installer assets for supported release platforms

### Changed

- Release communication is tracked as repository-managed stable notes instead of
  sharing one mixed changelog page with prerelease channels
- Stable notes stay focused on supported workflows, upgrade impact, and
  operator guidance

### Planned

- Final v0.1 release communication with known issues and next-step guidance
- Stable summaries for future release streams after release-preparation closes

## Source Files

- [Machine-readable stable changelog](../../version/changelog/stable.yaml)
- [Versions overview](index.md)
