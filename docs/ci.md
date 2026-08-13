# CI & Release Automation

Two workflows live in `.github/workflows/`:

- **`ci.yml`** — runs on GitHub-hosted `macos-14` for every pull request and push
  to `main`: `npm run build`, `cargo fmt --all --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test -p libretexts-reader`.
  No secrets, no signing.
- **`release.yml`** — runs on a **self-hosted** Apple-Silicon runner (label
  `release`), triggered only by pushing a `vX.Y.Z` tag (or a manual dry-run). It
  builds, Developer-ID-signs the bundled native libs, runs `tauri build`,
  notarizes with the local `jr-notary` profile, staples, and publishes a GitHub
  Release with the DMG + SHA-256.

## Why self-hosted, and the security rules

The signing certificate and the `jr-notary` notarization profile live in the
release Mac's keychain, so building there keeps all secrets off GitHub. Because
this repo is **public**, a self-hosted runner is only safe under these rules
(enforced by the workflow — do not weaken them):

- `release.yml` triggers **only** on `v*` tag pushes and `workflow_dispatch`.
  Never `pull_request`. Tags require write access, so the runner never executes
  code from a fork PR.
- Only `release.yml` uses the `release` runner label. Never add that label to any
  other (especially PR-triggered) workflow.
- Run the runner **on-demand / ephemeral** — start it when cutting a release, stop
  it afterward. Do not leave it idling 24/7.

## One-time setup

> Values in angle brackets below (`<Your Name>`, `<TEAMID>`, `<REG_TOKEN>`,
> `<login-keychain-password>`) are fill-ins for the operator running these
> commands locally — do not commit real credentials (especially the keychain
> password) to this public repo.

1. **Register the self-hosted runner** (Settings → Actions → Runners → New
   self-hosted runner, macOS/arm64). During `./config.sh`, give it the labels
   `macos` and `release`:
   ```bash
   ./config.sh --url https://github.com/johnnylibretexts/libretexts-reader \
     --token <REG_TOKEN> --labels macos,release --name jr-release-mac --ephemeral
   ```
   `--ephemeral` makes the runner take one job then exit (re-run `./run.sh` per
   release), which is the recommended on-demand posture.

2. **Set the signing-identity variable** (non-secret):
   ```bash
   gh variable set APPLE_SIGNING_IDENTITY \
     --body "Developer ID Application: <Your Name> (<TEAMID>)"
   ```

3. **Authorize codesign to use the signing key non-interactively** (one time, so
   the runner does not hit a keychain UI prompt):
   ```bash
   security set-key-partition-list -S apple-tool:,apple: -s -k "<login-keychain-password>" login.keychain-db
   ```
   (Equivalent to clicking "Always Allow" once during a manual `codesign` run.)

4. **Confirm prerequisites on the runner Mac:** `gh` (authenticated or relying on
   the workflow `GITHUB_TOKEN`), `xcrun`, `nvm` with Node 22.20.0 installed, the
   Developer ID cert in the login keychain, and the `jr-notary` profile
   (`xcrun notarytool store-credentials jr-notary …` — see `RELEASE.md`).

## Cutting a release

```bash
# 1. Bump the version in all three files if needed (Cargo.toml, package.json,
#    src-tauri/tauri.conf.json) and commit.
# 2. Start the runner on the release Mac:
./run.sh            # (in the runner install dir; exits after one job if --ephemeral)
# 3. Push the tag:
git tag v0.1.1
git push origin v0.1.1
```
The workflow builds, signs, notarizes, and publishes automatically. A tag whose
version contains a pre-release suffix (e.g. `v0.2.0-beta.1`) is published as a
GitHub **pre-release**; a plain `vX.Y.Z` becomes a full release.

## Dry run / testing

- `workflow_dispatch` with `dry_run: true` (Actions tab → Release → Run workflow)
  builds + Developer-ID-signs but skips notarize/publish.
- Full end-to-end test: push a throwaway `v0.0.0-citest` tag (auto-detected as a
  pre-release) with the runner up, verify the release is created, then delete the
  test release and tag.
