#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/recover-release-planner-ref.sh --repo OWNER/REPO \
  --ref refs/heads/BRANCH --expected-sha SHA [--delete]

The default mode is a dry run. --delete performs a final ref comparison and
deletes only the selected branch ref when it still points at --expected-sha.
EOF
}

fail() {
  printf '%s\n' "$1" >&2
  exit 1
}

repo=""
ref=""
expected_sha=""
delete_ref=false

while (($# > 0)); do
  case "$1" in
    --repo)
      (($# >= 2)) || fail "--repo requires OWNER/REPO"
      repo="$2"
      shift 2
      ;;
    --ref)
      (($# >= 2)) || fail "--ref requires refs/heads/BRANCH"
      ref="$2"
      shift 2
      ;;
    --expected-sha)
      (($# >= 2)) || fail "--expected-sha requires a 40-character commit SHA"
      expected_sha="$2"
      shift 2
      ;;
    --delete)
      delete_ref=true
      shift
      ;;
    --help)
      usage
      exit 0
      ;;
    *)
      fail "Unknown option: $1"
      ;;
  esac
done

[[ "$repo" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] ||
  fail "--repo must be OWNER/REPO"
[[ "$ref" == refs/heads/* ]] ||
  fail "--ref must select a branch under refs/heads/"
branch="${ref#refs/heads/}"
[[ "$branch" =~ ^[A-Za-z0-9._/-]+$ && "$branch" != *..* && "$branch" != *//* ]] ||
  fail "--ref contains an invalid branch name"
[[ "$branch" != */ && "$branch" != /* ]] ||
  fail "--ref must name a non-empty branch"
[[ "$expected_sha" =~ ^[0-9a-f]{40}$ ]] ||
  fail "--expected-sha must be a lowercase 40-character commit SHA"

command -v gh >/dev/null 2>&1 || fail "Recovery requires gh"

read_ref_sha() {
  gh api "repos/${repo}/git/ref/heads/${branch}" --jq '.object.sha'
}

initial_sha="$(read_ref_sha)"
[[ "$initial_sha" =~ ^[0-9a-f]{40}$ ]] ||
  fail "GitHub returned an invalid commit SHA for ${ref}"
printf 'Recovery target: repo=%s ref=%s expected_sha=%s observed_sha=%s\n' \
  "$repo" "$ref" "$expected_sha" "$initial_sha"
[[ "$initial_sha" == "$expected_sha" ]] ||
  fail "Ref does not match expected commit; refusing recovery"

if [[ "$delete_ref" != true ]]; then
  printf 'Dry run: no mutation performed\n'
  exit 0
fi

final_sha="$(read_ref_sha)"
printf 'Final pre-delete check: ref=%s observed_sha=%s\n' "$ref" "$final_sha"
if [[ "$final_sha" != "$expected_sha" ]]; then
  fail "Ref changed during recovery check; refusing deletion"
fi

gh api --method DELETE "repos/${repo}/git/refs/heads/${branch}" >/dev/null
printf 'Deleted branch ref: repo=%s ref=%s commit=%s\n' \
  "$repo" "$ref" "$expected_sha"
