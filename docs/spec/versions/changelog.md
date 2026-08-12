---
title: 'Changelog channels'
---

The repository keeps separate stable, beta, and alpha source files. Release publication is orchestrated by `.github/workflows/release-publish.yml`: stable releases may refresh `latest`/`stable`, while prereleases refresh only their matching `alpha` or `beta` alias. After artifact publication and quick-start verification, the matching source is rendered into a marked section of the GitHub Release body; reruns replace that section without duplicating the generated notes.

- [Stable](changelog-stable.md)
- [Beta](changelog-beta.md)
- [Alpha](changelog-alpha.md)
