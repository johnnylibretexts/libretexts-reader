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

Tauri signs the main binary, but **not** `libpdfium.dylib` (it ships under
`…/resources/pdfium/`). Notarization rejects ad-hoc-signed Mach-O files, so sign
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

### Build, notarize, staple, verify

```bash
export APPLE_SIGNING_IDENTITY="Developer ID Application: <Name> (<TEAMID>)"

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
DMG="target/release/bundle/dmg/LibreTexts Reader_${VERSION}_aarch64.dmg"
xcrun notarytool submit "$DMG" --keychain-profile jr-notary --wait
xcrun stapler staple "$DMG"
xcrun stapler staple "target/release/bundle/macos/LibreTexts Reader.app"
spctl -a -t open --context context:primary-signature -vvv "$DMG"   # expect: accepted, Notarized Developer ID
```

If notarization returns Invalid, read the log:
`xcrun notarytool log <submission-id> --keychain-profile jr-notary`.

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
