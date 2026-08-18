#!/usr/bin/env bash
# Verify the Source types Rust can store match the SourceType union in TypeScript.
#
# These two lists are twins with no compile-time link between them. Rust's
# `SourceType` serialises `rename_all = "lowercase"`, and the webview switches
# on the same strings -- but nothing makes adding a variant to one add it to the
# other. The Fish Audio provider defect was this exact shape: the frontend list
# gained a value the Rust list did not, and the write returned `Ok` while
# storing something else.
#
# `source_type_str` in content/document.rs is the source of truth, because it is
# an exhaustive match: the compiler already refuses a new variant without a
# string. This catches the other half -- a variant added in Rust but never
# mirrored on the frontend.
#
# It also checks `parse_source_type` in db/library.rs, which is the inverse and
# matches on `&str`, so the compiler does *not* refuse a new variant there. That
# gap shipped once: Pressbooks was added to the enum, to `source_type_str` and to
# the TypeScript union, and reading a Pressbooks row back then failed -- and
# because the parse runs inside `query_map`, it took the whole Library listing
# down rather than one row.
#
# The SQL `CHECK (source_type IN (...))` constraint is a fourth place naming the
# same set. It is not greppable across migrations, so the round-trip test in
# db/library.rs covers it instead: that test persists and lists one Document per
# variant and fails if any of the four is missed.
#
# Usage: check-source-types.sh
# Env:   ROOT  repo root (default: git rev-parse --show-toplevel)
set -euo pipefail

ROOT="${ROOT:-$(git rev-parse --show-toplevel)}"
rust_file="$ROOT/src-tauri/src/content/document.rs"
parse_file="$ROOT/src-tauri/src/db/library.rs"
ts_file="$ROOT/src/types/domain.ts"

for f in "$rust_file" "$ts_file"; do
  [ -f "$f" ] || { echo "check-source-types: missing $f" >&2; exit 1; }
done

# Match arms inside `fn source_type_str`, up to its closing brace.
rust_types="$(
  awk '/fn source_type_str\(/{f=1} f && /^}$/{exit} f' "$rust_file" \
    | sed -n 's/.*=> "\([a-z_]*\)".*/\1/p' | sort
)"

# String literals in the `SourceType` type alias.
ts_types="$(
  sed -n 's/^export type SourceType = \(.*\);$/\1/p' "$ts_file" \
    | grep -o '"[a-z_]*"' | tr -d '"' | sort
)"

if [ -z "$rust_types" ]; then
  echo "check-source-types: found no variants in $rust_file -- has source_type_str moved?" >&2
  exit 1
fi
if [ -z "$ts_types" ]; then
  echo "check-source-types: found no SourceType union in $ts_file" >&2
  exit 1
fi

# Match arms inside `fn parse_source_type`: `"x" => Ok(SourceType::X),`. Absent
# in a stripped test root, which is fine -- the check below skips it then.
if [ -f "$parse_file" ]; then
  parse_types="$(
    awk '/fn parse_source_type\(/{f=1} f && /^}$/{exit} f' "$parse_file" \
      | sed -n 's/.*"\([a-z_]*\)" *=> *Ok(.*/\1/p' | sort
  )"
  if [ -n "$parse_types" ] && [ "$rust_types" != "$parse_types" ]; then
    echo "Source types are out of sync inside Rust:" >&2
    diff <(echo "$rust_types") <(echo "$parse_types") \
      --label "src-tauri/src/content/document.rs (writes)" \
      --label "src-tauri/src/db/library.rs (reads back)" -u >&2 || true
    echo "" >&2
    echo "A Source that can be written but not read back fails the whole Library" >&2
    echo "listing, not just its own row." >&2
    exit 1
  fi
fi

if [ "$rust_types" != "$ts_types" ]; then
  echo "Source types are out of sync between Rust and TypeScript:" >&2
  diff <(echo "$rust_types") <(echo "$ts_types") \
    --label "src-tauri/src/content/document.rs" --label "src/types/domain.ts" -u >&2 || true
  exit 1
fi

echo "source types OK: $(echo "$rust_types" | wc -l | tr -d ' ') types match in document.rs and domain.ts"
