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
  Release with the DMG + SHA-256. **It notarizes twice, on purpose** — see
  "Why the release notarizes twice" below.

## Why self-hosted, and the security rules

The signing certificate and the `jr-notary` notarization profile live in the
release Mac's keychain, so building there keeps all secrets off GitHub.

**This repo is private** (decided in #56) and has no forks. An earlier version of
this file said "because this repo is **public**" and used that as the reason the
rules below exist — that was simply wrong, and the correction matters in both
directions: the fork-PR attack the public framing worried about cannot happen
here, and yet every rule below still stands. They are kept because of what a
self-hosted runner *is*, not because of who can see the repo:

- A self-hosted runner has **no job-level isolation**. It is a real Mac with a
  Developer ID signing key and a notarization profile in its login keychain. Any
  workflow that can schedule a job onto it can use that key.
- Visibility limits *who* can trigger a workflow. It does not limit what a
  triggered workflow can do once it lands on that Mac.
- If this repo is ever made public, the rules have to already be in place.
  Discovering them at that moment is too late.

So, unchanged and not to be weakened:

- `release.yml` triggers **only** on `v*` tag pushes and `workflow_dispatch`.
  Never `pull_request`. Both require write access, so the runner never executes
  code from an untrusted branch.
- Only `release.yml` uses the `release` runner label. Never add that label to any
  other (especially PR-triggered) workflow. **This is the rule most likely to be
  broken by accident**, it is the one that would silently hand a future
  PR-triggered workflow the signing Mac, and it does not depend on visibility at
  all.
- Run the runner **on-demand / ephemeral** — register and start it when cutting a
  release, and let it remove itself afterward. Do not leave it idling 24/7. Note
  that with `--ephemeral` "start it again" means **re-register**, not just re-run
  `./run.sh` — see "The runner is single-use" below.

## One-time setup

> **Run `scripts/release-setup.sh` instead of doing the four steps below by
> hand.** It checks each precondition before the step that needs it, refuses to
> continue when one did not take, and finishes by triggering the dry run. The
> steps are kept here as the reference for what it does. Neither your keychain
> password nor the runner registration token is written anywhere by it.

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
   `--ephemeral` makes the runner take exactly one job and then **delete its own
   registration**, which is the recommended on-demand posture. It also means this
   `config.sh` command is not one-time: it is the per-release command. See "The
   runner is single-use" below.

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
# 1. Bump the version if needed. It lives in FIVE places, and check-version.sh
#    only guards the first three:
#      Cargo.toml, package.json, src-tauri/tauri.conf.json   (edit by hand)
#      package-lock.json   -> npm install --package-lock-only
#      Cargo.lock          -> cargo check -p libretexts-reader
#    The lockfiles matter: release.yml runs `npm ci`, which fails outright when
#    package-lock.json disagrees with package.json. Then commit.
#    Verify with: scripts/ci/check-version.sh <version>
# 2. Register AND start the runner on the release Mac. Both, every release --
#    the previous run deleted the registration on its way out.
cd ~/actions-runner
./config.sh --url https://github.com/johnnylibretexts/libretexts-reader \
  --token "$(gh api -X POST repos/johnnylibretexts/libretexts-reader/actions/runners/registration-token --jq .token)" \
  --labels macos,release --name jr-release-mac --ephemeral --unattended
./run.sh            # takes one job, then exits and deregisters itself
# 3. Push the tag:
git tag v0.1.1
git push origin v0.1.1
```
The workflow builds, signs, notarizes, and publishes automatically. A tag whose
version contains a pre-release suffix (e.g. `v0.2.0-beta.1`) is published as a
GitHub **pre-release**; a plain `vX.Y.Z` becomes a full release.

## The runner is single-use

`--ephemeral` does not merely make `./run.sh` exit after one job. GitHub **removes
the runner from the repository** when that job finishes, so
`gh api repos/.../actions/runners` goes back to `{"total_count":0}` and the
install directory's `.runner` credentials are no longer valid for anything.

Re-running `./run.sh` on its own therefore does not work for a second release. The
per-release sequence is `config.sh` *then* `run.sh`, with a fresh registration
token each time (they expire in about an hour):

```bash
cd ~/actions-runner
./config.sh --url https://github.com/johnnylibretexts/libretexts-reader \
  --token "$(gh api -X POST repos/johnnylibretexts/libretexts-reader/actions/runners/registration-token --jq .token)" \
  --labels macos,release --name jr-release-mac --ephemeral --unattended
./run.sh
```

This is a deliberate trade, not an annoyance to engineer away: a runner that cannot
outlive one job cannot be reused by a later workflow, which is most of why a
self-hosted runner on a Mac holding a Developer ID key is acceptable at all. If the
re-registration ever becomes the reason releases get skipped, drop `--ephemeral`
and start/stop the runner by hand instead — but that is a weaker posture and the
security rules above still apply.

**Labels are case-insensitive.** The runner registers with GitHub's automatic
`macOS` label and `runs-on: [self-hosted, macos, release]` matches it. Trying to
add a lowercase `macos` is rejected with *"Cannot add labels that duplicate
existing read-only labels: macos"* — the auto-labels cannot be overridden, and do
not need to be.

## Dry run / testing

- `workflow_dispatch` with `dry_run: true` (Actions tab → Release → Run workflow)
  builds + Developer-ID-signs but skips notarize/publish. It **does** still run
  the pass-2 DMG rebuild and re-sign, so the dry run rehearses the artifact that
  actually ships rather than only the half that precedes it.

## Why the release notarizes twice

Tauri bundles the DMG from the `.app` *before* anything is stapled. Notarize
once and staple in the obvious order and you ship a DMG whose inner `.app` —
the only copy anyone installs — has no ticket:

```
$ xcrun stapler validate "/Volumes/LibreTexts Reader/LibreTexts Reader.app"
LibreTexts Reader.app does not have a ticket stapled to it.
```

Gatekeeper still accepts it by fetching the ticket over the network, so this is
invisible until a tester's **first launch is offline** and they get "cannot be
verified". `release.yml` therefore runs: notarize → staple the `.app` → rebuild
the DMG around it → **codesign** → notarize → staple.

Two traps live in those steps, both load-bearing:

- **`spctl` uses the network**, so it reports `accepted / Notarized Developer ID`
  for an unstapled app and cannot detect this at all. Only `stapler validate`
  can. The "Verify the shipped artifact" step runs both, and validates the `.app`
  *inside* the mounted DMG so the bug cannot silently return.
- **`tauri:build` codesigns the `.dmg`; `bundle_dmg.sh` does not.** The rebuilt
  image must be signed before it is notarized — signing afterwards rewrites the
  file and voids the ticket. An unsigned DMG notarizes and staples happily and
  then fails `spctl` with `no usable signature`.

See #102 and `RELEASE.md` §2, which documents the same sequence for the manual
fallback path.
- Full end-to-end test: push a throwaway `v0.0.0-citest` tag (auto-detected as a
  pre-release) with the runner up, verify the release is created, then delete the
  test release and tag.
