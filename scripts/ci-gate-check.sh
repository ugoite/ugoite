#!/usr/bin/env bash
set -euo pipefail

fail() {
  printf 'ci-required: %s\n' "$1" >&2
  exit 1
}

require_success() {
  local name="$1"
  local result="$2"
  [[ "$result" == "success" ]] || fail "$name result was $result"
}

require_success "impact" "${IMPACT_RESULT:-missing}"
[[ "${IMPACT_PLAN_STATUS:-}" == "ok" ]] || fail "impact plan was not validated"
[[ "${IMPACT_PLAN_SCOPE:-}" == "all" || "${IMPACT_PLAN_SCOPE:-}" == "scoped" ]] || fail "impact scope was invalid"
[[ "${IMPACT_JOBS_SKIPPED:-}" == "false" ]] || fail "shadow mode must not skip jobs"
if [[ "${EVENT_NAME:-}" == "merge_group" && "${IMPACT_PLAN_SCOPE:-}" != "all" ]]; then
  fail "merge-group impact scope must be all"
fi
require_success "rust-check" "${RUST_CHECK_RESULT:-missing}"
require_success "rust-test" "${RUST_TEST_RESULT:-missing}"
require_success "web" "${WEB_RESULT:-missing}"
require_success "artifacts" "${ARTIFACTS_RESULT:-missing}"
require_success "docsite-nav" "${DOCSITE_NAV_RESULT:-missing}"

if [[ "${EVENT_NAME:-}" == "pull_request" ]]; then
  require_success "pr-context-report" "${PR_CONTEXT_RESULT:-missing}"
else
  [[ "${PR_CONTEXT_RESULT:-}" == "skipped" ]] || fail "pr-context-report result was ${PR_CONTEXT_RESULT:-missing} outside pull_request"
fi

printf 'ci-required: all CI lanes succeeded; impact report remained shadow-only\n'
