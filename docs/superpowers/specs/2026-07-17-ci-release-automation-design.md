# CI + Release Automation — Design

Date: 2026-07-17
Status: Approved (design); pending implementation plan
Scope: Automate building, signing, notarizing, and publishing macOS releases of
Johnny Reader on tag push, plus lightweight hosted CI on PRs/pushes.

## Goal

Replace the fully manual `RELEASE.md` process with:

1. **Hosted CI** that catches breakage early on every PR and push to `main`.
2. **Automated release** so that pushing a `vX.Y.Z` tag produces a signed,
   notarized, stapled DMG attached to a published GitHub Release — with no manual
   build/notarize steps.

Non-goals (explicitly out of scope, YAGNI):

- Intel / universal (`x86_64` / universal) builds. Releases build **aarch64 only**.
  (Universal build is separately-deferred release work, not part of this.)
- Auto-updater (removed in v0.1.0; `build.rs` returns early when no updater
  plugin is configured).
- Windows/Linux packaging. `tauri.conf.json` lists `msi`/`nsis` targets, but Tauri
  only builds host-valid targets, so a macOS runner produces `dmg` + `app` only.

## Background / grounding facts

- Repo: `github.com/johnnyrobot/johnny-reader`, **public**, Apache-2.0, default
  branch `main`. No `.github/` directory exists yet — this is greenfield CI.
- Version is declared in three places, all currently `0.1.0`:
  - `Cargo.toml` → `[workspace.package].version` (src-tauri inherits via `version.workspace = true`)
  - `package.json` → `version`
  - `src-tauri/tauri.conf.json` → `version` (authoritative for the DMG file name)
- Toolchain: Node **22.20.0** (via nvm locally), Rust stable pinned by
  `rust-toolchain.toml` (includes `clippy` + `rustfmt`), workspace `rust-version = 1.88`.
- **Native libraries are downloaded reproducibly by `src-tauri/build.rs`** at build
  time with pinned release tags + pinned SHA-256:
  - PDFium: `bblanchon/pdfium-binaries@chromium/7789` → `src-tauri/resources/pdfium/<target>/libpdfium.dylib`
  - ffmpeg (macOS): `ColorsWind/FFmpeg-macOS@n5.0.1-patch3` → sidecar
    `src-tauri/binaries/ffmpeg-aarch64-apple-darwin` + dylibs under
    `src-tauri/binaries/ffmpeg-aarch64-apple-darwin-libs/`.
    (The rolling-`latest` reproducibility caveat in `build.rs` applies only to the
    BtbN Windows/Linux assets, **not** the pinned macOS build.)
  - `build.rs` caches via `.sha256` / `.asset-sha256` marker files and returns early
    when the marker matches — so a second build will **not** re-download or
    re-sign already-prepared libs.
- `build.rs` **ad-hoc** signs the ffmpeg dylibs (`codesign --force --sign -`).
  Notarization rejects ad-hoc-signed Mach-O, so they must be **re-signed with
  Developer ID** before Tauri bundles them (per `RELEASE.md`).
- DMG output path (verified): `target/release/bundle/dmg/Johnny Reader_<version>_aarch64.dmg`.
- Tooling already present on the release Mac: `gh 2.96.0`, `xcrun`, the Developer ID
  cert in the login keychain, and a `notarytool` keychain profile named `jr-notary`.

## Key decisions (from brainstorming)

1. **Release runner: self-hosted on the user's Apple-Silicon Mac.** The signing cert
   and `jr-notary` notarization profile already live there, so **no signing secrets
   go into GitHub**.
2. **Hosted CI added** for everyday verification (fmt/clippy/test/build) so the
   self-hosted runner is used *exclusively* for tag releases.
3. **Release publishes a full GitHub Release** by default, auto-flipped to
   pre-release only when the tag is a semver pre-release (e.g. `v0.2.0-beta.1`).

## Security model (non-negotiable — self-hosted runner on a PUBLIC repo)

A self-hosted runner will, by default, execute code from any workflow that targets
it, including fork PRs. On a public repo that is arbitrary-code-execution on the
user's machine. The design forecloses this:

- **`release.yml` triggers only on `push: tags: ['v*']`** (and manual
  `workflow_dispatch`). Never `pull_request`. Tags are creatable only by users with
  write access (the owner), so the self-hosted job never runs untrusted code.
- **A dedicated runner label `release`** is required by `release.yml`
  (`runs-on: [self-hosted, macos, release]`). No other workflow references that
  label, so nothing else can land on the runner.
- **Everyday CI runs on GitHub-hosted runners only** (`ci.yml`), never self-hosted.
- **The runner is run on-demand / ephemeral** — started when cutting a release and
  stopped afterward — not left idling 24/7.
- A defensive `if: github.repository == 'johnnyrobot/johnny-reader'` guard on the
  release job.
- Third-party actions are pinned (see Implementation notes).

## Architecture

Two workflow files under `.github/workflows/`:

| File | Runner | Trigger | Secrets | Purpose |
|---|---|---|---|---|
| `ci.yml` | hosted `macos-14` (arm64) | `pull_request`, `push` → `main` | none | fmt / clippy / test / frontend build |
| `release.yml` | self-hosted `[self-hosted, macos, release]` | `push` tags `v*`; `workflow_dispatch` (dry-run) | none (uses local keychain + auto `GITHUB_TOKEN`) | build → sign → notarize → staple → publish |

### `ci.yml` (hosted verification)

Steps:

1. `actions/checkout`
2. `actions/setup-node` pinned to Node **22.20.0**, then `npm ci`
3. Rust from `rust-toolchain.toml` (rustup auto-installs; clippy + rustfmt included)
4. `npm run build` (tsc + vite)
5. `cargo fmt --check`
6. `cargo clippy --all-targets -- -D warnings`
7. `cargo test -p johnny-reader`

Notes:

- The Rust compile triggers `build.rs`, which network-downloads pdfium/ffmpeg;
  hosted runners have network, so this works unmodified. `#[ignore]`d live-import
  tests stay skipped.
- **Green-on-arrival prerequisite:** before enabling the `-D warnings` gate, run
  `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and
  `cargo test -p johnny-reader` locally and fix any pre-existing lint/format issues
  (or, if the fixes are large/contentious, drop to non-`-D` clippy and record why).
  The implementation plan must include this step and report the delta.
- Optional (nice-to-have, not required): cache `~/.cargo` and `node_modules`.

### `release.yml` (self-hosted release)

Triggers:

```yaml
on:
  push:
    tags: ['v*']
  workflow_dispatch:
    inputs:
      dry_run:
        description: 'Build + sign but skip notarize/publish'
        type: boolean
        default: true
```

Job config: `runs-on: [self-hosted, macos, release]`,
`permissions: { contents: write }`, `if: github.repository == 'johnnyrobot/johnny-reader'`,
concurrency group per ref.

Steps:

1. **Checkout** (clean).
2. **Tag ↔ version gate.** For a `push` tag event, strip the leading `v` from
   `github.ref_name` → `TAG_VERSION`. Read `.version` from `src-tauri/tauri.conf.json`
   (jq) and assert it equals `TAG_VERSION`; cross-check `package.json` and the
   workspace `Cargo.toml` version agree. Fail the job on any mismatch. (Skipped for
   `workflow_dispatch`, which derives the version from `tauri.conf.json` directly.)
3. **Setup env.** `source ~/.nvm/nvm.sh && nvm use 22.20.0`, `. ~/.cargo/env`, `npm ci`.
4. **Materialize native libs.** `cargo build --release -p johnny-reader` — runs
   `build.rs`, which downloads + prepares pdfium and ffmpeg into
   `src-tauri/resources/pdfium/...` and `src-tauri/binaries/...`.
5. **Developer-ID re-sign the native libs** (before Tauri bundles them):
   ```bash
   ID="$APPLE_SIGNING_IDENTITY"   # from vars.APPLE_SIGNING_IDENTITY
   # dylibs (ffmpeg libs + pdfium)
   find src-tauri/binaries -type f -name '*.dylib' \
     -exec codesign --force --options runtime --timestamp --sign "$ID" {} \;
   codesign --force --options runtime --timestamp --sign "$ID" \
     src-tauri/resources/pdfium/*/libpdfium.dylib
   # ffmpeg sidecar executable only (NOT the -libs dir or .sha256 markers)
   codesign --force --options runtime --timestamp --sign "$ID" \
     src-tauri/binaries/ffmpeg-aarch64-apple-darwin
   ```
   (Tightened vs `RELEASE.md`'s looser `ffmpeg-*` glob to avoid touching
   `.sha256` marker files or the `-libs` directory entry.)
6. **Build the bundle.** `APPLE_SIGNING_IDENTITY="$ID" npm run tauri:build`. Because
   the `build.rs` markers are already satisfied, the libs are not re-downloaded or
   re-ad-hoc-signed; the Developer-ID signatures survive into the bundle. Optionally
   set `JOHNNY_READER_REQUIRE_UPDATER_KEY=1` as a fail-closed safety belt (currently
   a no-op since no updater plugin is configured).
7. **Locate DMG:** `target/release/bundle/dmg/Johnny Reader_<version>_aarch64.dmg`.
8. **Notarize + staple + verify** (uses the local `jr-notary` profile — no secrets):
   ```bash
   xcrun notarytool submit "$DMG" --keychain-profile jr-notary --wait
   xcrun stapler staple "$DMG"
   xcrun stapler staple "target/release/bundle/macos/Johnny Reader.app"
   spctl -a -t open --context context:primary-signature -vvv "$DMG"   # expect accepted
   ```
   If `dry_run`, skip this step and step 9.
9. **Publish.** Compute `shasum -a 256 "$DMG"`. Assemble the release body by
   generating commit-based notes and appending a `SHA-256: <hash>  <dmg name>` line,
   then create the release passing the assembled body via `--notes-file` (avoid
   combining `--generate-notes` with `--notes`, which conflict):
   ```bash
   gh release create "$TAG" "$DMG" --title "Johnny Reader $TAG" \
     --notes-file notes.md $PRERELEASE_FLAG
   ```
   Notes assembly detail (implementation): use the GitHub "generate notes" API
   (`gh api repos/{owner}/{repo}/releases/generate-notes`) or a simple
   `git log` range to build the body, then append the SHA-256 line.
   `PRERELEASE_FLAG=--prerelease` iff the tag contains a semver pre-release
   identifier (a `-`, e.g. `v0.2.0-beta.1`); otherwise a full release. Auth via the
   auto-provided `GITHUB_TOKEN` (exported as `GH_TOKEN`).

## Secrets & configuration

- **No GitHub secrets.** Sensitive material (Developer ID cert, notarization
  credentials) stays in the local keychain.
- One **Actions variable** (non-sensitive): `APPLE_SIGNING_IDENTITY` =
  `"Developer ID Application: <Name> (<TEAMID>)"`. Stored as a repo *variable*, not a
  secret (it is not confidential, and keeping it out of the YAML avoids hardcoding
  personal details in a public file).
- Release job auth: the workflow-scoped `GITHUB_TOKEN` (needs `contents: write`).

## Keychain handling

Because the runner executes inside the user's logged-in session on demand, the login
keychain is already unlocked and `codesign` / `notarytool` succeed without prompts —
**provided** a one-time authorization has been granted so `codesign` can use the
signing key non-interactively:

```bash
security set-key-partition-list -S apple-tool:,apple: -s -k "<login-pw>" login.keychain-db
```

(or clicking "Always Allow" once during a manual signing run). No keychain password
is ever placed in CI. This one-time step is documented in `docs/ci.md`.

## Documentation changes

- **New `docs/ci.md`**: how to register the self-hosted runner with the `release`
  label; how to run it ephemeral/on-demand (start when releasing, stop after — not
  24/7); the one-time keychain authorization; setting the `APPLE_SIGNING_IDENTITY`
  variable; and the security rationale (tag-only trigger, dedicated label).
- **Update `RELEASE.md`**: lead with the automated flow ("push a `vX.Y.Z` tag → CI
  builds, signs, notarizes, and publishes"), and retain the current manual steps as
  the documented fallback — they are exactly what the workflow runs under the hood.

## Testing the pipeline

1. **`ci.yml`**: validate by opening a PR / pushing a branch and confirming all four
   checks run and pass.
2. **`release.yml` dry-run**: `workflow_dispatch` with `dry_run: true` (runner up) —
   builds + Developer-ID-signs but skips notarize/publish. Confirms the lib-signing
   and Tauri-bundle sequence works end-to-end without creating a release.
3. **`release.yml` full run**: push a throwaway pre-release tag `v0.1.0-ci-test`
   (auto-detected as `--prerelease`) with the runner up; confirm it builds,
   notarizes, staples, `spctl`-verifies, and creates the release with the DMG +
   SHA-256. Delete the test release and tag afterward.

## Risks / open considerations

- **First hosted-CI run may be red** if pre-existing clippy/fmt issues exist; the
  green-on-arrival step mitigates this.
- **`notarytool` latency**: notarization can take minutes; `--wait` blocks the job.
  Acceptable for on-demand releases.
- **Runner must be online/awake** when a tag is pushed; if it is not, the release job
  queues until the runner comes up. This is inherent to the self-hosted choice and
  acceptable for a manually-cut release.
- **`gh` / `xcrun` / `nvm` availability on the runner**: all present on the release
  Mac today; `docs/ci.md` lists them as prerequisites.
```
