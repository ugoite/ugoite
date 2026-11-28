---
title: "Compact control browser evidence"
---

The `mobile-ui` Playwright task checks **11 nested surfaces** that render the
shared `BackLink`, including Composition create and history. The exact surface
list and selectors are owned by `e2e/compact-controls-evidence.test.ts`; the
Composition edit route is not part of that list.

Each surface is checked at 390 CSS pixels and at 320 CSS pixels. The latter is
an effective-width proxy for 200% zoom, not a native browser zoom run. The test
checks the visible destination-named 44px link, page title, header actions,
overflow, and keyboard focus. It records the source SHA and outcome in the
Playwright artifact.

The same task checks an over-capacity icon-only action bar at both widths.
Automated accessible-name and keyboard-focus checks run in Chromium. Manual
VoiceOver/NVDA reading-order and announcement evidence remains unverified.
