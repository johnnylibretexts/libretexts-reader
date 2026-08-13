# CI + Release Automation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add hosted CI on PRs/pushes and a self-hosted, tag-triggered workflow that builds, signs, notarizes, staples, and publishes the macOS DMG automatically.

**Architecture:** Two GitHub Actions workflows under `.github/workflows/`. `ci.yml` runs on GitHub-hosted `macos-14` for every PR and push to `main` (fmt/clippy/test/frontend build). `release.yml` runs on a self-hosted Apple-Silicon runner (label `release`), triggered only by `v*` tag pushes (plus a manual dry-run), and automates the exact `RELEASE.md` sequence: materialize native libs via `build.rs`, Developer-ID re-sign them, `tauri build`, notarize with the local `jr-notary` profile, staple, and publish a GitHub Release. A small `check-version.sh` guards tag↔version consistency.

**Tech Stack:** GitHub Actions, Bash, Node 22.20.0, Rust (stable, pinned by `rust-toolchain.toml`), Tauri 2, `xcrun notarytool`/`stapler`, `codesign`, `gh` CLI.

## Global Constraints

- Node version is exactly **22.20.0** (via `actions/setup-node` in hosted CI; via `nvm use 22.20.0` on the self-hosted runner).
- Rust toolchain comes from `rust-toolchain.toml` (stable, with `clippy` + `rustfmt`). Do not hardcode a different channel.
- Version is declared in three files and must stay equal: `Cargo.toml` (`[workspace.package].version`), `package.json` (`version`), `src-tauri/tauri.conf.json` (`version`). Currently all `0.1.0`.
- **Commits in this repo MUST NOT include a `Co-Authored-By` trailer** (single-author history).
- Release builds are **aarch64 only**. No Intel/universal, no Windows/Linux packaging.
- **Security (self-hosted runner on a public repo):** `release.yml` triggers only on `push` tags `v*` and `workflow_dispatch` — never `pull_request`. It is the only workflow allowed to use the `release` runner label. `ci.yml` uses GitHub-hosted runners only. Include `if: github.repository == 'johnnyrobot/johnny-reader'` on the release job.
- **No GitHub secrets** for signing/notarization. The only Actions config value is a non-secret repo **variable** `APPLE_SIGNING_IDENTITY`. The release job authenticates to GitHub with the auto-provided `GITHUB_TOKEN` (needs `contents: write`).
- DMG output path: `target/release/bundle/dmg/Johnny Reader_<version>_aarch64.dmg`.
- Spec: `docs/superpowers/specs/2026-07-17-ci-release-automation-design.md`.

---

### Task 1: Establish a green fmt/clippy/test/build baseline

Hosted CI will gate on `cargo clippy --all-targets -- -D warnings` and `cargo fmt --all --check`. Before adding that gate, make those commands pass locally so CI is not red on arrival.

**Files:**
- Modify: only source files that fmt/clippy flag (unknown until run; keep changes minimal and mechanical).

**Interfaces:**
- Consumes: nothing.
- Produces: a repo state where `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test -p johnny-reader`, and `npm run build` all succeed. Later tasks (Task 3) rely on these commands being green.

- [ ] **Step 1: Set up the shell environment**

Run:
```bash
cd /Users/laccd/code/johnny-reader
source "$HOME/.nvm/nvm.sh" && nvm use 22.20.0
. "$HOME/.cargo/env"
```
Expected: `Now using node v22.20.0`.

- [ ] **Step 2: Frontend build (baseline)**

Run: `npm ci && npm run build`
Expected: PASS (tsc + vite build with no errors).

- [ ] **Step 3: Format check (observe current state)**

Run: `cargo fmt --all --check`
Expected: either clean (exit 0) or a diff. If it reports a diff, apply it: `cargo fmt --all`.

- [ ] **Step 4: Clippy at CI strictness (observe current state)**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: either clean or a list of warnings-as-errors. If warnings appear, fix them minimally and idiomatically (prefer the clippy-suggested change). Do not silence with blanket `#[allow(...)]` unless a warning is a known false positive — if so, scope the `allow` to the smallest item and add a one-line comment saying why.

- [ ] **Step 5: Rust tests**

Run: `cargo test -p johnny-reader`
Expected: PASS. (The `#[ignore]`d live-import tests are not run.)

- [ ] **Step 6: Re-verify all four are green**

Run:
```bash
cargo fmt --all --check && \
cargo clippy --all-targets -- -D warnings && \
cargo test -p johnny-reader && \
npm run build
```
Expected: all exit 0.

- [ ] **Step 7: Commit (only if fixes were needed)**

If Steps 3–5 changed files:
```bash
git add -A
git commit -m "chore: satisfy cargo fmt + clippy -D warnings for CI gate"
```
If nothing changed, record in the task notes: "baseline already green, no commit." (Remember: no `Co-Authored-By` trailer.)

---

### Task 2: Version-consistency check script (`check-version.sh`)

A small, unit-tested script the release workflow uses to fail fast when a pushed tag disagrees with the configured version.

**Files:**
- Create: `scripts/ci/check-version.sh`
- Create (test): `scripts/ci/check-version.test.sh`

**Interfaces:**
- Consumes: nothing.
- Produces: `scripts/ci/check-version.sh <expected-version>` — reads `.version` from `src-tauri/tauri.conf.json` and `package.json`, and the `[workspace.package].version` from `Cargo.toml`; exits `0` iff all three equal `<expected-version>` (no leading `v`), else non-zero with a mismatch message on stderr. Honors a `ROOT` env var (default `git rev-parse --show-toplevel`) so it is testable against fixtures. Task 4's `release.yml` calls `scripts/ci/check-version.sh "$VERSION"`.

- [ ] **Step 1: Write the failing test**

Create `scripts/ci/check-version.test.sh`:
```bash
#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
script="$here/check-version.sh"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/src-tauri"
printf '{ "version": "1.2.3" }\n' > "$tmp/src-tauri/tauri.conf.json"
printf '{ "version": "1.2.3" }\n' > "$tmp/package.json"
printf '[workspace.package]\nversion = "1.2.3"\n' > "$tmp/Cargo.toml"

# Case 1: all sources match expected -> exit 0
ROOT="$tmp" bash "$script" 1.2.3 >/dev/null
echo "PASS: matching versions accepted"

# Case 2: one source mismatches -> non-zero
printf '{ "version": "9.9.9" }\n' > "$tmp/package.json"
if ROOT="$tmp" bash "$script" 1.2.3 >/dev/null 2>&1; then
  echo "FAIL: mismatch not detected" >&2; exit 1
fi
echo "PASS: mismatched source rejected"

# Case 3: expected differs from (consistent) sources -> non-zero
printf '{ "version": "1.2.3" }\n' > "$tmp/package.json"
if ROOT="$tmp" bash "$script" 0.0.0 >/dev/null 2>&1; then
  echo "FAIL: wrong expected not detected" >&2; exit 1
fi
echo "PASS: wrong expected rejected"

echo "ALL TESTS PASSED"
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `bash scripts/ci/check-version.test.sh`
Expected: FAIL — the script does not exist yet (e.g. `check-version.sh: No such file or directory`).

- [ ] **Step 3: Write the script**

Create `scripts/ci/check-version.sh`:
```bash
#!/usr/bin/env bash
# Verify the given version matches every version source in the repo.
# Usage: check-version.sh <expected-version>   (e.g. 0.1.0, no leading "v")
# Env:   ROOT  repo root (default: git rev-parse --show-toplevel)
set -euo pipefail

expected="${1:?usage: check-version.sh <expected-version>}"
ROOT="${ROOT:-$(git rev-parse --show-toplevel)}"

read_json_version() { # $1 = path to a JSON file with a top-level "version"
  node -e "process.stdout.write(String(require('$1').version))"
}

tauri_v="$(read_json_version "$ROOT/src-tauri/tauri.conf.json")"
pkg_v="$(read_json_version "$ROOT/package.json")"
# First `version = "x"` line after the [workspace.package] header.
cargo_v="$(awk '/^\[workspace\.package\]/{f=1} f && /^version[[:space:]]*=/{gsub(/[",]/,"",$3); print $3; exit}' "$ROOT/Cargo.toml")"

fail=0
for pair in "tauri.conf.json:$tauri_v" "package.json:$pkg_v" "Cargo.toml:$cargo_v"; do
  name="${pair%%:*}"; val="${pair#*:}"
  if [ "$val" != "$expected" ]; then
    echo "version mismatch: $name has '$val', expected '$expected'" >&2
    fail=1
  fi
done

if [ "$fail" -eq 0 ]; then
  echo "version OK: tauri.conf.json, package.json, Cargo.toml all == $expected"
fi
exit "$fail"
```

- [ ] **Step 4: Make executable and run the test to verify it passes**

Run:
```bash
chmod +x scripts/ci/check-version.sh scripts/ci/check-version.test.sh
bash scripts/ci/check-version.test.sh
```
Expected: prints three `PASS:` lines and `ALL TESTS PASSED`.

- [ ] **Step 5: Sanity-check against the real repo**

Run: `scripts/ci/check-version.sh 0.1.0 && ! scripts/ci/check-version.sh 9.9.9`
Expected: first prints `version OK: … == 0.1.0` (exit 0); second prints a mismatch line and returns non-zero (the `!` makes the compound command succeed).

- [ ] **Step 6: Commit**

```bash
git add scripts/ci/check-version.sh scripts/ci/check-version.test.sh
git commit -m "ci: add version-consistency check script with tests"
```

---

### Task 3: Hosted CI workflow (`ci.yml`)

**Files:**
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: the green baseline from Task 1 (the four commands must pass).
- Produces: a CI check named `verify` running on PRs and pushes to `main`.

- [ ] **Step 1: Ensure actionlint is available (workflow linter = our "test")**

Run: `command -v actionlint || brew install actionlint`
Expected: `actionlint` on PATH. (This is the local proxy for "the workflow is valid" — a real end-to-end check happens when the workflow runs on GitHub in Step 5.)

- [ ] **Step 2: Write the workflow**

Create `.github/workflows/ci.yml`:
```yaml
name: CI

on:
  pull_request:
  push:
    branches: [main]

concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: true

jobs:
  verify:
    runs-on: macos-14
    steps:
      - uses: actions/checkout@v4

      - uses: actions/setup-node@v4
        with:
          node-version: '22.20.0'

      - name: Install Rust toolchain (from rust-toolchain.toml)
        run: rustup show

      - name: Install npm dependencies
        run: npm ci

      - name: Frontend build
        run: npm run build

      - name: Rust format check
        run: cargo fmt --all --check

      - name: Clippy
        run: cargo clippy --all-targets -- -D warnings

      - name: Rust tests
        run: cargo test -p johnny-reader
```

- [ ] **Step 3: Lint the workflow (verify it "fails" then "passes")**

Run: `actionlint .github/workflows/ci.yml`
Expected: no output (exit 0). If actionlint reports errors, fix them and re-run until clean.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add hosted fmt/clippy/test/build workflow"
```

- [ ] **Step 5: End-to-end verification on GitHub (requires push)**

Push the branch and open a PR (or push to a branch that triggers CI). Confirm the `verify` job runs on `macos-14` and all steps pass:
```bash
git push -u origin ci-release-automation
gh pr create --fill --base main --head ci-release-automation
gh pr checks --watch
```
Expected: the CI `verify` check completes green. If a step fails, fix locally, commit (no trailer), push, re-watch. (Note: the Rust compile downloads pdfium/ffmpeg via `build.rs`; this is expected and needs the runner's network.)

---

### Task 4: Self-hosted release workflow (`release.yml`)

Automates the `RELEASE.md` sequence. This task authors the workflow; the runner-dependent end-to-end run is Task 6.

**Files:**
- Create: `.github/workflows/release.yml`

**Interfaces:**
- Consumes: `scripts/ci/check-version.sh` (Task 2); the repo variable `vars.APPLE_SIGNING_IDENTITY` and the `release`-labelled self-hosted runner (provisioned in Task 5).
- Produces: on a `v*` tag push, a published GitHub Release with the signed+notarized DMG and a SHA-256 in the notes; on `workflow_dispatch` with `dry_run: true`, a built+signed (not notarized, not published) bundle.

- [ ] **Step 1: Write the workflow**

Create `.github/workflows/release.yml`:
```yaml
name: Release

on:
  push:
    tags: ['v*']
  workflow_dispatch:
    inputs:
      dry_run:
        description: 'Build + Developer-ID sign, but skip notarize/publish'
        type: boolean
        default: true

concurrency:
  group: release-${{ github.ref }}
  cancel-in-progress: false

permissions:
  contents: write

jobs:
  release:
    if: github.repository == 'johnnyrobot/johnny-reader'
    runs-on: [self-hosted, macos, release]
    env:
      APPLE_SIGNING_IDENTITY: ${{ vars.APPLE_SIGNING_IDENTITY }}
      GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
    steps:
      - uses: actions/checkout@v4

      - name: Resolve version, tag, and mode
        id: meta
        run: |
          set -euo pipefail
          if [ "${{ github.event_name }}" = "push" ]; then
            TAG="${GITHUB_REF_NAME}"
            VERSION="${TAG#v}"
            DRY_RUN="false"
          else
            VERSION="$(node -e "process.stdout.write(String(require('./src-tauri/tauri.conf.json').version))")"
            TAG="v${VERSION}"
            DRY_RUN="${{ github.event.inputs.dry_run }}"
          fi
          {
            echo "tag=$TAG"
            echo "version=$VERSION"
            echo "dry_run=$DRY_RUN"
            # semver pre-release (a hyphen after the version core) -> mark prerelease
            if [[ "$VERSION" == *-* ]]; then echo "prerelease=true"; else echo "prerelease=false"; fi
          } >> "$GITHUB_OUTPUT"

      - name: Verify tag matches configured version
        if: github.event_name == 'push'
        run: scripts/ci/check-version.sh "${{ steps.meta.outputs.version }}"

      - name: Install npm dependencies (Node 22.20.0)
        run: |
          set -euo pipefail
          source "$HOME/.nvm/nvm.sh"
          nvm use 22.20.0
          npm ci

      - name: Materialize native libs via build.rs
        run: |
          set -euo pipefail
          source "$HOME/.cargo/env"
          cargo build --release -p johnny-reader

      - name: Developer-ID re-sign native libs
        run: |
          set -euo pipefail
          ID="$APPLE_SIGNING_IDENTITY"
          if [ -z "$ID" ]; then echo "APPLE_SIGNING_IDENTITY variable is not set" >&2; exit 1; fi
          # ffmpeg dylibs (under binaries/<sidecar>-libs/) + any other bundled dylibs
          find src-tauri/binaries -type f -name '*.dylib' \
            -exec codesign --force --options runtime --timestamp --sign "$ID" {} \;
          # pdfium dylib(s)
          for lib in src-tauri/resources/pdfium/*/libpdfium.dylib; do
            codesign --force --options runtime --timestamp --sign "$ID" "$lib"
          done
          # ffmpeg sidecar executable (exact target name; not the -libs dir or .sha256 markers)
          codesign --force --options runtime --timestamp --sign "$ID" \
            src-tauri/binaries/ffmpeg-aarch64-apple-darwin

      - name: Build signed bundle
        run: |
          set -euo pipefail
          source "$HOME/.nvm/nvm.sh"; nvm use 22.20.0
          source "$HOME/.cargo/env"
          export APPLE_SIGNING_IDENTITY
          export JOHNNY_READER_REQUIRE_UPDATER_KEY=1   # fail-closed safety belt (no-op today)
          npm run tauri:build

      - name: Locate DMG
        id: dmg
        run: |
          set -euo pipefail
          DMG="target/release/bundle/dmg/Johnny Reader_${{ steps.meta.outputs.version }}_aarch64.dmg"
          test -f "$DMG"
          echo "path=$DMG" >> "$GITHUB_OUTPUT"

      - name: Notarize, staple, verify
        if: steps.meta.outputs.dry_run != 'true'
        run: |
          set -euo pipefail
          DMG="${{ steps.dmg.outputs.path }}"
          xcrun notarytool submit "$DMG" --keychain-profile jr-notary --wait
          xcrun stapler staple "$DMG"
          xcrun stapler staple "target/release/bundle/macos/Johnny Reader.app"
          spctl -a -t open --context context:primary-signature -vvv "$DMG"

      - name: Publish GitHub Release
        if: steps.meta.outputs.dry_run != 'true' && github.event_name == 'push'
        run: |
          set -euo pipefail
          DMG="${{ steps.dmg.outputs.path }}"
          BASENAME="$(basename "$DMG")"
          SHA="$(shasum -a 256 "$DMG" | awk '{print $1}')"
          # Commit-based notes via the API, then append the checksum.
          gh api -X POST "repos/${GITHUB_REPOSITORY}/releases/generate-notes" \
            -f tag_name="${{ steps.meta.outputs.tag }}" --jq '.body' > notes.md \
            || echo "Release ${{ steps.meta.outputs.tag }}" > notes.md
          printf '\n\n---\nSHA-256 (%s):\n`%s`\n' "$BASENAME" "$SHA" >> notes.md
          PRERELEASE=""
          if [ "${{ steps.meta.outputs.prerelease }}" = "true" ]; then PRERELEASE="--prerelease"; fi
          gh release create "${{ steps.meta.outputs.tag }}" "$DMG" \
            --title "Johnny Reader ${{ steps.meta.outputs.tag }}" \
            --notes-file notes.md $PRERELEASE
```

- [ ] **Step 2: Lint the workflow**

Run: `actionlint .github/workflows/release.yml`
Expected: no output (exit 0). Fix any reported issues and re-run until clean.
Note: actionlint runs `shellcheck` on `run:` blocks if shellcheck is installed (`brew install shellcheck`) — address any warnings it surfaces (quoting, `set -euo pipefail` already present).

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci: add self-hosted tag-triggered release workflow"
```

(End-to-end validation of this workflow is Task 6 — it needs the runner and a tag.)

---

### Task 5: Runner provisioning docs + RELEASE.md update

Documents the one-time manual setup (runner registration, signing-identity variable, keychain authorization) and repoints `RELEASE.md` at the automated flow. Nothing here is committable code the workflow imports; it is the operator guide the workflows depend on.

**Files:**
- Create: `docs/ci.md`
- Modify: `RELEASE.md`

**Interfaces:**
- Consumes: the workflows from Tasks 3–4 (documents how to operate them).
- Produces: operator documentation. No code interface.

- [ ] **Step 1: Write `docs/ci.md`**

Create `docs/ci.md` covering exactly these sections (fill with the real values below):

````markdown
# CI & Release Automation

Two workflows live in `.github/workflows/`:

- **`ci.yml`** — runs on GitHub-hosted `macos-14` for every pull request and push
  to `main`: `npm run build`, `cargo fmt --all --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test -p johnny-reader`.
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

1. **Register the self-hosted runner** (Settings → Actions → Runners → New
   self-hosted runner, macOS/arm64). During `./config.sh`, give it the labels
   `macos` and `release`:
   ```bash
   ./config.sh --url https://github.com/johnnyrobot/johnny-reader \
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
````

Replace `<Your Name>`, `<TEAMID>`, `<REG_TOKEN>`, and `<login-keychain-password>` placeholders with the real values (the identity/team come from `security find-identity -v -p codesigning`; the registration token from the runner setup page).

- [ ] **Step 2: Update `RELEASE.md` to lead with the automated flow**

At the top of `RELEASE.md`, add an "Automated release (preferred)" section before the existing manual checklist:
```markdown
## Automated release (preferred)

Push a `vX.Y.Z` tag with the self-hosted runner up and CI does everything below
automatically — build, Developer-ID signing of the bundled ffmpeg/pdfium libs,
`tauri build`, notarization via the `jr-notary` profile, stapling, and publishing
the GitHub Release with the DMG + SHA-256. See `docs/ci.md` for runner setup and
the exact steps. The manual checklist below is the fallback and documents what the
workflow runs under the hood.
```
Leave the existing manual sections intact beneath it.

- [ ] **Step 3: Verify docs render and links resolve**

Run:
```bash
test -f docs/ci.md && grep -q "Automated release (preferred)" RELEASE.md && echo OK
```
Expected: `OK`.

- [ ] **Step 4: Commit**

```bash
git add docs/ci.md RELEASE.md
git commit -m "docs: document CI/release automation and runner setup"
```

---

### Task 6: End-to-end pipeline validation (runner-dependent)

Final acceptance. This task is an operator procedure, not committable code; it needs the self-hosted runner online and repo settings from Task 5. Record results in the PR.

**Files:** none (validation only).

**Interfaces:**
- Consumes: everything from Tasks 1–5, plus the provisioned runner + `APPLE_SIGNING_IDENTITY` variable.
- Produces: confirmation that both workflows work end-to-end.

- [ ] **Step 1: Confirm hosted CI is green** (from Task 3 Step 5 — the PR's `verify` check passes).

- [ ] **Step 2: Dry-run the release workflow**

Start the runner (`./run.sh`), then:
```bash
gh workflow run release.yml -f dry_run=true
gh run watch "$(gh run list --workflow=release.yml --limit 1 --json databaseId --jq '.[0].databaseId')"
```
Expected: job reaches the "Build signed bundle" + "Locate DMG" steps and succeeds; "Notarize" and "Publish" steps are **skipped** (dry-run). Confirm the DMG exists in the run's workspace.

- [ ] **Step 3: Full end-to-end with a throwaway tag**

With the runner up:
```bash
git tag v0.0.0-citest
git push origin v0.0.0-citest
gh run watch "$(gh run list --workflow=release.yml --limit 1 --json databaseId --jq '.[0].databaseId')"
```
Expected: build → notarize (`accepted`) → staple → `spctl … accepted` → a **pre-release** `v0.0.0-citest` created with the DMG asset and a SHA-256 in the notes.

- [ ] **Step 4: Verify the published artifact**

```bash
gh release view v0.0.0-citest
gh release download v0.0.0-citest --dir /tmp/jr-citest
spctl -a -t open --context context:primary-signature -vvv "/tmp/jr-citest/Johnny Reader_0.0.0-citest_aarch64.dmg"
```
Expected: `spctl` reports `accepted` / `Notarized Developer ID`, and the SHA-256 in the release notes matches `shasum -a 256` of the downloaded DMG.

- [ ] **Step 5: Clean up the test release and tag**

```bash
gh release delete v0.0.0-citest --yes
git push origin :refs/tags/v0.0.0-citest
git tag -d v0.0.0-citest
```
Expected: test release and tag removed.

- [ ] **Step 6: Record results** in the PR description (CI green; dry-run OK; E2E notarized + published + cleaned up).

---

## Self-Review

**Spec coverage:**
- Two workflows (`ci.yml` hosted, `release.yml` self-hosted) → Tasks 3, 4. ✓
- Security model (tag-only trigger, `release` label, on-demand runner, repo guard) → Task 4 workflow + Task 5 docs + Global Constraints. ✓
- Tag↔version gate → Task 2 (script) + Task 4 (invocation). ✓
- Native-lib materialize + Developer-ID re-sign + tauri build ordering → Task 4 steps. ✓
- Notarize/staple/verify via local `jr-notary` → Task 4. ✓
- Publish full release, semver-prerelease auto-flip, SHA-256 in notes → Task 4. ✓
- No GitHub secrets; `APPLE_SIGNING_IDENTITY` as a variable → Global Constraints + Task 4 env + Task 5 docs. ✓
- Green-on-arrival before `-D warnings` gate → Task 1. ✓
- `docs/ci.md` + `RELEASE.md` update → Task 5. ✓
- Dry-run + throwaway-tag testing → Task 4 (`workflow_dispatch`/skips) + Task 6. ✓
- Out-of-scope (Intel/universal, updater, Win/Linux) → Global Constraints. ✓
- No `Co-Authored-By` trailer → Global Constraints + every commit step. ✓

**Placeholder scan:** The only `<...>` tokens are runtime/operator values (identity, team id, tokens, keychain password) that are inherently machine/secret-specific and are documented as such — not plan gaps. No "TBD/TODO".

**Type consistency:** `scripts/ci/check-version.sh <expected-version>` with `ROOT` env is defined in Task 2 and called identically in Task 4. DMG path string, `APPLE_SIGNING_IDENTITY`, and the `steps.meta.outputs.*` names are consistent across Task 4 steps. The ffmpeg sidecar name `ffmpeg-aarch64-apple-darwin` matches `externalBin: binaries/ffmpeg` + target triple.
