#!/usr/bin/env bash
# Verify the bundled ffmpeg sidecar can actually load its shared libraries.
#
# build.rs extracts ffmpeg into binaries/ffmpeg-<target> plus a sibling
# -libs directory. On the tar path (Linux) it long skipped every non-regular
# entry, and ffmpeg's lib/ is mostly symlinks -- 14 of them against 7 real
# files -- so the SONAME links the loader actually asks for
# (libavdevice.so.63 -> libavdevice.so.63.2.100) were dropped and the sidecar
# could not resolve anything. Nothing else catches this: no Rust source
# references ffmpeg, so no test executes it, and a build that never runs the
# binary is perfectly happy with an unloadable one.
#
# Deliberately fail-closed. A missing sidecar is a failure, not a skip -- a
# check that quietly passes when its subject is absent is the same defect
# check-identifier.sh was hardened against.
#
# Usage: check-ffmpeg-bundle.sh
# Env:   ROOT           repo root (default: git rev-parse --show-toplevel)
#        FFMPEG_TARGET  rust target triple (default: <arch>-unknown-linux-gnu)
set -euo pipefail

ROOT="${ROOT:-$(git rev-parse --show-toplevel)}"
target="${FFMPEG_TARGET:-$(uname -m)-unknown-linux-gnu}"

binaries="$ROOT/src-tauri/binaries"
sidecar="$binaries/ffmpeg-$target"
libs="$binaries/ffmpeg-$target-libs"

if [ ! -f "$sidecar" ]; then
  echo "check-ffmpeg-bundle: no ffmpeg sidecar at $sidecar" >&2
  echo "  (build.rs must have run for $target before this check)" >&2
  exit 1
fi

if [ ! -d "$libs" ]; then
  echo "check-ffmpeg-bundle: no shared-library directory at $libs" >&2
  exit 1
fi

# The real assertion: ask the dynamic loader, rather than counting files.
# Every DT_NEEDED entry must resolve against the extracted lib directory, which
# is what a dropped SONAME symlink breaks and what a file count would miss.
unresolved="$(LD_LIBRARY_PATH="$libs" ldd "$sidecar" 2>/dev/null | grep 'not found' || true)"

if [ -n "$unresolved" ]; then
  echo "check-ffmpeg-bundle: the bundled ffmpeg cannot resolve its libraries:" >&2
  printf '%s\n' "$unresolved" >&2
  echo "  extracted into $libs" >&2
  exit 1
fi

echo "check-ffmpeg-bundle: sidecar resolves every shared library it needs"
