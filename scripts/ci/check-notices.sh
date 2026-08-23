#!/usr/bin/env bash
# Verify the third-party notices still describe the dependencies that ship.
#
# `LICENSES/NOTICE-third-party.md` is generated from the two lockfiles and
# records their hashes in its header. If either lockfile has moved since, the
# notice is describing a build that no longer exists -- an attribution file that
# is quietly wrong is worse than none, because it looks like the obligation was
# met.
#
# Deliberately hash-based rather than regenerating and diffing: regenerating
# needs cargo-about and an installed node_modules, and this runs in CI before
# any toolchain is set up, alongside the other cheap checks. The hashes catch
# the drift that matters -- dependencies changed, notices not regenerated --
# without needing either.
#
# Usage: check-notices.sh
# Env:   ROOT  repo root (default: git rev-parse --show-toplevel)
set -euo pipefail

ROOT="${ROOT:-$(git rev-parse --show-toplevel)}"
notice="$ROOT/LICENSES/NOTICE-third-party.md"

[ -f "$notice" ] || {
  echo "check-notices: missing $notice -- run scripts/generate-notices.sh" >&2
  exit 1
}

for lock in "$ROOT/Cargo.lock" "$ROOT/package-lock.json"; do
  [ -f "$lock" ] || { echo "check-notices: missing $lock" >&2; exit 1; }
done

recorded_cargo="$(sed -n 's/^<!-- cargo-lock-sha256: \([0-9a-f]*\) -->$/\1/p' "$notice")"
recorded_npm="$(sed -n 's/^<!-- npm-lock-sha256: \([0-9a-f]*\) -->$/\1/p' "$notice")"

# Fail closed: a notice with no fingerprint cannot be checked at all, and
# treating "nothing to compare" as "nothing has changed" is how this gate would
# silently stop gating.
if [ -z "$recorded_cargo" ] || [ -z "$recorded_npm" ]; then
  echo "check-notices: $notice carries no lockfile fingerprint." >&2
  echo "Regenerate it with scripts/generate-notices.sh." >&2
  exit 1
fi

actual_cargo="$(shasum -a 256 "$ROOT/Cargo.lock" | cut -d' ' -f1)"
actual_npm="$(shasum -a 256 "$ROOT/package-lock.json" | cut -d' ' -f1)"

stale=""
[ "$recorded_cargo" = "$actual_cargo" ] || stale="Cargo.lock"
[ "$recorded_npm" = "$actual_npm" ] || stale="${stale:+$stale and }package-lock.json"

if [ -n "$stale" ]; then
  echo "third-party notices are stale: $stale changed since they were generated." >&2
  echo >&2
  echo "  scripts/generate-notices.sh" >&2
  echo >&2
  echo "Several bundled components require their copyright notice be retained" >&2
  echo "in binary distributions, so this file is a licence obligation, not" >&2
  echo "documentation." >&2
  exit 1
fi

echo "third-party notices OK: generated from the current Cargo.lock and package-lock.json"
