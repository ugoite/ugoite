# ugoite npm installer

Release-oriented installer package source for Ugoite CLI binaries on supported Linux and macOS architectures. A checkout of this repository does not prove that the npm package or matching GitHub release assets have been published.

The package is not the repository development toolchain and does not contain the Ugoite application. When a matching package version and GitHub release exist, its `ugoite-install` executable resolves the release archive, downloads the checksum manifest and signature material expected by the release process, verifies the archive, and installs the CLI binary.

For an actually published package, prefer a pinned version:

```bash
npx ugoite@<version>
```

Do not use this package as a replacement for `mise`, Rust, or Deno when developing the repository.
