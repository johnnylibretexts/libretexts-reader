#!/usr/bin/env bash

set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "usage: $0 <output.dmg> <LibreTexts Reader.app>" >&2
  exit 2
fi

OUTPUT="$1"
APP="$2"

if [ ! -d "$APP" ]; then
  echo "app bundle not found: $APP" >&2
  exit 1
fi

case "$OUTPUT" in
  /*) ;;
  *) OUTPUT="$PWD/$OUTPUT" ;;
esac

mkdir -p "$(dirname "$OUTPUT")"

STAGE="$(mktemp -d "${TMPDIR:-/tmp}/libretexts-reader-dmg.XXXXXX")"
cleanup() {
  rm -rf "$STAGE"
}
trap cleanup EXIT

ditto "$APP" "$STAGE/LibreTexts Reader.app"
ln -s /Applications "$STAGE/Applications"

# On the release host, hdiutil cannot copy the app into a volume mounted as
# "/Volumes/LibreTexts Reader" (EPERM). The compact label avoids that macOS
# restriction; it does not change the app name, bundle identifier, or DMG name.
hdiutil create \
  -volname "LibreTextsReader" \
  -srcfolder "$STAGE" \
  -format UDZO \
  -ov \
  "$OUTPUT"
