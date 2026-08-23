#!/usr/bin/env bash
# Tests for check-notices.sh. Builds throwaway repo roots with known lockfile
# contents and checks that the notice is accepted only when its recorded
# fingerprints match them.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
subject="$script_dir/check-notices.sh"
pass=0
fail=0

make_root() { # $1 = Cargo.lock body, $2 = package-lock body, $3 = "fresh"|"stale"|"unfingerprinted"
  local root; root="$(mktemp -d)"
  mkdir -p "$root/LICENSES"
  printf '%s\n' "$1" > "$root/Cargo.lock"
  printf '%s\n' "$2" > "$root/package-lock.json"

  local cargo_hash npm_hash
  cargo_hash="$(shasum -a 256 "$root/Cargo.lock" | cut -d' ' -f1)"
  npm_hash="$(shasum -a 256 "$root/package-lock.json" | cut -d' ' -f1)"

  case "$3" in
    stale) cargo_hash="0000000000000000000000000000000000000000000000000000000000000000" ;;
    unfingerprinted) cargo_hash=""; npm_hash="" ;;
  esac

  {
    echo "# Third-party notices"
    if [ "$3" != "unfingerprinted" ]; then
      echo "<!-- cargo-lock-sha256: $cargo_hash -->"
      echo "<!-- npm-lock-sha256: $npm_hash -->"
    fi
  } > "$root/LICENSES/NOTICE-third-party.md"

  echo "$root"
}

expect() { # $1 = "pass"|"fail", $2 = description, $3 = root
  local status=0
  ROOT="$3" "$subject" >/dev/null 2>&1 || status=$?
  if { [ "$1" = "pass" ] && [ "$status" -eq 0 ]; } || { [ "$1" = "fail" ] && [ "$status" -ne 0 ]; }; then
    echo "ok: $2"; pass=$((pass + 1))
  else
    echo "FAILED: $2 (exit $status, expected $1)" >&2; fail=$((fail + 1))
  fi
  rm -rf "$3"
}

expect pass "notices generated from the current lockfiles" \
  "$(make_root "crate-a 1.0" '{"name":"app"}' fresh)"

expect fail "a Cargo.lock that moved after the notices were generated" \
  "$(make_root "crate-a 1.0" '{"name":"app"}' stale)"

# The npm side gets its own case: a check that only ever compared the Rust
# lockfile would pass every JavaScript dependency change.
npm_root="$(make_root "crate-a 1.0" '{"name":"app"}' fresh)"
printf '%s\n' '{"name":"app","dependencies":{"left-pad":"1.0.0"}}' > "$npm_root/package-lock.json"
expect fail "a package-lock.json that moved after the notices were generated" "$npm_root"

# Fail-closed: nothing to compare must not read as nothing has changed.
expect fail "a notice carrying no fingerprint at all" \
  "$(make_root "crate-a 1.0" '{"name":"app"}' unfingerprinted)"

missing_notice="$(mktemp -d)"
mkdir -p "$missing_notice/LICENSES"
echo "crate-a 1.0" > "$missing_notice/Cargo.lock"
echo '{}' > "$missing_notice/package-lock.json"
expect fail "no notice file at all" "$missing_notice"

missing_lock="$(mktemp -d)"
mkdir -p "$missing_lock/LICENSES"
echo "# Third-party notices" > "$missing_lock/LICENSES/NOTICE-third-party.md"
expect fail "no lockfiles to compare against" "$missing_lock"

echo
echo "$pass passed, $fail failed"
[ "$fail" -eq 0 ]
