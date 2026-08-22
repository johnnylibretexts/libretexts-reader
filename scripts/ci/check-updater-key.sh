#!/usr/bin/env bash
# Verify that if the Tauri updater plugin is a dependency, a real signing
# pubkey is actually configured for it.
#
# This closes the one case build.rs cannot. Its
# LIBRETEXTS_READER_REQUIRE_UPDATER_KEY belt inspects plugins.updater.pubkey and
# returns early when the whole updater block is absent -- correct, since without
# a block there is nothing to configure, but it means adding the plugin and
# forgetting the config sails straight through. That combination ships an
# updater that verifies nothing, which is the failure mode worth being loud
# about: a compromised update server can then hand every reader an arbitrary
# binary.
#
# Deliberately keyed on the dependency, not on the config. An updater block
# with no plugin behind it is inert; a plugin with no block is not.
#
# Usage: check-updater-key.sh
# Env:   ROOT  repo root (default: git rev-parse --show-toplevel)
set -euo pipefail

ROOT="${ROOT:-$(git rev-parse --show-toplevel)}"
cargo_toml="$ROOT/src-tauri/Cargo.toml"
conf="$ROOT/src-tauri/tauri.conf.json"

# Kept in step with UPDATER_PUBKEY_PLACEHOLDER in src-tauri/build.rs.
PLACEHOLDER="TAURI_UPDATER_PUBKEY_PLACEHOLDER"

for f in "$cargo_toml" "$conf"; do
  [ -f "$f" ] || { echo "check-updater-key: missing $f" >&2; exit 1; }
done

# A dependency line, not a mention. Anchored at the start of the line so a
# commented-out entry -- the natural way to park this until a key exists --
# does not read as enabled and fail a build that has no updater at all.
# Deliberately no Node/jq dependency: this runs in CI before the toolchain is
# set up, alongside the other cheap pre-toolchain checks.
if ! grep -qE '^[[:space:]]*tauri-plugin-updater[[:space:]]*=' "$cargo_toml"; then
  echo "check-updater-key OK: the updater plugin is not a dependency, nothing to verify"
  exit 0
fi

# Everything from "updater" to the end of its object, so a pubkey belonging to
# some other plugin cannot stand in for a missing one.
updater_block="$(tr -d '\n' < "$conf" | sed -n 's/.*"updater"[[:space:]]*:[[:space:]]*{\([^}]*\)}.*/\1/p')"

if [ -z "$updater_block" ]; then
  echo "check-updater-key: tauri-plugin-updater is a dependency but src-tauri/tauri.conf.json has no plugins.updater block." >&2
  echo "  An updater with no configured pubkey verifies nothing. See RELEASE.md." >&2
  exit 1
fi

pubkey="$(printf '%s' "$updater_block" | sed -n 's/.*"pubkey"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')"

if [ -z "$pubkey" ]; then
  echo "check-updater-key: plugins.updater has no pubkey." >&2
  echo "  Generate one with \`npm run tauri -- signer generate\`. See RELEASE.md." >&2
  exit 1
fi

if [ "$pubkey" = "$PLACEHOLDER" ]; then
  echo "check-updater-key: plugins.updater.pubkey is still the placeholder ($PLACEHOLDER)." >&2
  echo "  Generate a real key with \`npm run tauri -- signer generate\`. See RELEASE.md." >&2
  exit 1
fi

echo "check-updater-key OK: the updater plugin has a configured pubkey"
