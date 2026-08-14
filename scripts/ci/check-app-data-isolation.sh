#!/usr/bin/env bash
# Run the Rust test suite under a throwaway HOME and fail if it wrote anything there.
#
# paths.rs creates every directory it resolves (`create_dir_all`), so merely
# *asking* for a path materialises the real app-data tree. A test that forgets
# to keep away from paths:: therefore writes into
# `~/Library/Application Support/dev.johnnylibretexts.reader` and is
# indistinguishable from real usage on disk -- which is exactly why the original
# leak (issue #2) went unnoticed for so long. Nothing about it is visible in
# test output; the suite stays green either way.
#
# This wraps the suite rather than running alongside it, so CI does not pay for
# two test runs.
#
# Usage: check-app-data-isolation.sh [extra cargo args...]
# Env:   TEST_CMD  command to run under the sandboxed HOME
#                  (default: cargo test -p libretexts-reader)
set -euo pipefail

sandbox="$(mktemp -d)"
trap 'rm -rf "$sandbox"' EXIT

# Resolved against the real HOME before it is replaced: cargo and rustup keep
# their state there, and the point is to isolate app data, not the toolchain.
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"

# Every branch of paths::platform_app_data_dir keys off one of these.
export HOME="$sandbox"
export XDG_DATA_HOME="$sandbox/.local/share"
export APPDATA="$sandbox/AppData/Roaming"

status=0
eval "${TEST_CMD:-cargo test -p libretexts-reader $*}" || status=$?

if [ "$status" -ne 0 ]; then
  echo "check-app-data-isolation: test command failed (exit $status)" >&2
  exit "$status"
fi

leaked="$(find "$sandbox" -mindepth 1 2>/dev/null || true)"
if [ -n "$leaked" ]; then
  echo "check-app-data-isolation: the test suite wrote into the app-data directory." >&2
  echo "" >&2
  echo "$leaked" | sed "s|$sandbox|\$HOME|" >&2
  echo "" >&2
  echo "A test reached a paths:: helper, which calls create_dir_all on the path it" >&2
  echo "resolves. Pass the directory in explicitly instead -- see" >&2
  echo "cache::cache_path_in and cleanup::reclaim_in. Do not set" >&2
  echo "LIBRETEXTS_READER_APP_DATA_DIR: set_var is process-global and Rust runs" >&2
  echo "tests as threads in one process, so it can race another test." >&2
  exit 1
fi

echo "app-data isolation OK: the suite created nothing under \$HOME"
