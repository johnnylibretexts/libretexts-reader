#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
script="$here/check-identifier.sh"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/src-tauri/src"

write_conf() { printf '{ "identifier": "%s" }\n' "$1" > "$tmp/src-tauri/tauri.conf.json"; }
write_paths() { printf 'const APP_DIR_NAME: &str = "%s";\n' "$1" > "$tmp/src-tauri/src/paths.rs"; }

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

# Case 1: identifier and APP_DIR_NAME agree -> exit 0
write_conf dev.example.reader
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

echo "ALL TESTS PASSED"
