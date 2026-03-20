# Changelog

Ugoite now keeps release communication in channel-scoped tracks so stable,
beta, and alpha notes do not blur together in one mixed page.

The machine-readable sources live under `docs/version/changelog/`, while this
entrypoint keeps the human-readable release-note views discoverable from the
existing versions index.

## Release note channels

| Channel | Audience | Human-readable page | Machine-readable source |
|---------|----------|---------------------|-------------------------|
| stable | Supported operator-ready releases | [Stable changelog](changelog-stable.md) | [`../../version/changelog/stable.yaml`](../../version/changelog/stable.yaml) |
| beta | Broad prerelease validation before stable cut | [Beta changelog](changelog-beta.md) | [`../../version/changelog/beta.yaml`](../../version/changelog/beta.yaml) |
| alpha | Earliest validated previews and experiments | [Alpha changelog](changelog-alpha.md) | [`../../version/changelog/alpha.yaml`](../../version/changelog/alpha.yaml) |

Use the page that matches the channel you plan to install or publish:

- [Stable changelog](changelog-stable.md)
- [Beta changelog](changelog-beta.md)
- [Alpha changelog](changelog-alpha.md)

## Version stream snapshots

The version stream pages still explain what `v0.1` and `v0.2` mean overall:

- [v0.1 Release Stream](v0.1.md)
- [v0.2 Roadmap](v0.2.md)
