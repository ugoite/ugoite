# Versions Overview

The docsite exposes two layers of release planning:

- `docs/version/*.yaml` is the machine-readable source of truth for version,
  milestone, phase, and task status.
- `docs/spec/versions/*.md` explains what those statuses mean for readers who
  want to understand product evolution instead of raw task state.

## How to Read Ugoite Versions

Ugoite tracks release progress with the following hierarchy:

1. **Version** - a release stream such as `v0.1` or `v0.2`
2. **Milestone** - a large capability slice inside that version
3. **Phase** - design, implementation, testing, or release-preparation work
4. **Task** - the concrete work items that move a phase forward

This means a version number does not change only because a single task was
checked off. A version changes when a milestone or set of milestones adds,
changes, or prepares meaningful product behavior for users and operators.

## Release Streams

| Version | Status | What it means |
|---------|--------|---------------|
| `v0.1` | in progress | Foundational release stream covering MVP, full configuration, Iceberg-backed data model changes, user management, and release preparation |
| `v0.2` | planned | Next stream focused on user-controlled experiences and broader AI-assisted workflows beyond today's resource-first MCP baseline |

## What Each Version Adds

| Capability Area | `v0.1` | `v0.2` |
|-----------------|--------|--------|
| Local-first core app, storage abstraction, REST/MCP baseline | Added | Continues |
| Form-first terminology and shared `ugoite-core` architecture | Added | Continues |
| Iceberg-backed entries and Ugoite SQL workflows | Added | Continues |
| New space UI, responsive layout, theme switching, sample data | Added | Continues |
| Multi-user auth, membership management, audit and service accounts | In progress | Continues |
| Release packaging, operator onboarding, release notes | Planned | Continues |
| User-controlled views driven by queries | Not in scope | Planned |
| AI workflow automation, vector search, voice transcription, computational entries | Not in scope | Planned |

## Reading Version Upgrades

If you want to understand what changed when a version moved forward, use this
flow:

1. Start with the version page in this directory for the human-readable
   summary.
2. Follow the linked milestone YAML files under `docs/version/` when you need
   task-level detail.
3. Use the changelog entrypoint to choose the stable, beta, or alpha release-note view.

## Linked Pages

- [v0.1 release stream](v0.1.md)
- [v0.2 roadmap](v0.2.md)
- [changelog](changelog.md)
- [stable changelog](changelog-stable.md)
- [beta changelog](changelog-beta.md)
- [alpha changelog](changelog-alpha.md)
- [machine-readable v0.1 tracker](../../version/v0.1.yaml)
- [machine-readable v0.2 tracker](../../version/v0.2.yaml)
