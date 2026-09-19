---
title: "Stable changelog"
---

Stable release publication uses the versioned GitHub Release assets,
`ghcr.io/ugoite/ugoite:<version>`,
`oci://ghcr.io/ugoite/charts/ugoite:<version>`, and the GitHub Packages
installer `@ugoite/ugoite@<version>`. Stable releases may additionally refresh
the `stable` and `latest` aliases after the exact version has been published.
The release authority is the manual Markdown file
`docs/version/releases/v<version>.md`. Candidate preflight validates that the
file is non-empty and contains a matching versioned frontmatter title. The
validator also accepts an H1 for source-only notes outside the docsite.
After distribution verification, the publish workflow reads the file from the
candidate's exact source revision and applies those bytes as the GitHub Release
body. Mutable aliases wait for that publication to succeed.

Historical channel metadata (not an active publication source):
[`../../version/changelog/stable.yaml`](https://github.com/ugoite/ugoite/blob/main/docs/version/changelog/stable.yaml).
