# Composition fixtures

`space-seed/manifest.json` drives the server-backed SQL-first browser journey.
The canonical portable Composition fixture lives with `ugoite-domain`, so the
E2E fixtures do not duplicate it.

`acceptance-plan.json` preserves the attached implementation plan's A01–A14
acceptance conditions and points to relevant repository evidence. A selector
records planned evidence; it does not claim that a test ran or passed. Manual
Safari and assistive-technology checks remain explicit evidence gaps.

The raw recovery candidates exercise unsupported format versions and removed
prerelease Composition grammar. They do not define a compatibility path.
