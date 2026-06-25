# Releasing Johnny Reader

This checklist covers the steps required before publishing a public build. The
default repository state intentionally ships with placeholders so that local
development works without secrets; a real release must supply them.

## 1. Auto-updater signing key (required if updater is enabled)

`src-tauri/tauri.conf.json` enables the Tauri updater plugin with a placeholder
public key (`TAURI_UPDATER_PUBKEY_PLACEHOLDER`). The build warns whenever this
placeholder is present, and fails hard when `JOHNNY_READER_REQUIRE_UPDATER_KEY=1`
is set (use this in release CI so a misconfigured updater can never ship).

Generate a keypair once and keep the private key secret:

```bash
npm run tauri -- signer generate -w ~/.johnny-reader/updater.key
```

Then:

1. Put the printed **public** key in `tauri.conf.json` under
   `plugins.updater.pubkey`.
2. At build time, provide the **private** key so release artifacts are signed:

   ```bash
   export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.johnny-reader/updater.key)"
   export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="<password-if-set>"
   ```

3. Confirm `plugins.updater.endpoints` in `tauri.conf.json` points at the
   correct published release manifest (currently the project's GitHub
   `releases/latest/download/latest.json`), and that the updater plugin is still
   registered in `src-tauri/src/lib.rs`.
4. Publish the generated `latest.json` and signatures to the GitHub release that
   the `endpoints` URL points at.

If you do **not** want auto-update in a release, remove the `updater` plugin from
`tauri.conf.json` (`plugins.updater`), `src-tauri/Cargo.toml`
(`tauri-plugin-updater`), `src-tauri/src/lib.rs` (the plugin registration), and
`src-tauri/capabilities/default.json` (`updater:default`).

## 2. macOS code signing and notarization

`tauri.conf.json` ships with `bundle.macOS.signingIdentity: null`, so local builds
are unsigned and users will see Gatekeeper warnings. For a public macOS release,
sign and notarize:

```bash
export APPLE_SIGNING_IDENTITY="Developer ID Application: <Your Name> (<TEAMID>)"
export APPLE_ID="<your-apple-id-email>"
export APPLE_PASSWORD="<app-specific-password>"
export APPLE_TEAM_ID="<TEAMID>"
npm run tauri:build
```

Tauri notarizes automatically when the `APPLE_*` variables are present. Verify the
result with `spctl -a -vvv "target/release/bundle/macos/Johnny Reader.app"`.

## 3. Build

```bash
JOHNNY_READER_REQUIRE_UPDATER_KEY=1 npm run tauri:build
```

Artifacts are written under `target/release/bundle/`.

## 4. Pre-publish verification

```bash
npm run build
cargo test -p johnny-reader
git diff --check
```
