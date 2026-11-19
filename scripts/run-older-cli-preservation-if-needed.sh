#!/usr/bin/env bash
set -euo pipefail

if [[ "${CI_IMPACT_EVENT:-}" == "pull_request" &&
  "${CI_IMPACT_SCOPE:-}" == "scoped" &&
  "${CI_IMPACT_RUN_OLDER_CLI:-}" == "false" ]]; then
  echo "Skipping older-CLI Knowledge preservation for an unrelated scoped pull request."
  exit 0
fi

mise run test:older-cli-knowledge-preservation
