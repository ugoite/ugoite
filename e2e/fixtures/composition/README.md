# Composition fixtures

`space-seed/manifest.json` drives the server-backed Text-first browser journey.
The canonical portable Composition fixture lives with `ugoite-domain`, so the
E2E fixtures do not duplicate it.

Current requirements and verification bindings live in
[Mitase](https://github.com/ugoite/ugoite/tree/main/docs/mitase/requirements/composition). Hosted test
results are recorded by [CI](https://github.com/ugoite/ugoite/actions/workflows/ci.yml).

The raw recovery candidates exercise unsupported format versions and removed
prerelease Composition grammar. They do not define a compatibility path.
