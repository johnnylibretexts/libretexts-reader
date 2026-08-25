#!/usr/bin/env bash
# Regenerate LICENSES/NOTICE-third-party.md from the two lockfiles.
#
# Run this whenever a dependency is added, removed or bumped, and commit the
# result. `scripts/ci/check-notices.sh` fails the build when the file no longer
# matches the lockfiles it was generated from.
#
# Deliberately NOT part of `tauri:build`. Doing it there would make every
# release depend on `cargo-about` being installed on the machine doing the
# building -- including the single-use release runner, where a missing tool
# fails the build late, cryptically, and at the worst possible moment. The
# freshness check costs nothing and catches the same drift.
#
# Requires: cargo-about (`cargo install cargo-about --locked --features cli`)
#           and an installed node_modules.
# Usage:    scripts/generate-notices.sh
# Env:      ROOT  repo root (default: git rev-parse --show-toplevel)
set -euo pipefail

ROOT="${ROOT:-$(git rev-parse --show-toplevel)}"
output="$ROOT/LICENSES/NOTICE-third-party.md"

if ! cargo about --version >/dev/null 2>&1; then
  echo "generate-notices: cargo-about is not installed." >&2
  echo "  cargo install cargo-about --locked --features cli" >&2
  echo "  (without --features cli the install exits 0 and installs nothing)" >&2
  exit 1
fi

if [ ! -d "$ROOT/node_modules" ]; then
  echo "generate-notices: no node_modules -- run npm install first" >&2
  exit 1
fi

rust_notices="$(cd "$ROOT/src-tauri" && cargo about generate about.hbs)"
npm_notices="$(cd "$ROOT" && node scripts/npm-notices.mjs)"

# The fingerprint the freshness check compares against. Both lockfiles, because
# either can change what ships.
#
# Hashed *after* the generators run, not before. `cargo about` shells out to
# `cargo metadata`, which rewrites Cargo.lock when it is stale against
# Cargo.toml -- exactly the state this script exists to be run in, since you
# run it after bumping a dependency. Hashing first recorded the pre-update
# Cargo.lock, so check-notices.sh then failed on the file this script had just
# generated, with nothing to show for the difference.
cargo_lock_hash="$(shasum -a 256 "$ROOT/Cargo.lock" | cut -d' ' -f1)"
npm_lock_hash="$(shasum -a 256 "$ROOT/package-lock.json" | cut -d' ' -f1)"

mkdir -p "$ROOT/LICENSES"
cat > "$output" <<HEADER
# Third-party notices

LibreTexts Reader is distributed with the components below. Several are under
licences that require their copyright notice be retained in binary
distributions; this file is how that obligation is met, and it ships inside the
\`.app\` (see \`build.rs\`, which mirrors this directory into the bundle).

**Generated — do not edit by hand.** Run \`scripts/generate-notices.sh\` after
changing a dependency, and commit the result.

The bundled native components (PDFium, and the \`id3\` and \`mp4ameta\` crates)
carry their own full notices as separate files in this directory.

The Supertonic voice model and M2M100 translation model are **not** covered
here: they are downloaded on the reader's own machine rather than distributed
with the app, and their terms are recorded in \`supertonic-model.md\` and
\`translation-models.md\`.

<!-- cargo-lock-sha256: $cargo_lock_hash -->
<!-- npm-lock-sha256: $npm_lock_hash -->

HEADER

# The template has no equality helper, so the singular is fixed here rather
# than by teaching handlebars to count.
printf '%s\n\n%s\n' "$rust_notices" "$npm_notices" \
  | sed 's/^- \*\*\(.*\)\*\* — 1 components$/- **\1** — 1 component/' >> "$output"

echo "wrote $output ($(wc -l < "$output" | tr -d ' ') lines)"
