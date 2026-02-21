#!/usr/bin/env bash
set -euo pipefail

rm -rf e2e/test-results e2e/playwright-report e2e/.playwright
rm -f backend-pytest.xml cli-pytest.xml core-pytest.xml docs-pytest.xml

echo "Cleaned common local review artifacts"
