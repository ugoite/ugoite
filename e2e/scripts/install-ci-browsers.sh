#!/usr/bin/env bash
set -euo pipefail

playwright=(deno run --node-modules-dir=none -A 'npm:@playwright/test@^1.60.0')
"${playwright[@]}" install chromium

check_chromium() {
  deno eval --node-modules-dir=none '
    import { chromium } from "npm:@playwright/test@^1.60.0";
    const browser = await chromium.launch();
    await browser.close();
  '
}

# The hosted runner normally has Chromium's libraries. A launch checks the
# actual cached browser against this runner before invoking the system installer.
if ! check_chromium; then
  "${playwright[@]}" install-deps chromium
  check_chromium
fi
