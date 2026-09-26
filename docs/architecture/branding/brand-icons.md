---
title: "Brand icons"
---

The original [`ugoite-mark.svg`](../../../frontend/public/brand/ugoite-mark.svg)
is the only source of the Ugoite `!!` mark. Keep its shapes and colors intact.
The square SVG, favicon, touch icon, PWA icons, and GitHub upload images are
generated from that source:

```sh
deno task brand:icons
```

The generator uses the pinned `sharp@0.34.5` dependency. It writes a provenance
manifest with the source and output SHA-256 values, dimensions, and renderer to
[`docs/brand/assets/manifest.json`](../../brand/assets/manifest.json). Review
that manifest with every generated asset update. The square mark uses a white
background and centered, undistorted mark; the favicon uses a slightly larger
mark to retain detail at 16 pixels. The SVG favicon uses a self-contained copy
of the original mark geometry. The original transparent SVG remains in the
sidebar and login UI.

The social preview is 1280×640 PNG with a white background, the mark, and the
name “Ugoite”. The organization avatar image is square and leaves room for
GitHub's circular crop. Both are prepared images; committing them does not
change GitHub settings. An organization owner must decide whether to apply the
avatar because it affects the whole `ugoite` organization. A repository
administrator can apply the social preview under repository Settings. Record
those external updates separately from the code change.

To revert the frontend icons, restore the generated assets and HTML/manifest
declarations together, then run `deno task brand:icons` only after the source
state has been restored. GitHub images must be restored through their respective
settings. Browser, PWA, and social-card caches can delay visible changes.
