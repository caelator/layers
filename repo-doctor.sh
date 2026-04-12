#!/bin/sh
# repo-doctor.sh — Health checks for the layers repository.
#
# Usage:
#   ./repo-doctor.sh         # Local run (human-friendly output)
#   ./repo-doctor.sh --ci    # CI run (strict, exits non-zero on any failure)
#
# Checks:
#   1. core.bare is not true
#   2. core.hooksPath is set to "hooks"
#   3. No force-push indicators in recent reflog
#   4. Working tree is not in detached HEAD state on main
#   5. Required hook files exist and are executable
#   6. CI workflow file exists
#   7. No uncommitted secrets (.env, credentials, tokens)
#
# Exit codes:
#   0 — all checks passed
#   1 — one or more checks failed

set -u

CI_MODE=false
if [ "${1:-}" = "--ci" ]; then
    CI_MODE=true
fi

PASS=0
FAIL=0
WARN=0

pass() {
    PASS=$((PASS + 1))
    echo "  ✓ $1"
}

fail() {
    FAIL=$((FAIL + 1))
    echo "  ✗ $1"
}

warn() {
    WARN=$((WARN + 1))
    echo "  ⚠ $1"
}

echo "repo-doctor: running health checks..."
echo ""

# ── Check 1: core.bare ──
bare_val=$(git config --get core.bare 2>/dev/null || echo "false")
if echo "$bare_val" | grep -qi "true"; then
    fail "core.bare=true — repo should not be bare"
else
    pass "core.bare=false"
fi

# ── Check 2: core.hooksPath ──
hooks_path=$(git config --get core.hooksPath 2>/dev/null || echo "")
if [ "$hooks_path" = "hooks" ]; then
    pass "core.hooksPath=hooks"
elif [ -z "$hooks_path" ]; then
    fail "core.hooksPath not set (should be 'hooks')"
else
    warn "core.hooksPath='$hooks_path' (expected 'hooks')"
fi

# ── Check 3: Required hook files ──
for hook in pre-commit pre-push; do
    if [ -f "hooks/$hook" ]; then
        if [ -x "hooks/$hook" ]; then
            pass "hooks/$hook exists and is executable"
        else
            warn "hooks/$hook exists but is not executable"
        fi
    else
        fail "hooks/$hook is missing"
    fi
done

# ── Check 4: CI workflow ──
if [ -f ".github/workflows/ci.yml" ]; then
    pass ".github/workflows/ci.yml exists"
else
    fail ".github/workflows/ci.yml is missing"
fi

# ── Check 5: No detached HEAD on main ──
current_branch=$(git symbolic-ref --short HEAD 2>/dev/null || echo "DETACHED")
if [ "$current_branch" = "DETACHED" ]; then
    warn "HEAD is detached (not on a branch)"
else
    pass "On branch: $current_branch"
fi

# ── Check 6: No secrets in tracked files ──
secrets_found=false
for pattern in ".env" "credentials.json" "*.pem" "*.key"; do
    if git ls-files "$pattern" 2>/dev/null | grep -q .; then
        fail "Tracked secret file matching '$pattern'"
        secrets_found=true
    fi
done
if [ "$secrets_found" = "false" ]; then
    pass "No tracked secret files detected"
fi

# ── Check 7: Force-push detection in reflog (last 50 entries) ──
if git reflog 2>/dev/null | head -50 | grep -qi "forced-update\|reset.*hard"; then
    warn "Recent reflog contains force-update or hard-reset entries"
else
    pass "No force-push indicators in recent reflog"
fi

# ── Check 8: Hook safety guards present ──
if [ -f "hooks/pre-commit" ]; then
    if grep -q "core.bare" "hooks/pre-commit"; then
        pass "pre-commit includes bare-repo guard"
    else
        fail "pre-commit missing bare-repo guard"
    fi
    if grep -q "mass.delet" "hooks/pre-commit" || grep -q "staged_deletes" "hooks/pre-commit"; then
        pass "pre-commit includes mass-deletion guard"
    else
        fail "pre-commit missing mass-deletion guard"
    fi
fi

if [ -f "hooks/pre-push" ]; then
    if grep -q "force.push\|merge-base.*is-ancestor" "hooks/pre-push"; then
        pass "pre-push includes force-push block"
    else
        fail "pre-push missing force-push block"
    fi
fi

# ── Summary ──
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Result: $PASS passed, $FAIL failed, $WARN warnings"

if [ "$FAIL" -gt 0 ]; then
    echo "  Result: FAIL"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    exit 1
else
    echo "  Result: PASS"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    exit 0
fi
