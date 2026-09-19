---
title: "Changelog channels"
---

The repository keeps separate stable, beta, and alpha source files so historical
release guidance remains attributable. Current publication is orchestrated by
`.github/workflows/release-publish.yml` for stable verified candidates only;
alpha and beta are not normal public release channels. Each stable candidate
must carry a non-empty manual note at
`docs/version/releases/v<version>.md`. After distribution verification, the
publish workflow reads that note from the candidate's exact source revision and
applies it as the GitHub Release body. Reruns reapply the same source note;
there is no generated channel section to merge.

- [Stable release-note contract](changelog-stable.md)
- [Beta historical record](changelog-beta.md)
- [Alpha historical record](changelog-alpha.md)
