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
identifier="$(sed -n '/"identifier"/{s/.*"identifier"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/;p;q;}' "$ROOT/src-tauri/tauri.conf.json")"
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

# Every app.security.assetProtocol.scope entry must stay $APPDATA-relative.
# Replacing one with a literal path (e.g. a hardcoded absolute path) keeps
# the identifier check above green while reproducing the exact
# silent-blank-images failure this gate exists to prevent.
#
# Fail-closed all the way down. A missing assetProtocol block, a missing
# scope, and a scope that is never examined are three spellings of the same
# drift, so none of them may exit 0 -- this check used to skip itself in the
# last two cases.
conf="$ROOT/src-tauri/tauri.conf.json"

asset_protocol_line="$(sed -n '/"assetProtocol"/{=;q;}' "$conf")"
if [ -z "$asset_protocol_line" ]; then
  echo "check-identifier: no app.security.assetProtocol in src-tauri/tauri.conf.json" >&2
  exit 1
fi

# Find assetProtocol's own "scope", bounded at both ends.
#
# Anchoring at the assetProtocol line is what stops an unrelated *earlier* key
# -- a plugins.fs.scope, say -- from capturing the match and leaving the real
# scope unexamined. Tracking brace depth is what stops the mirror of that:
# a search running on to EOF would accept a *later* unrelated scope in place
# of assetProtocol's own missing one. Both spellings print OK for a scope the
# gate never read, which is the failure this whole script exists to prevent.
#
# Depth counting assumes braces do not appear inside string values, which is
# true of this config and is the price of staying free of node and jq.
scope_line_num=""
depth=0
offset=0
seen_open=0
while IFS= read -r line; do
  offset=$((offset + 1))

  if [ -z "$scope_line_num" ]; then
    case "$line" in
      *'"scope"'*) scope_line_num=$((asset_protocol_line + offset - 1)) ;;
    esac
  fi

  opens="${line//[!\{]/}"
  closes="${line//[!\}]/}"
  if [ ${#opens} -gt 0 ]; then
    seen_open=1
  fi
  depth=$((depth + ${#opens} - ${#closes}))

  # Left assetProtocol's object -- anything past here belongs to a sibling.
  if [ "$seen_open" -eq 1 ] && [ "$depth" -le 0 ]; then
    break
  fi
done < <(sed -n "${asset_protocol_line},\$p" "$conf")

if [ -z "$scope_line_num" ]; then
  echo "check-identifier: app.security.assetProtocol has no scope in src-tauri/tauri.conf.json" >&2
  exit 1
fi

scope_first_line="$(sed -n "${scope_line_num}p" "$conf")"
case "$scope_first_line" in
  # Whole array is on the "scope" line itself (the common case): a
  # start,/end/ sed range would otherwise skip re-checking the end
  # pattern on the very same line it started on and overrun into
  # unrelated JSON further down the file.
  *"]"*) scope_block="$scope_first_line" ;;
  *) scope_block="$(sed -n "${scope_line_num},/\]/p" "$conf")" ;;
esac

bad_entries=""
while IFS= read -r entry; do
  [ -z "$entry" ] && continue
  [ "$entry" = "scope" ] && continue
  case "$entry" in
    '$APPDATA/'*) ;;
    *) bad_entries="$bad_entries '$entry'" ;;
  esac
done < <(printf '%s\n' "$scope_block" | grep -o '"[^"]*"' | sed -e 's/^"//' -e 's/"$//')

if [ -n "$bad_entries" ]; then
  echo "check-identifier: assetProtocol.scope entries must start with \$APPDATA/, found:$bad_entries" >&2
  exit 1
fi

echo "assetProtocol.scope OK: all entries are \$APPDATA-relative"
