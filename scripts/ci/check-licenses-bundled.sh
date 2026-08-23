#!/usr/bin/env bash
# Verify the third-party licence notices are actually inside the built .app.
#
# `build.rs` writes the notices to <repo>/LICENSES, but `bundle.resources` in
# tauri.conf.json resolves relative to src-tauri/ -- so for the whole life of
# the project the shipped bundle carried LGPL and PDFium binaries with none of
# their licence texts. Reading build.rs does not reveal that; only looking in
# the artifact does, which is what this checks.
#
# Deliberately fail-closed: a missing .app is
# a failure, not a skip. A check that quietly passes when its subject is absent
# is the defect check-identifier.sh was hardened against.
#
# Usage: check-licenses-bundled.sh
# Env:   ROOT      repo root (default: git rev-parse --show-toplevel)
#        APP_PATH  built bundle (default: target/release/bundle/macos/*.app)
set -euo pipefail

ROOT="${ROOT:-$(git rev-parse --show-toplevel)}"

if [ -n "${APP_PATH:-}" ]; then
  app="$APP_PATH"
else
  app="$(find "$ROOT/target/release/bundle/macos" -maxdepth 1 -name '*.app' 2>/dev/null | head -1)"
fi

if [ -z "$app" ] || [ ! -d "$app" ]; then
  echo "check-licenses-bundled: no .app found (looked in target/release/bundle/macos)" >&2
  exit 1
fi

# Located rather than hardcoded. Tauri's `resources/**/*` glob preserves the
# leading path segment, so the notices land at Contents/Resources/resources/
# LICENSES, not Contents/Resources/LICENSES -- a detail visible only in a built
# bundle, and one this check itself got wrong first time round.
licenses_dir="$(find "$app/Contents/Resources" -type d -name LICENSES 2>/dev/null | head -1)"
if [ -z "$licenses_dir" ]; then
  echo "check-licenses-bundled: no LICENSES directory anywhere in $app/Contents/Resources" >&2
  echo "  the bundle ships third-party binaries with no licence notices" >&2
  exit 1
fi

missing=0
for required in pdfium.txt pdfium-binaries-license.txt; do
  if [ ! -s "$licenses_dir/$required" ]; then
    echo "check-licenses-bundled: $required missing or empty in the bundle" >&2
    missing=$((missing + 1))
  fi
done

if [ "$missing" -ne 0 ]; then
  exit 1
fi

echo "check-licenses-bundled OK: $(ls "$licenses_dir" | wc -l | tr -d ' ') notice(s) in the bundle"
