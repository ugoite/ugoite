# Composition fixtures

`space-seed/manifest.json` drives the server-backed Text-first browser journey.
The canonical portable Composition fixture lives with `ugoite-domain`, so the
E2E fixtures do not duplicate it.

`acceptance-plan.json` maps the current Text-first Composition contract to
repository evidence. A selector points to evidence; it does not claim that a
test ran or passed. Responsive behavior and keyboard/focus semantics use the
automated checks listed there. Physical-device testing, actual native browser
zoom, and manual screen-reader sessions are outside the v0.2.2 acceptance
scope.

The raw recovery candidates exercise unsupported format versions and removed
prerelease Composition grammar. They do not define a compatibility path.
