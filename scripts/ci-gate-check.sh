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
validate_lane() {
  local name="$1"
  local planned="$2"
  local result="$3"
  case "$planned" in
    true) require_success "$name" "$result" ;;
    false) [[ "$result" == "skipped" ]] || fail "$name was unplanned but result was $result" ;;
    *) fail "$name plan was invalid: $planned" ;;
  esac
}

any_skipped=false
for planned in \
  "${PLAN_RUST_CHECK:-missing}" "${PLAN_RUST_TEST:-missing}" \
  "${PLAN_WEB:-missing}" "${PLAN_ARTIFACTS:-missing}" \
  "${PLAN_DOCSITE_NAV:-missing}" "${PLAN_CP1_ACCEPTANCE:-missing}"; do
  case "$planned" in
    true) ;;
    false) any_skipped=true ;;
    *) fail "lane plan contained an invalid value: $planned" ;;
  esac
done
[[ "${IMPACT_JOBS_SKIPPED:-}" == "$any_skipped" ]] || fail "jobs_skipped did not match the planned lanes"

if [[ "${EVENT_NAME:-}" == "merge_group" || "${EVENT_NAME:-}" == "push" ]]; then
  [[ "${IMPACT_PLAN_SCOPE:-}" == "all" ]] || fail "$EVENT_NAME impact scope must be all"
  [[ "$any_skipped" == "false" ]] || fail "$EVENT_NAME must run every lane"
fi

validate_lane "rust-check" "${PLAN_RUST_CHECK:-missing}" "${RUST_CHECK_RESULT:-missing}"
validate_lane "rust-test" "${PLAN_RUST_TEST:-missing}" "${RUST_TEST_RESULT:-missing}"
validate_lane "web" "${PLAN_WEB:-missing}" "${WEB_RESULT:-missing}"
validate_lane "artifact-build" "${PLAN_ARTIFACTS:-missing}" "${ARTIFACT_BUILD_RESULT:-missing}"
validate_lane "e2e-smoke-mobile" "${PLAN_ARTIFACTS:-missing}" "${E2E_SMOKE_MOBILE_RESULT:-missing}"
validate_lane "e2e-owner" "${PLAN_ARTIFACTS:-missing}" "${E2E_OWNER_RESULT:-missing}"
validate_lane "e2e-portable" "${PLAN_ARTIFACTS:-missing}" "${E2E_PORTABLE_RESULT:-missing}"
validate_lane "docsite-nav" "${PLAN_DOCSITE_NAV:-missing}" "${DOCSITE_NAV_RESULT:-missing}"

# The single cp1 plan flag gates the fixtures/query/export trio as a unit:
# when planned all three must succeed, when unplanned all three must be
# skipped. Any mixed state (exactly one skipped or failed, timeout/cancel)
# fails closed.
validate_cp1_trio() {
  local planned="$1"
  local fixtures="$2"
  local query="$3"
  local export="$4"
  case "$planned" in
    true)
      require_success "cp1-fixtures" "$fixtures"
      require_success "cp1-query" "$query"
      require_success "cp1-export" "$export"
      ;;
    false)
      [[ "$fixtures" == "skipped" ]] || fail "cp1-fixtures was unplanned but result was $fixtures"
      [[ "$query" == "skipped" ]] || fail "cp1-query was unplanned but result was $query"
      [[ "$export" == "skipped" ]] || fail "cp1-export was unplanned but result was $export"
      ;;
    *) fail "cp1 plan was invalid: $planned" ;;
  esac
}

validate_cp1_trio "${PLAN_CP1_ACCEPTANCE:-missing}" \
  "${CP1_FIXTURES_RESULT:-missing}" \
  "${CP1_QUERY_RESULT:-missing}" \
  "${CP1_EXPORT_RESULT:-missing}"

if [[ "${EVENT_NAME:-}" == "pull_request" ]]; then
  require_success "pr-context-report" "${PR_CONTEXT_RESULT:-missing}"
else
  [[ "${PR_CONTEXT_RESULT:-}" == "skipped" ]] || fail "pr-context-report result was ${PR_CONTEXT_RESULT:-missing} outside pull_request"
fi

printf 'ci-required: planned lanes matched their results\n'
