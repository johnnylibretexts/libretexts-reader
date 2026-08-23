#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
script="$here/check-identifier.sh"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/src-tauri/src"
printf 'pub const KEYCHAIN_SERVICE: &str = "dev.example.reader";\n' > "$tmp/src-tauri/src/secrets.rs"

write_conf() { printf '{ "identifier": "%s" }\n' "$1" > "$tmp/src-tauri/tauri.conf.json"; }
write_paths() { printf 'const APP_DIR_NAME: &str = "%s";\n' "$1" > "$tmp/src-tauri/src/paths.rs"; }
write_secrets() { printf 'pub const KEYCHAIN_SERVICE: &str = "%s";\n' "$1" > "$tmp/src-tauri/src/secrets.rs"; }

# Writes a conf with a real assetProtocol.scope array so the scope check has
# something to inspect. $2 is the raw (already-quoted) array contents, e.g.
# '"$APPDATA/covers/**", "$APPDATA/images/**"'.
write_conf_with_scope() {
  cat > "$tmp/src-tauri/tauri.conf.json" <<EOF
{
  "identifier": "$1",
  "app": {
    "security": {
      "assetProtocol": {
        "enable": true,
        "scope": [$2]
      }
    }
  }
}
EOF
}

# Writes a conf carrying an unrelated "scope" key *before* assetProtocol, to
# prove the scope check anchors on assetProtocol rather than on the first
# "scope" in the file. The decoy is deliberately $APPDATA-relative so it would
# pass on its own -- $2 is what must actually be judged.
write_conf_with_decoy_scope() {
  cat > "$tmp/src-tauri/tauri.conf.json" <<EOF
{
  "identifier": "$1",
  "plugins": {
    "fs": {
      "scope": ["\$APPDATA/decoy/**"]
    }
  },
  "app": {
    "security": {
      "assetProtocol": {
        "enable": true,
        "scope": [$2]
      }
    }
  }
}
EOF
}

# Writes a conf whose assetProtocol has no scope, with an unrelated scope
# *after* it. The mirror of the decoy case: anchoring the search at
# assetProtocol is not enough on its own, because a search running to EOF
# validates this later scope in place of the missing real one.
write_conf_without_scope_then_decoy() {
  cat > "$tmp/src-tauri/tauri.conf.json" <<EOF
{
  "identifier": "$1",
  "app": {
    "security": {
      "assetProtocol": {
        "enable": true
      }
    }
  },
  "plugins": {
    "fs": {
      "scope": ["\$APPDATA/decoy/**"]
    }
  }
}
EOF
}

# Writes a conf with an assetProtocol block that has no "scope" key at all.
write_conf_without_scope() {
  cat > "$tmp/src-tauri/tauri.conf.json" <<EOF
{
  "identifier": "$1",
  "app": {
    "security": {
      "assetProtocol": {
        "enable": true
      }
    }
  }
}
EOF
}

# Case 1: identifier and APP_DIR_NAME agree -> exit 0
# Carries a valid scope block because the scope check is unconditional: an
# identifier-only conf is itself drift now, covered by case 9.
write_conf_with_scope dev.example.reader '"$APPDATA/covers/**", "$APPDATA/images/**"'
write_paths dev.example.reader
ROOT="$tmp" bash "$script" >/dev/null
echo "PASS: matching identifier accepted"

# Case 2: they disagree -> non-zero
write_paths dev.other.reader
if ROOT="$tmp" bash "$script" >/dev/null 2>&1; then
  echo "FAIL: mismatch not detected" >&2; exit 1
fi
echo "PASS: mismatched identifier rejected"

# Case 3: identifier missing from tauri.conf.json -> non-zero
printf '{ "productName": "X" }\n' > "$tmp/src-tauri/tauri.conf.json"
write_paths dev.example.reader
if ROOT="$tmp" bash "$script" >/dev/null 2>&1; then
  echo "FAIL: missing identifier not detected" >&2; exit 1
fi
echo "PASS: missing identifier rejected"

# Case 4: APP_DIR_NAME missing from paths.rs -> non-zero
write_conf dev.example.reader
printf 'const SOMETHING_ELSE: &str = "x";\n' > "$tmp/src-tauri/src/paths.rs"
if ROOT="$tmp" bash "$script" >/dev/null 2>&1; then
  echo "FAIL: missing APP_DIR_NAME not detected" >&2; exit 1
fi
echo "PASS: missing APP_DIR_NAME rejected"

# Case 5: assetProtocol.scope entries are all $APPDATA-relative -> exit 0
write_conf_with_scope dev.example.reader '"$APPDATA/covers/**", "$APPDATA/images/**"'
write_paths dev.example.reader
ROOT="$tmp" bash "$script" >/dev/null
echo "PASS: \$APPDATA-relative scope accepted"

# Case 6: a literal (non-$APPDATA) scope entry -> non-zero
write_conf_with_scope dev.example.reader '"$APPDATA/covers/**", "/Users/someone/Library/Application Support/dev.example.reader/images/**"'
write_paths dev.example.reader
if ROOT="$tmp" bash "$script" >/dev/null 2>&1; then
  echo "FAIL: literal-path scope entry not detected" >&2; exit 1
fi
echo "PASS: literal-path scope entry rejected"

# Case 7: an earlier unrelated "scope" must not capture the match. The decoy is
# $APPDATA-relative and the real assetProtocol.scope is fully literal, so a
# check that reads the first "scope" in the file reports OK and exits 0.
write_conf_with_decoy_scope dev.example.reader '"/Users/someone/Library/Application Support/dev.example.reader/covers/**", "/etc/**"'
write_paths dev.example.reader
if ROOT="$tmp" bash "$script" >/dev/null 2>&1; then
  echo "FAIL: decoy 'scope' before assetProtocol captured the match" >&2; exit 1
fi
echo "PASS: decoy 'scope' before assetProtocol does not capture the match"

# Case 8: deleting the scope key is itself the drift that blanks every image,
# so an assetProtocol block without one must fail rather than skip the check.
write_conf_without_scope dev.example.reader
write_paths dev.example.reader
if ROOT="$tmp" bash "$script" >/dev/null 2>&1; then
  echo "FAIL: missing assetProtocol.scope not detected" >&2; exit 1
fi
echo "PASS: missing assetProtocol.scope rejected"

# Case 9: and neither may the whole assetProtocol block go missing.
write_conf dev.example.reader
write_paths dev.example.reader
if ROOT="$tmp" bash "$script" >/dev/null 2>&1; then
  echo "FAIL: missing assetProtocol block not detected" >&2; exit 1
fi
echo "PASS: missing assetProtocol block rejected"

# Case 10: a scope belonging to a *later* unrelated block must not stand in for
# assetProtocol's own missing one. Anchoring the search at assetProtocol closes
# case 7 but opens this mirror of it unless the search also stops at the end of
# the assetProtocol object.
write_conf_without_scope_then_decoy dev.example.reader
write_paths dev.example.reader
if ROOT="$tmp" bash "$script" >/dev/null 2>&1; then
  echo "FAIL: later unrelated 'scope' stood in for the missing assetProtocol one" >&2; exit 1
fi
echo "PASS: later unrelated 'scope' does not stand in for a missing one"

# Case 10: KEYCHAIN_SERVICE agrees with the identifier -> exit 0
write_conf_with_scope dev.example.reader '"$APPDATA/covers/**", "$APPDATA/images/**"'
write_paths dev.example.reader
write_secrets dev.example.reader
ROOT="$tmp" bash "$script" >/dev/null
echo "PASS: matching KEYCHAIN_SERVICE accepted"

# Case 11: KEYCHAIN_SERVICE left behind when the identifier moves -> non-zero.
# This is the drift with no visible symptom: the app still builds, still runs,
# and still renders every image -- it just looks for the reader's Fish Audio
# key under a service name nothing writes any more, so the key silently
# vanishes and they are asked to enter it again.
write_secrets dev.old.reader
if ROOT="$tmp" bash "$script" >/dev/null 2>&1; then
  echo "FAIL: stale KEYCHAIN_SERVICE not detected" >&2; exit 1
fi
echo "PASS: stale KEYCHAIN_SERVICE rejected"

# Case 12: KEYCHAIN_SERVICE missing from secrets.rs -> non-zero
printf 'pub const SOMETHING_ELSE: &str = "x";\n' > "$tmp/src-tauri/src/secrets.rs"
if ROOT="$tmp" bash "$script" >/dev/null 2>&1; then
  echo "FAIL: missing KEYCHAIN_SERVICE not detected" >&2; exit 1
fi
echo "PASS: missing KEYCHAIN_SERVICE rejected"

echo "ALL TESTS PASSED"
