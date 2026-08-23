#!/usr/bin/env bash
# Verify the webview's connect-src stays 'self' and nothing else.
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

connect="$(printf '%s' "$csp" | tr ';' '\n' \
  | sed -n 's/^[[:space:]]*connect-src[[:space:]][[:space:]]*//p')"

# Fail closed on absence too. Dropping the directive would leave connect-src
# falling back to default-src, which is tight today -- and would silently
# widen the moment anyone loosens default-src for an unrelated reason.
if [ -z "$connect" ]; then
  echo "check-csp: no connect-src directive in the CSP; it must stay explicit" >&2
  exit 1
fi

extra="$(printf '%s' "$connect" | tr ' ' '\n' | grep -v '^$' | grep -v "^'self'$" || true)"

if [ -n "$extra" ]; then
  echo "connect-src grants hosts the webview never contacts:" >&2
  printf '  %s\n' $extra >&2
  echo >&2
  echo "The webview makes no network requests. Outbound HTTP is Rust's" >&2
  echo "(reqwest), which the CSP does not govern -- so this grants nothing" >&2
  echo "useful and widens what an injection through MathText could reach." >&2
  exit 1
fi

echo "csp OK: connect-src is 'self' and nothing else"
