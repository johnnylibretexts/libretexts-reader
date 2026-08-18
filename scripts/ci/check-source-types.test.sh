#!/usr/bin/env bash
# Tests for check-source-types.sh. Builds throwaway repo roots and checks that
# the script passes on matching input and fails on each way they can drift.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
subject="$script_dir/check-source-types.sh"
pass=0
fail=0

make_root() { # $1 = rust types (space separated), $2 = ts types (space separated)
  local root; root="$(mktemp -d)"
  mkdir -p "$root/src-tauri/src/content" "$root/src/types"

  {
    echo "fn source_type_str(source_type: SourceType) -> &'static str {"
    echo "    match source_type {"
    for t in $1; do echo "        SourceType::Placeholder => \"$t\","; done
    echo "    }"
    echo "}"
  } > "$root/src-tauri/src/content/document.rs"

  local union=""
  for t in $2; do
    [ -n "$union" ] && union="$union | "
    union="$union\"$t\""
  done
  echo "export type SourceType = $union;" > "$root/src/types/domain.ts"

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

expect pass "identical Source types on both sides" \
  "$(make_root "openstax libretexts pdf" "openstax libretexts pdf")"

expect pass "same types in a different order" \
  "$(make_root "pdf openstax libretexts" "libretexts pdf openstax")"

expect fail "type added in Rust but not TypeScript" \
  "$(make_root "openstax libretexts pressbooks" "openstax libretexts")"

# The shape of the Fish Audio defect: the frontend gained a value the Rust side
# never had, so the write returned Ok while storing something else.
expect fail "type added in TypeScript but not Rust" \
  "$(make_root "openstax libretexts" "openstax libretexts pressbooks")"

expect fail "type renamed on one side only" \
  "$(make_root "openstax pasted" "openstax pastedtext")"

# A missing or moved source should fail loudly rather than pass on empty input.
empty_root="$(mktemp -d)"; mkdir -p "$empty_root/src-tauri/src/content" "$empty_root/src/types"
echo "// no source_type_str fn here" > "$empty_root/src-tauri/src/content/document.rs"
echo "export type Nothing = string;" > "$empty_root/src/types/domain.ts"
expect fail "source_type_str missing entirely" "$empty_root"

echo
echo "$pass passed, $fail failed"
[ "$fail" -eq 0 ]
