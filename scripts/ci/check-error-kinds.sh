#!/usr/bin/env bash
# Verify the error kinds Rust can emit match the AppErrorKind union in TypeScript.
#
# The Rust side is the source of truth: `AppError::kind` is an exhaustive match,
# so the compiler already refuses a new variant without a tag. This catches the
# other half -- a tag added in Rust but never mirrored on the frontend.
#
# Usage: check-error-kinds.sh
# Env:   ROOT  repo root (default: git rev-parse --show-toplevel)
set -euo pipefail

ROOT="${ROOT:-$(git rev-parse --show-toplevel)}"
rust_file="$ROOT/src-tauri/src/error.rs"
ts_file="$ROOT/src/types/domain.ts"

for f in "$rust_file" "$ts_file"; do
  [ -f "$f" ] || { echo "check-error-kinds: missing $f" >&2; exit 1; }
done

# Match arms inside `pub fn kind`, up to its closing brace: `Self::X(_) => "tag",`
rust_kinds="$(
  awk '/pub fn kind\(/{f=1} f && /^    }$/{exit} f' "$rust_file" \
    | sed -n 's/.*=> "\([a-z_]*\)".*/\1/p' | sort
)"

# String literals in the `AppErrorKind` type alias.
ts_kinds="$(
  sed -n 's/^export type AppErrorKind = \(.*\);$/\1/p' "$ts_file" \
    | grep -o '"[a-z_]*"' | tr -d '"' | sort
)"

if [ -z "$rust_kinds" ]; then
  echo "check-error-kinds: found no kinds in $rust_file -- has AppError::kind moved?" >&2
  exit 1
fi
if [ -z "$ts_kinds" ]; then
  echo "check-error-kinds: found no AppErrorKind union in $ts_file" >&2
  exit 1
fi

if [ "$rust_kinds" != "$ts_kinds" ]; then
  echo "error kinds are out of sync between Rust and TypeScript:" >&2
  diff <(echo "$rust_kinds") <(echo "$ts_kinds") \
    --label "src-tauri/src/error.rs" --label "src/types/domain.ts" -u >&2 || true
  exit 1
fi

echo "error kinds OK: $(echo "$rust_kinds" | wc -l | tr -d ' ') kinds match in error.rs and domain.ts"
