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
# Usage: check-source-types.sh
# Env:   ROOT  repo root (default: git rev-parse --show-toplevel)
set -euo pipefail

ROOT="${ROOT:-$(git rev-parse --show-toplevel)}"
rust_file="$ROOT/src-tauri/src/content/document.rs"
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

if [ "$rust_types" != "$ts_types" ]; then
  echo "Source types are out of sync between Rust and TypeScript:" >&2
  diff <(echo "$rust_types") <(echo "$ts_types") \
    --label "src-tauri/src/content/document.rs" --label "src/types/domain.ts" -u >&2 || true
  exit 1
fi

echo "source types OK: $(echo "$rust_types" | wc -l | tr -d ' ') types match in document.rs and domain.ts"
