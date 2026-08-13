#!/usr/bin/env bash
# Tests for check-error-kinds.sh. Builds throwaway repo roots and checks that
# the script passes on matching input and fails on each way they can drift.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
subject="$script_dir/check-error-kinds.sh"
pass=0
fail=0

make_root() { # $1 = rust kinds (space separated), $2 = ts kinds (space separated)
  local root; root="$(mktemp -d)"
  mkdir -p "$root/src-tauri/src" "$root/src/types"

  {
    echo "impl AppError {"
    echo "    pub fn kind(&self) -> &'static str {"
    echo "        match self {"
    for k in $1; do echo "            Self::Placeholder(_) => \"$k\","; done
    echo "        }"
    echo "    }"
    echo "}"
  } > "$root/src-tauri/src/error.rs"

  local union=""
  for k in $2; do
    [ -n "$union" ] && union="$union | "
    union="$union\"$k\""
  done
  echo "export type AppErrorKind = $union;" > "$root/src/types/domain.ts"

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

expect pass "identical kinds on both sides" \
  "$(make_root "database io tts" "database io tts")"

expect pass "same kinds in a different order" \
  "$(make_root "tts database io" "io tts database")"

expect fail "kind added in Rust but not TypeScript" \
  "$(make_root "database io tts drm_protected" "database io tts")"

expect fail "kind added in TypeScript but not Rust" \
  "$(make_root "database io" "database io tts")"

expect fail "kind renamed on one side only" \
  "$(make_root "database io invalid_input" "database io invalidinput")"

# A missing or moved source should fail loudly rather than pass on empty input.
empty_root="$(mktemp -d)"; mkdir -p "$empty_root/src-tauri/src" "$empty_root/src/types"
echo "// no kind fn here" > "$empty_root/src-tauri/src/error.rs"
echo "export type Nothing = string;" > "$empty_root/src/types/domain.ts"
expect fail "AppError::kind missing entirely" "$empty_root"

echo
echo "$pass passed, $fail failed"
[ "$fail" -eq 0 ]
