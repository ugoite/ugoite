#!/bin/bash
# Focused docsite-navigation E2E runner.
# This lane verifies the built Starlight artifact without starting the full
# backend/frontend stack, but it still prepares Playwright browsers and enforces
# the same non-zero/non-skipped expectations as the broader E2E runners.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"

ensure_playwright_browsers() {
  if [ "${UGOITE_SKIP_PLAYWRIGHT_DEPS:-}" = "1" ]; then
    echo "Skipping Playwright browser install because UGOITE_SKIP_PLAYWRIGHT_DEPS=1"
    return
  fi
  echo "Installing Playwright browsers..."
  (cd "$ROOT_DIR/e2e" && deno task install:browsers)
}

ensure_playwright_browsers

export PLAYWRIGHT_CI_REPORTER="${PLAYWRIGHT_CI_REPORTER:-junit}"
export PLAYWRIGHT_JUNIT_OUTPUT_FILE="${PLAYWRIGHT_JUNIT_OUTPUT_FILE:-test-results/docsite-navigation-junit.xml}"

cd "$ROOT_DIR/e2e"
mkdir -p "$(dirname "$PLAYWRIGHT_JUNIT_OUTPUT_FILE")"
rm -f "$PLAYWRIGHT_JUNIT_OUTPUT_FILE"

cmd=(deno task docsite:navigation:raw --)
if [ -n "${E2E_TEST_TIMEOUT_MS:-}" ]; then
  cmd+=(--timeout "$E2E_TEST_TIMEOUT_MS")
fi
"${cmd[@]}"

deno eval '
  const report = Deno.env.get("PLAYWRIGHT_JUNIT_OUTPUT_FILE");
  if (!report) throw new Error("PLAYWRIGHT_JUNIT_OUTPUT_FILE is required");
  const xml = await Deno.readTextFile(report);
  const suites = [...xml.matchAll(/<testsuite\b[^>]*>/g)].map((match) => match[0]);
  const attr = (text, name) => Number(text.match(new RegExp(`${name}="([^"]*)"`))?.[1] ?? 0);
  const tests = suites.reduce((sum, suite) => sum + attr(suite, "tests"), 0);
  const skipped = suites.reduce((sum, suite) => sum + attr(suite, "skipped"), 0);
  if (tests === 0) throw new Error("docsite navigation e2e: zero executed tests");
  if (skipped > 0) throw new Error(`docsite navigation e2e: skipped=${skipped} is not allowed`);
  console.log(`docsite navigation e2e OK: tests=${tests}, skipped=${skipped}`);
'
