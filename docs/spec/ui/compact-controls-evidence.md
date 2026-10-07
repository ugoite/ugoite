---
title: "Compact control browser evidence"
---

The merge-gated `mobile-ui` Playwright task runs
`e2e/compact-controls-evidence.test.ts` for the twelve nested surfaces that
render the shared `BackLink`. Each case checks one visible, destination-named
44px link, a visible page title (or the Composition Studio's visible name
field), visible and pointer-reachable header actions, a non-overflowing header,
and keyboard focus with a visible focus ring. It records the source SHA, exact
BackLink selector, surface, viewport, locale, result, screenshot filename, and
evidence gap in `compact-controls-evidence.json`.

The test captures screenshots at 390 CSS pixels and at a 320 CSS-pixel
effective-width proxy for 200% zoom. This uses the same responsive-E2E
convention as `login-responsive.test.ts`; Playwright sets the viewport width
to 320 CSS pixels rather than changing Chromium's browser zoom. The screenshots
and JSON record are part of the `ci-e2e-smoke-mobile` artifact.

Automated accessible-name and keyboard-focus checks run in Chromium. Manual
VoiceOver or NVDA reading-order and announcement evidence has not been
collected and remains unverified; the browser record states this gap for every
surface and viewport.
