#!/usr/bin/env bash
# Verify the Tauri bundle identifier matches APP_DIR_NAME in paths.rs.
# Both independently name the same app-data directory, and Tauri resolves the
# assetProtocol "$APPDATA" scope from the identifier -- so if they drift, the
# build stays green and every cover and figure silently stops rendering.
# Usage: check-identifier.sh
# Env:   ROOT  repo root (default: git rev-parse --show-toplevel)
set -euo pipefail

ROOT="${ROOT:-$(git rev-parse --show-toplevel)}"

# Deliberately no Node/jq dependency: this runs in CI before the toolchain is
# set up, alongside the other cheap pre-toolchain checks.
identifier="$(sed -n 's/.*"identifier"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$ROOT/src-tauri/tauri.conf.json" | head -1)"
app_dir="$(sed -n 's/^const APP_DIR_NAME: &str = "\(.*\)";$/\1/p' "$ROOT/src-tauri/src/paths.rs")"

if [ -z "$identifier" ]; then
  echo "check-identifier: no 'identifier' in src-tauri/tauri.conf.json" >&2
  exit 1
fi

if [ -z "$app_dir" ]; then
  echo "check-identifier: no APP_DIR_NAME in src-tauri/src/paths.rs" >&2
  exit 1
fi

if [ "$identifier" != "$app_dir" ]; then
  echo "identifier mismatch: tauri.conf.json has '$identifier', paths.rs APP_DIR_NAME has '$app_dir'" >&2
  exit 1
fi

echo "identifier OK: tauri.conf.json and paths.rs both == $identifier"
