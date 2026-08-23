#!/usr/bin/env bash
# Verify the webview's connect-src, script-src and worker-src stay tight.
#
# The webview makes no network requests at all -- no fetch, XHR or WebSocket
# anywhere in src/. Every outbound request in this app is Rust's, through
# `reqwest`, which is not subject to the webview CSP (see CLAUDE.md). So a host
# in connect-src grants a capability nothing uses, and the reader pane renders
# imported third-party markup through `dangerouslySetInnerHTML` in
# MathText.tsx -- which is exactly the path an unused grant would widen.
#
# Nothing else reads this directive, so without this check a host added "just
# for now" passes every test and every build. Twelve of them accumulated that
# way before #64.
#
# Adding a Source does NOT require widening this. That work happens in Rust.
#
# script-src and worker-src are here for the same reason and with the same
# history. Both were widened for Kokoro, which ran ONNX inference in the webview
# via onnxruntime-web (ADR-0001): 'wasm-unsafe-eval' so it could compile the
# model, blob: so it could spawn its inference worker. ADR-0003 moved synthesis
# to Rust and the grants stayed. Verified against the built bundle, not just the
# sources: `dist/assets/*.js` contains no WebAssembly, Worker or .wasm
# reference at all.
#
# blob: in *media-src* is a different thing and is load-bearing -- playback and
# the chapter-export preview both play a Blob through URL.createObjectURL. Do
# not read this check as a reason to touch it.
#
# Usage: check-csp.sh
# Env:   ROOT  repo root (default: git rev-parse --show-toplevel)
set -euo pipefail

ROOT="${ROOT:-$(git rev-parse --show-toplevel)}"
conf="$ROOT/src-tauri/tauri.conf.json"

# Deliberately no Node/jq dependency: this runs in CI before the toolchain is
# set up, alongside the other cheap pre-toolchain checks.
[ -f "$conf" ] || { echo "check-csp: missing $conf" >&2; exit 1; }

csp="$(sed -n '/"csp"/{s/.*"csp"[[:space:]]*:[[:space:]]*"\(.*\)".*/\1/;p;q;}' "$conf")"

if [ -z "$csp" ]; then
  echo "check-csp: no 'csp' in src-tauri/tauri.conf.json -- has app.security moved?" >&2
  exit 1
fi

directive() { # $1 = name -> its value, or empty when absent
  printf '%s' "$csp" | tr ';' '\n' \
    | sed -n "s/^[[:space:]]*$1[[:space:]][[:space:]]*//p"
}

# Tokens in $1 that are not in the allowed list $2.
beyond() {
  local allowed=" $2 "
  local extra=""
  for token in $1; do
    case "$allowed" in *" $token "*) ;; *) extra="$extra $token";; esac
  done
  printf '%s' "$extra"
}

connect="$(directive connect-src)"

# Fail closed on absence too. Dropping the directive would leave connect-src
# falling back to default-src, which is tight today -- and would silently
# widen the moment anyone loosens default-src for an unrelated reason.
if [ -z "$connect" ]; then
  echo "check-csp: no connect-src directive in the CSP; it must stay explicit" >&2
  exit 1
fi

extra="$(beyond "$connect" "'self'")"

if [ -n "$extra" ]; then
  echo "connect-src grants hosts the webview never contacts:" >&2
  printf '  %s\n' $extra >&2
  echo >&2
  echo "The webview makes no network requests. Outbound HTTP is Rust's" >&2
  echo "(reqwest), which the CSP does not govern -- so this grants nothing" >&2
  echo "useful and widens what an injection through MathText could reach." >&2
  exit 1
fi

script="$(directive script-src)"

# Fail closed on absence for the same reason as connect-src: script-src would
# fall back to default-src, tight today, silently widened later.
if [ -z "$script" ]; then
  echo "check-csp: no script-src directive in the CSP; it must stay explicit" >&2
  exit 1
fi

extra="$(beyond "$script" "'self'")"

if [ -n "$extra" ]; then
  echo "script-src grants execution primitives the webview does not use:" >&2
  printf '  %s\n' $extra >&2
  echo >&2
  echo "The built bundle contains no WebAssembly, Worker or .wasm reference." >&2
  echo "Synthesis runs in Rust (ADR-0003); onnxruntime-web went with Kokoro." >&2
  echo "Each of these re-enables a code-execution path an injection through" >&2
  echo "dangerouslySetInnerHTML in MathText.tsx could otherwise not reach." >&2
  exit 1
fi

worker="$(directive worker-src)"

if [ -z "$worker" ]; then
  echo "check-csp: no worker-src directive in the CSP; it must stay explicit" >&2
  echo "Absent, it falls back through child-src to script-src -- which would" >&2
  echo "permit a same-origin worker rather than denying workers outright." >&2
  exit 1
fi

extra="$(beyond "$worker" "'none'")"

if [ -n "$extra" ]; then
  echo "worker-src grants more than the webview uses:" >&2
  printf '  %s\n' $extra >&2
  echo >&2
  echo "Nothing constructs a Worker. blob: was Kokoro's inference worker." >&2
  echo "blob: in media-src is unrelated and must stay -- playback and the" >&2
  echo "export preview both play a Blob through URL.createObjectURL." >&2
  exit 1
fi

echo "csp OK: connect-src 'self', script-src 'self', worker-src 'none'"
