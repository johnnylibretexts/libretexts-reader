# Releasing LibreTexts Reader

Checklist for producing a signed, notarized macOS build.

## Automated release (preferred)

Push a `vX.Y.Z` tag with the self-hosted runner up and CI does everything below
automatically — build, Developer-ID signing of the bundled PDFium lib,
`tauri build`, notarization via the `jr-notary` profile, stapling, and publishing
the GitHub Release with the DMG + SHA-256. See `docs/ci.md` for runner setup and
the exact steps. The manual checklist below is the fallback and documents what the
workflow runs under the hood.

## 1. Auto-updater

The auto-updater is **disabled** in v0.1.0 — the app ships without in-app
updates; distribute new versions by publishing a new build. There is no updater
plugin, endpoint, or signing key to manage.

To add auto-update later, re-introduce `tauri-plugin-updater` in
`src-tauri/Cargo.toml`, register it in `src-tauri/src/lib.rs`, add
`updater:default` to `src-tauri/capabilities/default.json`, add a
`plugins.updater` block (endpoints + pubkey) to `src-tauri/tauri.conf.json`, and
generate a key with `npm run tauri -- signer generate`. The `build.rs` guard will
then enforce a real pubkey (fatal when `LIBRETEXTS_READER_REQUIRE_UPDATER_KEY=1`).

## 2. macOS code signing + notarization

Requires a "Developer ID Application" certificate in the login keychain and a
stored `notarytool` keychain profile (so secrets stay out of the shell):

```bash
# one-time: store notarization credentials in the keychain
xcrun notarytool store-credentials jr-notary \
  --apple-id "<apple-id-email>" --team-id <TEAMID>
```

### Important: sign the bundled native libraries first

Tauri signs the main binary, but **not** `libpdfium.dylib`. Notarization rejects
ad-hoc-signed Mach-O files, so sign
the **source** library with Developer ID + hardened runtime + secure timestamp
before building. Signing it *after* the build is not equivalent: the edit
invalidates Tauri's signature over the enclosing `.app`.

```bash
ID="Developer ID Application: <Name> (<TEAMID>)"
codesign --force --options runtime --timestamp --sign "$ID" \
  src-tauri/resources/pdfium/*/libpdfium.dylib

# Verify nothing ad-hoc survived into the bundle after building:
#   find "target/release/bundle/macos/LibreTexts Reader.app" -type f \
#     -exec sh -c 'file "$1" | grep -q Mach-O && codesign -dv "$1" 2>&1 | grep -q adhoc && echo "$1"' _ {} \;
# Expect no output. Any line printed here is a notarization rejection.
```

(This dir is a gitignored local asset; re-sign after any PDFium bump.)

**Where it lands in the bundle**, which is not where you would guess:

```
<App>/Contents/Resources/resources/pdfium/aarch64-apple-darwin/libpdfium.dylib
```

Note `resources/resources`. `tauri.conf.json` bundles `"resources/**/*"`, and that
glob keeps its own `resources/` prefix inside `Contents/Resources/`. Anyone
reaching for the obvious `Contents/Resources/pdfium/...` finds nothing there, and
a verification command written against that path reports success by matching
zero files -- which reads exactly like a pass.

### Build, notarize, staple, verify

**The order is build → codesign → notarize → staple, and it runs twice.** Read
"Why it runs twice" below before changing anything here — both the double pass and
the `codesign` call are load-bearing, and skipping either produces a DMG that
passes some checks and fails others.

```bash
export APPLE_SIGNING_IDENTITY="Developer ID Application: <Name> (<TEAMID>)"
ID="$APPLE_SIGNING_IDENTITY"
ROOT="$(pwd)"

# CI=true is load-bearing for a LOCAL build and is not optional. Without it,
# `tauri build` gets all the way through compiling and bundling the .app and
# then dies in bundle_dmg.sh:
#
#   execution error: Finder got an error: AppleEvent timed out. (-1712)
#   Failed running AppleScript
#
# That step is pure Finder cosmetics (window size, icon positions) and it
# needs a GUI session to drive Finder, which a terminal over SSH or an
# automation shell does not have. CI=true makes the bundler pass
# --skip-jenkins, which skips it; the DMG is otherwise identical.
#
# release.yml does NOT need this line -- the Actions runner sets CI=true
# itself, on self-hosted runners too. It is only the by-hand path that trips.
CI=true npm run tauri:build

# Derived, not hard-coded -- this is the same expression release.yml uses, so
# the runbook and the workflow cannot disagree about the filename.
VERSION="$(node -e "process.stdout.write(String(require('./src-tauri/tauri.conf.json').version))")"
DMG="$ROOT/target/release/bundle/dmg/LibreTexts Reader_${VERSION}_aarch64.dmg"
APP="$ROOT/target/release/bundle/macos/LibreTexts Reader.app"

# --- Pass 1: get the .app a ticket -------------------------------------------
# Notarizing the DMG also notarizes the .app inside it, which is what makes a
# ticket available to staple onto the .app on the next line. tauri:build has
# already codesigned both the .app and this DMG, so there is nothing to sign here.
xcrun notarytool submit "$DMG" --keychain-profile jr-notary --wait
xcrun stapler staple "$APP"

# --- Pass 2: rebuild the DMG around the now-stapled .app ----------------------
STAGE="$(mktemp -d)"
ditto "$APP" "$STAGE/LibreTexts Reader.app"

# Build to a temp dir under the CANONICAL filename: codesign derives the
# signature Identifier from the file name, and it must match what pass 1 produced
# (`LibreTexts Reader_<version>_aarch64`).
BUILDDIR="$(mktemp -d)"
NEWDMG="$BUILDDIR/LibreTexts Reader_${VERSION}_aarch64.dmg"
cd "$ROOT/target/release/bundle/dmg"
./bundle_dmg.sh --volname "LibreTexts Reader" --icon "LibreTexts Reader.app" 180 170 \
  --app-drop-link 480 170 --window-size 660 400 \
  --hide-extension "LibreTexts Reader.app" --volicon "icon.icns" --skip-jenkins \
  "$NEWDMG" "$STAGE"
cd "$ROOT"

# codesign BEFORE notarizing. tauri:build signs the .dmg for you; a DMG built by
# hand from bundle_dmg.sh is NOT signed, and an unsigned DMG notarizes and staples
# perfectly happily while `spctl` still rejects it with "no usable signature".
# Signing after stapling is not an option either -- it rewrites the file and
# invalidates the ticket.
codesign --force --sign "$ID" --timestamp "$NEWDMG"
codesign --verify --strict "$NEWDMG"        # cheap + local; do this before the 5-40 min round trip
xcrun notarytool submit "$NEWDMG" --keychain-profile jr-notary --wait
xcrun stapler staple "$NEWDMG"
mv -f "$NEWDMG" "$DMG"

# --- Verify: all six must pass -----------------------------------------------
# Nothing ad-hoc survived into the bundle. This is the one that catches an
# unsigned bundled native, and it scans every Mach-O rather than trusting a
# hard-coded path -- so it keeps working if a future dependency ships another
# dylib nobody remembered to sign. Expect no output.
find "$APP" -type f -exec sh -c \
  'file "$1" | grep -q Mach-O && codesign -dv "$1" 2>&1 | grep -q adhoc && echo "UNSIGNED: $1"' _ {} \;

codesign -dvv "$DMG" 2>&1 | grep "Authority=Developer ID Application"
xcrun stapler validate "$DMG"
spctl -a -t open --context context:primary-signature -vvv "$DMG"  # accepted, Notarized Developer ID

MNT="$(mktemp -d)"
hdiutil attach "$DMG" -nobrowse -readonly -mountpoint "$MNT" >/dev/null
xcrun stapler validate "$MNT/LibreTexts Reader.app"   # the point of pass 2 -- must NOT say
                                                     # "does not have a ticket stapled to it"
spctl -a -t exec -vvv "$MNT/LibreTexts Reader.app"
hdiutil detach "$MNT" >/dev/null
```

### Verify a release that has already been published

The checks above run against a local build. Once `release.yml` has published a
tag, verify **the artifact a reader will actually download** -- that is the only
thing that proves the automated path produced what the manual path would have.
A green workflow does not prove a stapled ticket.

```bash
gh release download <TAG> --repo johnnylibretexts/libretexts-reader --pattern '*.dmg'
DMG="$(ls LibreTexts.Reader_*_aarch64.dmg | head -1)"

xcrun stapler validate "$DMG"                                    # The validate action worked!
spctl -a -vvv -t open --context context:primary-signature "$DMG" # accepted
                                                                 # source=Notarized Developer ID

# The inner .app needs its own ticket, which is the entire point of pass 2.
# Without it the DMG passes and the app still says "cannot be verified" the
# first time someone opens it offline.
MNT="$(mktemp -d)"
hdiutil attach "$DMG" -nobrowse -readonly -mountpoint "$MNT" >/dev/null
xcrun stapler validate "$MNT/LibreTexts Reader.app"
find "$MNT/LibreTexts Reader.app" -type f -exec sh -c \
  'file "$1" | grep -q Mach-O && codesign -dv "$1" 2>&1 | grep -q adhoc && echo "UNSIGNED: $1"' _ {} \;
hdiutil detach "$MNT" >/dev/null
```

Done for v0.1.0-beta.2: all four clean.

If notarization returns Invalid, read the log:
`xcrun notarytool log <submission-id> --keychain-profile jr-notary`.

### Why it runs twice

Tauri bundles the DMG from the `.app` *before* anything is stapled, so a
single-pass run (notarize DMG → staple DMG → staple `.app`) staples the `.app`
sitting in `target/release/bundle/macos/` — a copy no one ships. The copy inside
the DMG, the one a tester drags to `/Applications`, still has no ticket:

```
$ xcrun stapler validate "/Volumes/LibreTexts Reader/LibreTexts Reader.app"
LibreTexts Reader.app does not have a ticket stapled to it.
```

That build is not broken — Gatekeeper accepts it by fetching the ticket from
Apple over the network — but a tester whose **first launch is offline** gets a
"cannot be verified" dialog. Pass 2 exists to close that hole, and stapling the
`.app` costs one extra notarization round trip.

Two ways this hides from you, both worth knowing:

- **`stapler validate` and `spctl` answer different questions.** Stapling asks
  "is a valid ticket attached to this file?"; `spctl --context
  context:primary-signature` asks "is this signed by a trusted Developer ID?".
  An unsigned DMG passes the first and fails the second. Checking only the
  stapler output ships a broken artifact.
- **`spctl` uses the network.** On an online machine it reports `accepted /
  Notarized Developer ID` for an *unstapled* app, because it fetches the ticket
  from Apple. It cannot tell you whether stapling worked. Only `stapler
  validate` can, which is why the verify block runs both.

To confirm the end state the way a tester experiences it, copy the app out of the
mounted DMG and mark it quarantined — that xattr is what triggers Gatekeeper's
assessment in the first place:

```bash
ditto "$MNT/LibreTexts Reader.app" "/tmp/qtest/LibreTexts Reader.app"
xattr -w com.apple.quarantine "0081;00000000;Safari;" "/tmp/qtest/LibreTexts Reader.app"
xcrun stapler validate "/tmp/qtest/LibreTexts Reader.app"
spctl -a -t exec -vvv "/tmp/qtest/LibreTexts Reader.app"
```

**`release.yml` does not do any of this yet** — `release.yml:131-133` is the
single-pass sequence, so the automated path still produces a DMG whose inner
`.app` is unstapled. See #102.

## 3. Pre-publish verification

**The automated path runs this for you.** `release.yml` calls the same gate
`ci.yml` does — `.github/workflows/verify.yml` — as a `verify` job that the
build job `needs:`, so a tag pointing at an unvalidated commit fails before
anything publishable is built. It was not always so: the release workflow used
to run no tests at all, and this section was the only thing standing between a
tag and a published DMG.

Run it by hand only when releasing outside the workflow:

```bash
npm run build
npm test
cargo test -p libretexts-reader
git diff --check
```

The gate also runs `scripts/ci/check-updater-key.sh`, which fails if
`tauri-plugin-updater` is ever added as a dependency without a real
`plugins.updater.pubkey`. `build.rs` guards the same thing from the
configuration side but cannot see the dependency graph, so neither check
replaces the other. The updater is deliberately absent in v0.1.0, and both are
quiet until it comes back.

## 4. Platforms and architectures

**A release builds macOS only.** `bundle.targets` in `tauri.conf.json` is
`["dmg", "app"]`, and `release.yml` is a single macOS job.

`msi` and `nsis` used to appear in that list. They never produced anything: the Tauri
bundler **silently** filters package types to the host platform, so on a macOS runner
those two were dropped with no error and no warning. Config that looks like support but
builds nothing is how a maintainer later assumes a platform is covered when it never was,
so the list now states what is actually produced.

The `bundle.windows.wix` block and `icons/icon.ico` are deliberately kept — they are
configuration for *if* Windows is built, not a claim that it is. Adding Windows means a
second `release.yml` job plus an Authenticode signing story, and Linux means adding
`deb`/`appimage`/`rpm` to the targets list (absent today, so a Linux build emits
"No bundles were built"). `build.rs` already carries PDFium assets for
`x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu`, and `aarch64-unknown-linux-gnu`,
so the groundwork survives either way. See #67.

The default build targets the host (Apple Silicon `aarch64`). For Intel/universal
builds, install the `x86_64-apple-darwin` target and provide x86_64 PDFium
libraries signed the same way. Intel macOS is unbuilt and untested — a release is
Apple-Silicon only unless someone does that work.
