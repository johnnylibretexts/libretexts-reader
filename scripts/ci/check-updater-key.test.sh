#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
script="$here/check-updater-key.sh"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/src-tauri"

PLACEHOLDER="TAURI_UPDATER_PUBKEY_PLACEHOLDER"

# $1 is pasted into [dependencies] verbatim, so a case can write a commented
# line, a plain line, or nothing at all.
write_cargo() {
  cat > "$tmp/src-tauri/Cargo.toml" <<EOF
[package]
name = "libretexts-reader"

[dependencies]
tauri = { version = "2.1", features = ["protocol-asset"] }
$1
EOF
}

# $1 is pasted in as the body of the JSON object, so a case can write no
# plugins block, an empty updater, or one with any pubkey.
write_conf() {
  cat > "$tmp/src-tauri/tauri.conf.json" <<EOF
{
  "identifier": "dev.example.reader"$1
}
EOF
}

updater_config() {
  printf ',\n  "plugins": { "updater": { %s } }' "$1"
}

# Case 1: no updater dependency and no updater config. This is the shipped
# state -- the auto-updater is deliberately absent in v0.1.0 -- and it must not
# be treated as a failure, or every build fails from the day this lands.
write_cargo ""
write_conf ""
ROOT="$tmp" bash "$script" >/dev/null
echo "PASS: no updater at all accepted"

# Case 2: the dependency with a real key configured -> exit 0.
write_cargo 'tauri-plugin-updater = "2"'
write_conf "$(updater_config '"pubkey": "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk"')"
ROOT="$tmp" bash "$script" >/dev/null
echo "PASS: dependency with a real pubkey accepted"

# Case 3: the dependency with no updater config at all. This is the gap
# build.rs cannot close -- it returns early on a missing block, so its
# LIBRETEXTS_READER_REQUIRE_UPDATER_KEY belt never engages. Adding the plugin
# and forgetting the config ships an updater that verifies nothing.
write_cargo 'tauri-plugin-updater = "2"'
write_conf ""
if out="$(ROOT="$tmp" bash "$script" 2>&1)"; then
  echo "FAIL: dependency with no updater config not detected" >&2; exit 1
fi
# Asserted on the message, not just the exit code. The empty-pubkey check below
# rejects this case too, so a status-only assertion passes even with the
# missing-block branch deleted -- and the reader is then told the pubkey is
# absent when the whole block is.
case "$out" in
  *"no plugins.updater block"*) ;;
  *) echo "FAIL: a missing block was reported as a missing pubkey: $out" >&2; exit 1 ;;
esac
echo "PASS: dependency with no updater config rejected, and named as such"

# Case 4: the dependency with the placeholder still in place.
write_cargo 'tauri-plugin-updater = "2"'
write_conf "$(updater_config "\"pubkey\": \"$PLACEHOLDER\"")"
if ROOT="$tmp" bash "$script" >/dev/null 2>&1; then
  echo "FAIL: placeholder pubkey not detected" >&2; exit 1
fi
echo "PASS: placeholder pubkey rejected"

# Case 5: the dependency with an empty pubkey.
write_cargo 'tauri-plugin-updater = "2"'
write_conf "$(updater_config '"pubkey": ""')"
if ROOT="$tmp" bash "$script" >/dev/null 2>&1; then
  echo "FAIL: empty pubkey not detected" >&2; exit 1
fi
echo "PASS: empty pubkey rejected"

# Case 6: the dependency, an updater block, and no pubkey key at all.
write_cargo 'tauri-plugin-updater = "2"'
write_conf "$(updater_config '"endpoints": ["https://example.invalid/releases"]')"
if ROOT="$tmp" bash "$script" >/dev/null 2>&1; then
  echo "FAIL: absent pubkey key not detected" >&2; exit 1
fi
echo "PASS: absent pubkey key rejected"

# Case 7: a commented-out dependency is not a dependency. A grep for the crate
# name alone reads this as enabled and fails a build that has no updater --
# the exact false positive that gets a check like this deleted.
write_cargo '# tauri-plugin-updater = "2"  # reintroduce when a key exists'
write_conf ""
ROOT="$tmp" bash "$script" >/dev/null
echo "PASS: commented-out dependency does not count"

echo "ALL TESTS PASSED"
