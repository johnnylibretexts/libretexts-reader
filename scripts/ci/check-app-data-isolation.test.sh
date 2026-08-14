#!/usr/bin/env bash
# Tests for check-app-data-isolation.sh. Substitutes TEST_CMD for the real suite
# so these run in milliseconds and can prove the guard actually catches a leak --
# a guard that only ever sees clean runs is indistinguishable from one that
# always passes.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
subject="$script_dir/check-app-data-isolation.sh"
pass=0
fail=0

expect() { # $1 = "pass"|"fail", $2 = description, $3 = TEST_CMD
  local status=0
  TEST_CMD="$3" "$subject" >/dev/null 2>&1 || status=$?
  if { [ "$1" = "pass" ] && [ "$status" -eq 0 ]; } || { [ "$1" = "fail" ] && [ "$status" -ne 0 ]; }; then
    echo "ok: $2"; pass=$((pass + 1))
  else
    echo "FAILED: $2 (exit $status, expected $1)" >&2; fail=$((fail + 1))
  fi
}

expect pass "a suite that touches nothing" \
  'true'

expect pass "a suite that writes only outside HOME" \
  'd="$(mktemp -d)"; touch "$d/scratch"; rm -rf "$d"'

# The real regression: paths::app_subdir("cache") under the macOS layout.
expect fail "a suite that creates the macOS app-data tree" \
  'mkdir -p "$HOME/Library/Application Support/dev.johnnylibretexts.reader/cache"'

expect fail "a suite that creates the XDG app-data tree" \
  'mkdir -p "$XDG_DATA_HOME/dev.johnnylibretexts.reader"'

expect fail "a suite that creates the Windows app-data tree" \
  'mkdir -p "$APPDATA/dev.johnnylibretexts.reader"'

# Any dotfile counts: the check is "wrote into HOME", not "wrote the app dir",
# so a helper that drops a config under $HOME is caught too.
expect fail "a suite that writes a stray dotfile into HOME" \
  'touch "$HOME/.some-cache"'

# A failing suite must surface as a failure, not be masked by a clean HOME.
expect fail "a failing test command with a clean HOME" \
  'exit 3'

echo
echo "$pass passed, $fail failed"
[ "$fail" -eq 0 ]
