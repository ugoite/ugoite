#!/usr/bin/env bash

validate_junit_report() {
  local report="$1"
  PLAYWRIGHT_JUNIT_OUTPUT_FILE="$report" deno eval '
    const report = Deno.env.get("PLAYWRIGHT_JUNIT_OUTPUT_FILE");
    if (!report) throw new Error("PLAYWRIGHT_JUNIT_OUTPUT_FILE is required");
    const xml = await Deno.readTextFile(report);
    const suites = [...xml.matchAll(/<testsuite\b[^>]*>/g)].map((match) => match[0]);
    const attr = (text, name) => Number(text.match(new RegExp(`${name}="([^"]*)"`))?.[1] ?? 0);
    const tests = suites.reduce((sum, suite) => sum + attr(suite, "tests"), 0);
    const skipped = suites.reduce((sum, suite) => sum + attr(suite, "skipped"), 0);
    if (tests === 0) throw new Error("e2e tests: zero executed tests");
    if (skipped > 0) throw new Error(`e2e tests: skipped=${skipped} is not allowed`);
    console.log(`e2e tests OK: tests=${tests}, skipped=${skipped}`);
  '
}

run_e2e_task() {
  local task="$1"
  local report="$2"
  local enforce_ci_gates="${3:-false}"
  local -a cmd=(deno task "$task")

  if [[ "$enforce_ci_gates" == "true" ]]; then
    export PLAYWRIGHT_JUNIT_OUTPUT_FILE="$report"
    mkdir -p "$(dirname "$report")"
    rm -f "$report"
  fi

  if [[ -n "${E2E_TEST_TIMEOUT_MS:-}" ]]; then
    cmd+=(-- --timeout "$E2E_TEST_TIMEOUT_MS")
  fi
  "${cmd[@]}"

  if [[ "$enforce_ci_gates" == "true" ]]; then
    validate_junit_report "$report"
  fi
}
