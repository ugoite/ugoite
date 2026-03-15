# ugoite installer package

This directory is the public package-managed distribution surface for the
released `ugoite` CLI.

The repository root `package.json` remains private and only exists for
repository-level Husky/commitlint tooling. This package is the public metadata
surface that release automation versions for registry publication.

## Install after publish

```bash
npm install -g ugoite
ugoite-install
ugoite --help
```

`ugoite-install` boots the canonical `scripts/install-ugoite-cli.sh` release
installer from the matching version tag in this repository. By default it uses
the package version as `UGOITE_VERSION` so the package and the released Rust CLI
stay aligned.

Environment passthrough:

- `UGOITE_VERSION`
- `UGOITE_INSTALL_DIR`
- `UGOITE_GITHUB_REPO`
- `UGOITE_DOWNLOAD_BASE_URL`

Current package metadata targets the same released CLI targets as the shell
installer today: Linux/macOS on `x64` and `arm64`.
