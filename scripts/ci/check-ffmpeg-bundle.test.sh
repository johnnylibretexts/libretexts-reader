#!/usr/bin/env bash
# Tests check-ffmpeg-bundle.sh's decision logic without needing a real ELF
# binary, by putting a stub `ldd` first on PATH. That keeps this runnable on a
# macOS dev machine, where there is no Linux sidecar and no system ldd at all;
# the real proof that the bundle loads runs in CI against the real artifacts.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
script="$here/check-ffmpeg-bundle.sh"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

target="x86_64-unknown-linux-gnu"
binaries="$tmp/src-tauri/binaries"
mkdir -p "$binaries" "$tmp/bin"

# Stub ldd. Writes whatever was staged in $tmp/ldd-output.
cat > "$tmp/bin/ldd" <<'EOF'
#!/usr/bin/env bash
cat "${LDD_FIXTURE:?}"
EOF
chmod +x "$tmp/bin/ldd"

make_bundle() {
  rm -rf "$binaries"
  mkdir -p "$binaries/ffmpeg-$target-libs"
  printf '#!/bin/false\n' > "$binaries/ffmpeg-$target"
  chmod +x "$binaries/ffmpeg-$target"
}

run_check() {
  PATH="$tmp/bin:$PATH" ROOT="$tmp" FFMPEG_TARGET="$target" \
    LDD_FIXTURE="$tmp/ldd-output" bash "$script"
}

# Case 1: every library resolves -> exit 0
make_bundle
cat > "$tmp/ldd-output" <<'EOF'
	linux-vdso.so.1 (0x00007ffd8f5fe000)
	libavdevice.so.63 => /libs/libavdevice.so.63 (0x00007f2b1c000000)
	libavfilter.so.12 => /libs/libavfilter.so.12 (0x00007f2b1b000000)
EOF
run_check >/dev/null
echo "PASS: fully resolved sidecar accepted"

# Case 2: a SONAME the loader cannot find -> non-zero. This is the shape a
# dropped symlink produces: the versioned real file is present, the SONAME the
# binary asks for is not.
make_bundle
cat > "$tmp/ldd-output" <<'EOF'
	linux-vdso.so.1 (0x00007ffd8f5fe000)
	libavdevice.so.63 => not found
	libavfilter.so.12 => /libs/libavfilter.so.12 (0x00007f2b1b000000)
EOF
if run_check >/dev/null 2>&1; then
  echo "FAIL: unresolved library not detected" >&2; exit 1
fi
echo "PASS: unresolved library rejected"

# Case 3: no sidecar at all -> non-zero, not a skip.
rm -rf "$binaries"
mkdir -p "$binaries"
printf 'ignored\n' > "$tmp/ldd-output"
if run_check >/dev/null 2>&1; then
  echo "FAIL: missing sidecar treated as pass" >&2; exit 1
fi
echo "PASS: missing sidecar rejected"

# Case 4: sidecar present but the -libs directory is gone -> non-zero.
rm -rf "$binaries"
mkdir -p "$binaries"
printf '#!/bin/false\n' > "$binaries/ffmpeg-$target"
chmod +x "$binaries/ffmpeg-$target"
if run_check >/dev/null 2>&1; then
  echo "FAIL: missing lib directory treated as pass" >&2; exit 1
fi
echo "PASS: missing lib directory rejected"

echo "ALL TESTS PASSED"
