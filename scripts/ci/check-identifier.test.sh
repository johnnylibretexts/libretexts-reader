#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
script="$here/check-identifier.sh"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/src-tauri/src"

write_conf() { printf '{ "identifier": "%s" }\n' "$1" > "$tmp/src-tauri/tauri.conf.json"; }
write_paths() { printf 'const APP_DIR_NAME: &str = "%s";\n' "$1" > "$tmp/src-tauri/src/paths.rs"; }

# Case 1: identifier and APP_DIR_NAME agree -> exit 0
write_conf dev.example.reader
write_paths dev.example.reader
ROOT="$tmp" bash "$script" >/dev/null
echo "PASS: matching identifier accepted"

# Case 2: they disagree -> non-zero
write_paths dev.other.reader
if ROOT="$tmp" bash "$script" >/dev/null 2>&1; then
  echo "FAIL: mismatch not detected" >&2; exit 1
fi
echo "PASS: mismatched identifier rejected"

# Case 3: identifier missing from tauri.conf.json -> non-zero
printf '{ "productName": "X" }\n' > "$tmp/src-tauri/tauri.conf.json"
write_paths dev.example.reader
if ROOT="$tmp" bash "$script" >/dev/null 2>&1; then
  echo "FAIL: missing identifier not detected" >&2; exit 1
fi
echo "PASS: missing identifier rejected"

# Case 4: APP_DIR_NAME missing from paths.rs -> non-zero
write_conf dev.example.reader
printf 'const SOMETHING_ELSE: &str = "x";\n' > "$tmp/src-tauri/src/paths.rs"
if ROOT="$tmp" bash "$script" >/dev/null 2>&1; then
  echo "FAIL: missing APP_DIR_NAME not detected" >&2; exit 1
fi
echo "PASS: missing APP_DIR_NAME rejected"

echo "ALL TESTS PASSED"
