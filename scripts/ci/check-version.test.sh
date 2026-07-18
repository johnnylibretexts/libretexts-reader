#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
script="$here/check-version.sh"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/src-tauri"
printf '{ "version": "1.2.3" }\n' > "$tmp/src-tauri/tauri.conf.json"
printf '{ "version": "1.2.3" }\n' > "$tmp/package.json"
printf '[workspace.package]\nversion = "1.2.3"\n' > "$tmp/Cargo.toml"

# Case 1: all sources match expected -> exit 0
ROOT="$tmp" bash "$script" 1.2.3 >/dev/null
echo "PASS: matching versions accepted"

# Case 2: one source mismatches -> non-zero
printf '{ "version": "9.9.9" }\n' > "$tmp/package.json"
if ROOT="$tmp" bash "$script" 1.2.3 >/dev/null 2>&1; then
  echo "FAIL: mismatch not detected" >&2; exit 1
fi
echo "PASS: mismatched source rejected"

# Case 3: expected differs from (consistent) sources -> non-zero
printf '{ "version": "1.2.3" }\n' > "$tmp/package.json"
if ROOT="$tmp" bash "$script" 0.0.0 >/dev/null 2>&1; then
  echo "FAIL: wrong expected not detected" >&2; exit 1
fi
echo "PASS: wrong expected rejected"

echo "ALL TESTS PASSED"
