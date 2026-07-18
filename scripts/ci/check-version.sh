#!/usr/bin/env bash
# Verify the given version matches every version source in the repo.
# Usage: check-version.sh <expected-version>   (e.g. 0.1.0, no leading "v")
# Env:   ROOT  repo root (default: git rev-parse --show-toplevel)
set -euo pipefail

expected="${1:?usage: check-version.sh <expected-version>}"
ROOT="${ROOT:-$(git rev-parse --show-toplevel)}"

read_json_version() { # $1 = path to a JSON file with a top-level "version"
  node -e "process.stdout.write(String(require('$1').version))"
}

tauri_v="$(read_json_version "$ROOT/src-tauri/tauri.conf.json")"
pkg_v="$(read_json_version "$ROOT/package.json")"
# First `version = "x"` line after the [workspace.package] header.
cargo_v="$(awk '/^\[workspace\.package\]/{f=1} f && /^version[[:space:]]*=/{gsub(/[",]/,"",$3); print $3; exit}' "$ROOT/Cargo.toml")"

fail=0
for pair in "tauri.conf.json:$tauri_v" "package.json:$pkg_v" "Cargo.toml:$cargo_v"; do
  name="${pair%%:*}"; val="${pair#*:}"
  if [ "$val" != "$expected" ]; then
    echo "version mismatch: $name has '$val', expected '$expected'" >&2
    fail=1
  fi
done

if [ "$fail" -eq 0 ]; then
  echo "version OK: tauri.conf.json, package.json, Cargo.toml all == $expected"
fi
exit "$fail"
