# Releasing Johnny Reader

Checklist for producing a signed, notarized macOS build.

## Automated release (preferred)

Push a `vX.Y.Z` tag with the self-hosted runner up and CI does everything below
automatically — build, Developer-ID signing of the bundled ffmpeg/pdfium libs,
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
then enforce a real pubkey (fatal when `JOHNNY_READER_REQUIRE_UPDATER_KEY=1`).

## 2. macOS code signing + notarization

Requires a "Developer ID Application" certificate in the login keychain and a
stored `notarytool` keychain profile (so secrets stay out of the shell):

```bash
# one-time: store notarization credentials in the keychain
xcrun notarytool store-credentials jr-notary \
  --apple-id "<apple-id-email>" --team-id <TEAMID>
```

### Important: sign the bundled native libraries first

Tauri signs the main binary and the `ffmpeg` sidecar, but **not** the bundled
ffmpeg shared libraries or `libpdfium.dylib` (they ship under
`Contents/Resources/binaries/` and `…/resources/pdfium/`). Notarization rejects
ad-hoc-signed Mach-O files, so sign the **source** libraries with Developer ID +
hardened runtime + secure timestamp before building:

```bash
ID="Developer ID Application: <Name> (<TEAMID>)"
find src-tauri/binaries/ffmpeg-*-libs -type f -name '*.dylib' \
  -exec codesign --force --options runtime --timestamp --sign "$ID" {} \;
codesign --force --options runtime --timestamp --sign "$ID" src-tauri/binaries/ffmpeg-*
codesign --force --options runtime --timestamp --sign "$ID" \
  src-tauri/resources/pdfium/*/libpdfium.dylib
```

(These dirs are gitignored local assets; re-sign after any ffmpeg/pdfium bump.)

### Build, notarize, staple, verify

```bash
export APPLE_SIGNING_IDENTITY="Developer ID Application: <Name> (<TEAMID>)"
npm run tauri:build

DMG="target/release/bundle/dmg/Johnny Reader_0.1.0_aarch64.dmg"
xcrun notarytool submit "$DMG" --keychain-profile jr-notary --wait
xcrun stapler staple "$DMG"
xcrun stapler staple "target/release/bundle/macos/Johnny Reader.app"
spctl -a -t open --context context:primary-signature -vvv "$DMG"   # expect: accepted, Notarized Developer ID
```

If notarization returns Invalid, read the log:
`xcrun notarytool log <submission-id> --keychain-profile jr-notary`.

## 3. Pre-publish verification

```bash
npm run build
cargo test -p johnny-reader
git diff --check
```

## 4. Architectures

The default build targets the host (Apple Silicon `aarch64`). For Intel/universal
builds, install the `x86_64-apple-darwin` target and provide x86_64 ffmpeg/pdfium
libraries signed the same way.
