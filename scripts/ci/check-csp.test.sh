#!/usr/bin/env bash
# Tests for check-csp.sh. Builds throwaway repo roots and checks that the
# script passes on a connect-src of 'self' alone and fails on every way a host
# can creep back in.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
subject="$script_dir/check-csp.sh"
pass=0
fail=0

make_root() { # $1 = the whole csp string
  # A tight script-src/worker-src is appended unless $1 names one, so a case
  # written to exercise connect-src parsing stays about connect-src instead of
  # failing on a directive it never meant to test.
  local csp="$1"
  case "$csp" in *script-src*) ;; *) csp="$csp; script-src 'self'";; esac
  case "$csp" in *worker-src*) ;; *) csp="$csp; worker-src 'none'";; esac
  set -- "$csp"
  local root; root="$(mktemp -d)"
  mkdir -p "$root/src-tauri"
  cat > "$root/src-tauri/tauri.conf.json" <<JSON
{
  "app": {
    "security": {
      "csp": "$1",
      "assetProtocol": { "enable": true, "scope": ["\$APPDATA/covers/**"] }
    }
  }
}
JSON
  echo "$root"
}

# The CSP written verbatim, with nothing appended. The absence cases need it:
# make_root would helpfully add back the very directive they remove.
make_root_exact() { # $1 = the whole csp string
  local root; root="$(mktemp -d)"
  mkdir -p "$root/src-tauri"
  cat > "$root/src-tauri/tauri.conf.json" <<JSON
{
  "app": {
    "security": {
      "csp": "$1",
      "assetProtocol": { "enable": true, "scope": ["\$APPDATA/covers/**"] }
    }
  }
}
JSON
  echo "$root"
}

expect() { # $1 = "pass"|"fail", $2 = description, $3 = root
  local status=0
  ROOT="$3" "$subject" >/dev/null 2>&1 || status=$?
  if { [ "$1" = "pass" ] && [ "$status" -eq 0 ]; } || { [ "$1" = "fail" ] && [ "$status" -ne 0 ]; }; then
    echo "ok: $2"; pass=$((pass + 1))
  else
    echo "FAILED: $2 (exit $status, expected $1)" >&2; fail=$((fail + 1))
  fi
  rm -rf "$3"
}

expect pass "connect-src of 'self' alone" \
  "$(make_root "default-src 'self'; connect-src 'self'; img-src 'self' data:")"

expect pass "connect-src last in the policy, with no trailing semicolon" \
  "$(make_root "default-src 'self'; connect-src 'self'")"

expect pass "extra whitespace around the directive" \
  "$(make_root "default-src 'self';   connect-src   'self'  ; img-src 'self'")"

expect fail "one host added beside 'self'" \
  "$(make_root "default-src 'self'; connect-src 'self' https://example.test")"

expect fail "a wildcard host added beside 'self'" \
  "$(make_root "default-src 'self'; connect-src 'self' https://*.example.test")"

expect fail "a host in place of 'self'" \
  "$(make_root "default-src 'self'; connect-src https://example.test")"

# A host in img-src is fine -- the catalog browsers render publisher cover
# thumbnails straight from their URLs, so those grants are load-bearing. Only
# connect-src is in scope, and the check must not fail on its neighbours.
expect pass "hosts in img-src while connect-src stays tight" \
  "$(make_root "default-src 'self'; img-src 'self' https://openstax.org https://*.libretexts.org; connect-src 'self'")"

# Fail-closed: a directive that vanished must not read as a tight one. It would
# fall back to default-src, which is tight today and would widen silently the
# moment default-src is loosened for something unrelated.
expect fail "connect-src dropped from the policy entirely" \
  "$(make_root "default-src 'self'; img-src 'self' data:")"

# script-src. 'wasm-unsafe-eval' dates from Kokoro running ONNX in the webview;
# the shipped bundle has contained no WebAssembly, Worker or .wasm reference
# since Supertonic moved inference to Rust.
expect fail "script-src still granting 'wasm-unsafe-eval'" \
  "$(make_root "default-src 'self'; connect-src 'self'; script-src 'self' 'wasm-unsafe-eval'; worker-src 'none'")"

expect fail "script-src granting 'unsafe-eval'" \
  "$(make_root "default-src 'self'; connect-src 'self'; script-src 'self' 'unsafe-eval'; worker-src 'none'")"

expect fail "script-src granting 'unsafe-inline'" \
  "$(make_root "default-src 'self'; connect-src 'self'; script-src 'self' 'unsafe-inline'; worker-src 'none'")"

expect fail "script-src dropped from the policy entirely" \
  "$(make_root_exact "default-src 'self'; connect-src 'self'; worker-src 'none'")"

# worker-src. Nothing constructs a Worker, so 'none' is what the webview uses;
# blob: was the Kokoro inference worker's grant.
expect fail "worker-src still granting blob:" \
  "$(make_root "default-src 'self'; connect-src 'self'; script-src 'self'; worker-src 'self' blob:")"

expect fail "worker-src dropped from the policy entirely" \
  "$(make_root_exact "default-src 'self'; connect-src 'self'; script-src 'self'")"

# blob: in media-src is load-bearing and must not be confused for the worker
# grant: playback and the export preview both play a Blob through
# URL.createObjectURL.
expect pass "media-src keeps blob: while worker-src is 'none'" \
  "$(make_root "default-src 'self'; connect-src 'self'; script-src 'self'; worker-src 'none'; media-src 'self' blob:")"

# The other two ways the check can be looking at nothing at all.
no_csp_root="$(mktemp -d)"; mkdir -p "$no_csp_root/src-tauri"
echo '{ "app": { "security": { "assetProtocol": { "enable": true } } } }' \
  > "$no_csp_root/src-tauri/tauri.conf.json"
expect fail "no csp key in the config" "$no_csp_root"

missing_root="$(mktemp -d)"
expect fail "no tauri.conf.json at all" "$missing_root"

echo
echo "$pass passed, $fail failed"
[ "$fail" -eq 0 ]
