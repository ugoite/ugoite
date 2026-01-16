#!/bin/bash
# Simple smoke tests using curl - no Node.js required
# Usage: ./scripts/smoke-test.sh

# Don't exit on error - we want to run all tests
set +e

BASE_URL="${BASE_URL:-http://localhost:3000}"
API_URL="${API_URL:-http://localhost:8000}"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

TESTS_PASSED=0
TESTS_FAILED=0

log_pass() {
    echo -e "${GREEN}✓${NC} $1"
    TESTS_PASSED=$((TESTS_PASSED + 1))
}

log_fail() {
    echo -e "${RED}✗${NC} $1"
    TESTS_FAILED=$((TESTS_FAILED + 1))
}

log_info() {
    echo -e "${YELLOW}ℹ${NC} $1"
}

# Test: API health check
test_api_health() {
    local response
    response=$(curl -s -w "\n%{http_code}" "$API_URL/workspaces" 2>/dev/null)
    local http_code=$(echo "$response" | tail -n1)
    local body=$(echo "$response" | head -n-1)
    
    if [ "$http_code" = "200" ]; then
        if echo "$body" | grep -q '"healthy"'; then
            log_pass "API health check returns healthy status"
        else
            log_fail "API health check: unexpected response body"
        fi
    else
        log_fail "API health check: HTTP $http_code"
    fi
}

# Test: Frontend loads
test_frontend_loads() {
    local http_code
    http_code=$(curl -s -o /dev/null -w "%{http_code}" "$BASE_URL/" 2>/dev/null)
    
    if [ "$http_code" = "200" ]; then
        log_pass "Frontend homepage loads (HTTP 200)"
    else
        log_fail "Frontend homepage: HTTP $http_code"
    fi
}

# Test: Frontend returns HTML with expected content
test_frontend_content() {
    local response
    response=$(curl -s "$BASE_URL/" 2>/dev/null)
    
    if echo "$response" | grep -qi "ieapp\|<!DOCTYPE html>"; then
        log_pass "Frontend returns valid HTML content"
    else
        log_fail "Frontend content check failed"
    fi
}

# Test: Notes page loads
test_notes_page() {
    local http_code
    http_code=$(curl -s -o /dev/null -w "%{http_code}" "$BASE_URL/notes" 2>/dev/null)
    
    if [ "$http_code" = "200" ]; then
        log_pass "Notes page loads (HTTP 200)"
    else
        log_fail "Notes page: HTTP $http_code"
    fi
}

# Test: API workspaces endpoint
test_api_workspaces() {
    local http_code
    http_code=$(curl -s -o /dev/null -w "%{http_code}" "$API_URL/workspaces" 2>/dev/null)
    
    if [ "$http_code" = "200" ]; then
        log_pass "API /workspaces endpoint works"
    else
        log_fail "API /workspaces: HTTP $http_code"
    fi
}

# Test: Static assets (check if CSS/JS loads)
test_static_assets() {
    local response
    response=$(curl -s "$BASE_URL/" 2>/dev/null)
    
    # Check if page references script or style
    if echo "$response" | grep -qE '<script|<link.*stylesheet'; then
        log_pass "Frontend includes script/style references"
    else
        log_fail "Frontend missing script/style references"
    fi
}

echo "=========================================="
echo "Running Smoke Tests"
echo "=========================================="
echo ""
log_info "Frontend URL: $BASE_URL"
log_info "API URL: $API_URL"
echo ""

# Run all tests
test_api_health
test_frontend_loads
test_frontend_content
test_notes_page
test_api_workspaces
test_static_assets

echo ""
echo "=========================================="
echo "Results: $TESTS_PASSED passed, $TESTS_FAILED failed"
echo "=========================================="

if [ $TESTS_FAILED -gt 0 ]; then
    exit 1
fi
exit 0
