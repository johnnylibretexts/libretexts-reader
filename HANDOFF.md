# LibreTexts Reader Handoff

Last updated: 2026-08-23

> **The private beta is cut.** [**v0.1.0-beta.1**](https://github.com/johnnylibretexts/libretexts-reader/releases/tag/v0.1.0-beta.1)
> is published as a GitHub pre-release — signed, notarized, stapled, and produced by
> `release.yml` running unattended from a tag. The **`Private beta` milestone is 11/11
> closed**. Hand the DMG to fewer than ten named testers; the repo stays private.
>
> **The release pipeline works — say so.** Every older passage claiming otherwise has been
> corrected, but if you find one that was missed, it is stale, not news. See "Release: the
> pipeline ran itself" below.
>
> Seven issues remain open and **none of them gate anything**: #59, #61, #65, #57, #63, #64,
> #69. The most tester-visible is #59 — playback position is written but never read back, so
> there is no resume and every progress bar sits at zero.

This repo is an in-progress Tauri desktop app for reading and listening to OpenStax, LibreTexts, EPUB, PDF, pasted text, and article imports with local TTS.

**Pressbooks is a third content Source, shipped 2026-08-18.** [PR #35](https://github.com/johnnylibretexts/libretexts-reader/pull/35) merged 15 commits as `aaf0e7d`, closing the epic #19 and all eight of its children. A reader can browse Pressbooks Catalogs with a picker, search a crawled cache as they type, add a book in one click, see its cover in the Library, and hear its equations — Pressbooks renders those to images and keeps the LaTeX in the `alt`, which import recovers as a `[[latex:<base64>]]` token that KaTeX typesets and the speech path says aloud.

One follow-up fix merged behind it: [PR #36](https://github.com/johnnylibretexts/libretexts-reader/pull/36) (`0532eb3`) names the SQLite busy timeout the app relies on rather than inheriting it from rusqlite.

And [PR #37](https://github.com/johnnylibretexts/libretexts-reader/pull/37) (`0a6ea2d`) closed #31. An image download used to accept a response if *either* its content type or its URL extension looked like an image, so a WAF block page served `200 text/html` for `.../cover.png` was written to disk as a PNG and hung on the Library card. It now decides on the body — sniffed magic bytes — or on what the server said, never on the URL alone. Both signals have to stay, and the PR body says why: servers that will not guess send a genuine PNG as `application/octet-stream`, and a format the sniffer does not know is still an image when the server says so.

**Five open tickets closed in one session, 2026-08-18/19: #33, #29, #28, #34, #32 — all merged to `main`.**

- **#33** ([PR #39](https://github.com/johnnylibretexts/libretexts-reader/pull/39), `3fe7a5c`) — `word_count` on a Pressbooks TOC entry is now `Option<u32>`. `push_readable` gates on `has_post_content` alone and only excludes a *measured* zero, so a Catalog that never sends the field no longer imports as an empty book.
- **#29** ([PR #40](https://github.com/johnnylibretexts/libretexts-reader/pull/40), `81177af`) — `verify_offered_book_url` now also rejects a non-`https` scheme, an explicit port, and embedded userinfo, not just an unoffered host.
- **#28** ([PR #41](https://github.com/johnnylibretexts/libretexts-reader/pull/41), `372bf3b`) — `source_page_cache` (shared by LibreTexts and Pressbooks since migration `0008`; the `libretexts_cache` table the issue named no longer exists) now expires a read after a 7-day TTL, and `delete_document` clears a LibreTexts Document's cached pages when the Document is deleted. **Landed narrower than the issue asked, on purpose**: there is no cheap way to revalidate via `content_revision` without a full fetch (unlike OpenStax's `archive_release` manifest), so this is TTL-only rather than the revalidate-then-TTL policy the issue floated. Don't reopen that as a gap without a lightweight revision-check endpoint to build it on.
- **#34** ([PR #42](https://github.com/johnnylibretexts/libretexts-reader/pull/42), `71ed16c`) — the Pressbooks `catalog-progress` listener effect was declared *after* the effect that starts the crawl. `crawl_catalog` reports progress synchronously before issuing any request, so the first event was always emitted into a window with no subscriber. Fixed by swapping the declaration order — React runs effects in declaration order.
- **#32** ([PR #43](https://github.com/johnnylibretexts/libretexts-reader/pull/43), `5990b87`) — the flaky test was `PressbooksBrowser.test.tsx > search > "answers a search typed while the first catalog is still loading"`. Reproduced on iteration 5 of an 80-run loop of the *full* suite; 40 runs of just the two files the issue suspected never reproduced it in isolation — the race needs the full suite's CPU contention, matching the issue's own observation. Root cause: a Catalog's arrival renders both its books from the raw listing for one tick before the search resolves and narrows them; the test's `waitFor` polled for a book title present in *both* that transient tick and the final state, then a synchronous check right after raced the search's still-pending promise. Fixed by folding the negative assertion into the same `waitFor` callback instead of lengthening any timeout. A sibling test had the identical shape and the identical latent race and was fixed the same way, though it never itself reproduced. **Verified: 100 consecutive full-suite runs, zero failures**, after 0 in the first 40 isolated-file runs and a hit on run 5 of 80 full-suite runs — the isolation detail is worth keeping if this class of flake resurfaces elsewhere.

**One new ticket, opened and fixed in the same session: #44**, filed after PR #43's own CI run hung for the full 6-hour GitHub Actions per-job cap before being force-cancelled — on a two-line test-assertion diff that explains nothing about a 6-hour hang. The culprit step, `Install Linux build dependencies` (a plain `apt-get update && apt-get install`, `.github/workflows/ci.yml:52`), timed between 47 seconds and 28 minutes across other runs and, on that one run, indefinitely. [PR #45](https://github.com/johnnylibretexts/libretexts-reader/pull/45) (`ed54958`) adds `timeout-minutes: 60` to the `verify` job, and **merged after its own CI run passed in 9m56s** — comfortably inside the new bound, so 60 minutes does not clip a normal run.

**Verified in the real app on 2026-08-20, after all six landed.** `npm run tauri -- build --debug --no-bundle` built in 1m05s, and `./target/debug/libretexts-reader` launched, ran, and quit cleanly with no crash report and empty stderr. The app was driven, not merely launched, and it opened on Pressbooks — the surface four of the six fixes touched: the Catalog showed its progress indicator and then listed 90 Milne books with covers, authors and licences, and typing `logic` narrowed to the single match with no stale cards left behind. That last one is the exact behaviour #32's flaky test asserts, working live.

Two incidental facts from that run, neither a defect but both able to waste an hour:

- **The window opens on a second display when one is attached** (`Y = -1257` in `CGWindowList` terms). A `screencapture` of the primary display comes back showing no app at all, which reads exactly like a failure to launch. Capture the window by id instead — find it with `CGWindowListCopyWindowInfo` filtered on the pid, then `screencapture -o -l <window-id>`.
- **`osascript`/System Events cannot drive this binary.** A `--no-bundle` build has no bundle id (`bundleID=[ NULL ]` in `lsappinfo`), so `tell application "libretexts-reader"` fails with `-1728`, and System Events additionally needs assistive access this environment does not grant (`-25211`). `NSRunningApplication(processIdentifier:).activate` from a `swift -e` one-liner works and needs no permission; `CGEvent` posts clicks and keystrokes to the frontmost app the same way.

**There is no other work in progress as of this update.** The standing caution against `git reset --hard` applies to any new WIP.

The Fish Audio provider (spec B) merged on 2026-08-16 via [PR #4](https://github.com/johnnylibretexts/libretexts-reader/pull/4) — 39 commits, merge commit `64ead91`, reviewed with all 15 findings resolved.

**#30 is closed as not reproducible**, and the reasoning is worth not repeating: it asserted that no `busy_timeout` was set, but `rusqlite` applies 5000ms to every connection it opens, and the test the ticket demanded passed against unmodified code. **Reproduce a ticket's defect before building for it** — write the test it asks for and watch it fail first. `/code-review` findings on this repo have twice been wrong about their own premise.

**CI is green again.** It had been red on `main` since 2026-08-14 — not from any code defect, but because `check-app-data-isolation` wrapped `cargo test`, and the `ort` crate's build script caches a ~73MB `libonnxruntime.a` under `$HOME/Library/Caches`. That is a toolchain artifact, not app data, but a *cold* build inside the throwaway `$HOME` tripped the check. It reproduced only on machines that had never compiled before, so it was green for every developer and red on every runner, and the failure text blamed a `paths::` leak that did not exist. The script now compiles before creating the sandbox. **If this check ever fails again, read the leaked paths before believing the message** — it names `paths::` regardless of what actually wrote.

**CI runs on `ubuntu-latest` and the billing block is resolved** (2026-08-17). Two
things happened: the account moved to GitHub Pro, and the `verify` job moved off
macOS — see "Moving CI off macOS" below for what that cost to land.
`release.yml` stays on `macos-14`; it codesigns the `.app` and the bundled dylibs.

Keep the diagnostic, because the failure mode can recur: **a job that fails in ~3
seconds with *"The job was not started because recent account payments have failed
or your spending limit needs to be increased"* is billing, not code.** Check the run
annotation before debugging anything. The limit lives under Settings → Billing &
plans. What made it structural was the `macos-14` **10× multiplier** — a 4–13 minute
run cost 40–130 billable minutes. On Linux the same run bills at 1×.

## Project Location

Work from wherever you cloned the repo, for example:

```bash
cd "$HOME/code/libretexts-reader"
```

If this folder is copied to another machine, run setup from the copied repo root.

## Required Tooling

- Node/npm. Last verified with Node `22.20.0` and npm `10.9.3`.
- Rust via rustup. `rust-toolchain.toml` pins the stable toolchain and includes `clippy` and `rustfmt`.
- macOS needs Xcode Command Line Tools for local Tauri builds.
- Network access is needed on first build for Rust crates and for `src-tauri/build.rs` to prepare bundled PDFium/ffmpeg assets if they are not already present.

## Setup From A Fresh Copy

```bash
npm install
cargo check -p libretexts-reader
npm run build
```

For a runnable desktop binary without packaging installers:

```bash
npm run tauri -- build --debug --no-bundle
./target/debug/libretexts-reader
```

Run the binary **directly**. `--no-bundle` produces a bare Mach-O with no `.app`, and `open` on it can exit 0 while starting nothing — no window, no process, no error.

For live development:

```bash
npm run tauri:dev
```

If the copied folder includes `node_modules`, `target`, or `src-tauri/resources/pdfium` and `src-tauri/binaries`, setup may be faster, but the canonical setup is still `npm install` plus the build commands above.

## App Data

The app database and downloaded models/images are not stored in the repo. On macOS the app data directory is:

```bash
~/Library/Application Support/dev.johnnylibretexts.reader
```

Copying only the project folder will not copy the local library, downloaded books, TTS models, cover images, or downloaded section images. To preserve a test library across machines, copy that app data directory too. The app can also use `LIBRETEXTS_READER_APP_DATA_DIR` to point tests or local runs at a temporary data directory.

## Recently Landed

### Session of 2026-08-22 (later) — the last two non-human release blockers, and a real signed DMG

Three PRs, three issues closed. `main` at `fa7829c`.

**#54 → [PR #99](https://github.com/johnnylibretexts/libretexts-reader/pull/99)** — Fish playback
billed the reader with no warning and no way to stop it. **Its premise was verified first, and
one claim was wrong**: "Pausing does not stop the charge" is overbroad — `fillSpeechBuffer`'s
workers re-check the utterance token between sentences, so Pause already abandoned the queued
prefetch; only the ≤2 in flight still billed. Every line reference except `fishEngine.ts:16-36`
had drifted.

**Two money bugs the audit missed, both reproduced before fixing:**

- `throwIfAborted` ran *after* `invoke` returned, so a request that completed and was charged
  threw, and `cachedSpeechBlob`'s catch deleted the cache entry — resuming bought the same
  sentence again. A request that reached a billing engine is paid for; keep it.
- `cancelSpeech` set `speechAbort = null`, making `speechAbort?.signal` **undefined** for
  anything issued afterwards. A request started after Pause ran with no signal at all.

Shipped: a confirm gate on the provider picker, read-ahead capped at 3 for a billing engine
(10 stays for free ones), Pause reachable while buffering, and a standing disclosure beside the
API key field. `SPEECH_ENGINE_BILLS` is the one declaration of whether an engine costs money.
**A Rust cancellation channel was deliberately not built** — Fish bills on generation, so
dropping the connection mid-request leaves the reader charged *and* without the audio.

**#50 → [PR #100](https://github.com/johnnylibretexts/libretexts-reader/pull/100) and
[PR #101](https://github.com/johnnylibretexts/libretexts-reader/pull/101)** — licence
compliance. **No LGPL component is bundled any more.**

- **ffmpeg was bundled and never used, ever.** `git log --all -S"ffmpeg" -- src-tauri/src/`
  returns nothing. It arrived with the initial import; CLAUDE.md's claim that it "handles
  encoding" was never true. The repo had already written the fact down twice
  (`verify.yml:152`, `check-ffmpeg-bundle.sh`) and each time responded by adding infrastructure
  to verify the unused binary loaded. Removing it: 14 functions and ~390 lines from `build.rs`,
  the `xz2` build-dep, two CI scripts. **DMG 40 MB → 15 MB, `.app` 88 MB → 35 MB.**
- **`LICENSES/` never reached the bundle**, because `bundle.resources` resolves relative to
  `src-tauri/`. `mirror_licenses_into_bundle` now copies on *every* build — the first fix put it
  beside `extract_license_files`, which runs only on a PDFium cache **miss**, so it would never
  have run on a machine that had built before. `scripts/ci/check-licenses-bundled.sh` reads the
  built `.app`, because a test asserting on `build.rs` passed against every broken version.
- **LAME replaced by macOS AudioToolbox (ADR-0004).** Static link + `lto` + `strip` made the
  LGPL §6 relink right impossible to exercise without maintaining a parallel unstripped build
  forever. macOS has **no MP3 encoder**, so the container had to change: Supertonic exports are
  AAC/M4A; Fish keeps MP3 (its API returns it). Tagging dispatches on extension — ID3 onto an
  M4A *corrupts* it rather than failing, so `tag_chapter_export` errors on an unknown container
  rather than silently dropping the attribution #97 added. Cache moved to `tts-cache-v3`.

**Three bugs in that work were invisible to review and caught only by rebuilding and looking
inside the `.app`**: removing `externalBin` did not remove ffmpeg (the `binaries/**` resource
glob was the real mechanism); the licence fix sat on a rarely-executed branch; and the new check
itself hardcoded the wrong path (Tauri's `resources/**/*` glob preserves the leading segment, so
notices land at `Contents/Resources/resources/LICENSES/`).

### Release: signing works, and the manual pipeline has run end to end

**#48's credential half is done.** Fully documented in the issue, but the parts worth not
rediscovering:

- The Developer ID private key was on **`16-MacBook-Pro`**, not this machine. Two certs exist in
  the portal (Team `7XU3QW326W`, individual enrolment, legal name **Quang Phung**); the live one
  expires **2031/06/02**, the other 2027/02/01 and is keyless everywhere — revoke it. A `.p12`
  was exported and imported here; **keep a durable backup**, its absence caused an hour of
  archaeology.
- The certificate alone is not enough: the **`Developer ID Certification Authority G2`**
  intermediate (`http://certs.apple.com/devidg2.der`) must be installed too. Without it
  `codesign` fails with `errSecInternalComponent`, whose real message is the line above it —
  `unable to build chain to self-signed root`.
- `set-key-partition-list` was **not** needed in a logged-in GUI session; it will be on a
  headless runner.
- `APPLE_SIGNING_IDENTITY` is set as a repo variable; the `jr-notary` profile authenticates.
- **A DMG was built, notarized (Accepted), stapled and passed `spctl`** — `source=Notarized
  Developer ID`. Two caveats, both since resolved: `release.yml` had not run at that point
  (it has now), and that DMG was **not** the whole story — the `.app` *inside* it had no
  ticket. See the next two sections.

**Gatekeeper will show testers "Quang Phung"**, not LibreTexts — that is what individual
enrolment puts in the certificate. Changing it needs an Organization enrolment and a D-U-N-S
number.

**Unverified:** an end-to-end chapter export through the running app. The encoder, tagger and
paths are each covered, and `afinfo` confirms macOS decodes the output as real AAC
(`estimated duration: 2.000000 sec` for two seconds of samples) — but the assembled path has
never been exercised. Also note AudioToolbox is using its **default VBR quality**, not a chosen
bitrate, against LAME's previous 128 kbps CBR; if exports sound thin, that is the knob.

### Release: the shipped DMG now staples the `.app` inside it (2026-08-22, later)

**The beta artifact is final and correct.** `LibreTexts Reader_0.1.0-beta.1_aarch64.dmg`,
16,107,846 bytes, at `target/release/bundle/dmg/`. Signed `Developer ID Application: Quang
Phung (7XU3QW326W)`, notarization submission `3a0a8f45-58a4-4dc0-8ba1-c4898d40acc5`
**Accepted**, stapled, `spctl` `accepted / source=Notarized Developer ID` — and the `.app`
inside the DMG is stapled too, which the previous one was not.

Built from the tree at `fa7829c` (binary stamped `21:06:53`, between `fa7829c` and the
docs-only `a5bd543`), so it carries the post-#50 licence work.

**The size is a useful signal: 40 MB → 16.1 MB.** That is `c098a7d` (drop the unused ffmpeg
bundle) and `fa7829c` (AudioToolbox instead of LGPL LAME) landing *in the shipped artifact*,
not just in source. Two corroborating checks on the same bundle: exactly one bundled native
remains (`Contents/Resources/resources/pdfium/aarch64-apple-darwin/libpdfium.dylib`, down from
23 binaries needing pre-signing), and `LICENSES/` is present at
`Contents/Resources/resources/LICENSES/`. If a future bundle jumps back toward 40 MB,
something re-entered the bundle.

**The defect that was found — #102.** `RELEASE.md` and `release.yml:131-133` both ran
notarize DMG → staple DMG → staple `.app`. The third step staples
`target/release/bundle/macos/LibreTexts Reader.app`, but Tauri built the DMG from that app
two steps *earlier*, so the copy a tester installs never got a ticket:

```
$ xcrun stapler validate "/Volumes/LibreTexts Reader/LibreTexts Reader.app"
LibreTexts Reader.app does not have a ticket stapled to it.
```

Consequence is narrow — a tester whose **first launch is offline** sees "cannot be verified";
online machines are fine because Gatekeeper fetches the ticket. `RELEASE.md` §2 is fixed
(two-pass: build → codesign → notarize → staple, twice, with the inner-app check in the verify
block). `release.yml` carried the same single-pass bug and was fixed in `899b1dd`; #102 is
closed.

**Three traps here, all of which cost time this session:**

- **`stapler validate` and `spctl` answer different questions.** Stapling asks "is a valid
  ticket attached?"; `spctl --context context:primary-signature` asks "is this signed by a
  trusted Developer ID?". They are independent.
- **`spctl` uses the network**, so it reports `accepted / Notarized Developer ID` for an
  *unstapled* app and cannot tell you whether stapling worked. Only `stapler validate` can.
  `release.yml:134` runs `spctl` and not `stapler validate`, so the workflow cannot detect
  this class of bug at all.
- **`tauri:build` codesigns the `.dmg`; `bundle_dmg.sh` does not.** A hand-rebuilt DMG is
  unsigned, notarizes and staples happily, and then `spctl` says `rejected / source=no usable
  signature`. This burned one notarization round trip. `codesign` must come **before**
  notarizing — signing afterwards rewrites the file and voids the ticket. Run the cheap local
  `codesign --verify --strict` before the 5–40 minute remote one.

**To confirm the tester's experience**, copy the app out of the mounted DMG and mark it
quarantined — that xattr is what triggers Gatekeeper's assessment in the first place. Verified
passing: `xattr -w com.apple.quarantine "0081;00000000;Safari;"`, then `stapler validate` and
`spctl -a -t exec`.

**Nothing was outstanding before this build.** The `Private beta` milestone is 10 of 11 closed;
the one open item is #48, whose remaining half is the self-hosted runner and a `workflow_dispatch`
dry run — automation, explicitly not a prerequisite for a hand-cut private beta. The other open
issues (#59 no resume / progress bars stuck at zero, #61 imports cannot be cancelled, #65
dependency attribution + Supertonic model licence, #57 the product name, #63/#64/#69 cleanup) are
none of them milestone-gating.

### Release: the pipeline ran itself, and v0.1.0-beta.1 is published (2026-08-23)

**[v0.1.0-beta.1](https://github.com/johnnylibretexts/libretexts-reader/releases/tag/v0.1.0-beta.1)**
— GitHub pre-release, `LibreTexts Reader_0.1.0-beta.1_aarch64.dmg`, 16,107,717 bytes,
SHA-256 `50a4dcf9…531d`. Built and published by `release.yml` from tag `v0.1.0-beta.1` →
`f1125eb`, unattended, all 14 steps green in 7m20s. Two notarization submissions
(`234aee5f…`, `c3eea784…`), both Accepted.

`release.yml` had **never executed** before this day. It has now run twice: a
`workflow_dispatch` dry run (32623022810) and the tag run (32624211360). Both green.

**The published asset was downloaded and verified independently**, not taken on the
workflow's word: SHA-256 matches the release notes, the DMG is Developer-ID signed and
stapled, `spctl` accepts it, the **`.app` inside it is stapled**, `LICENSES/` ships, and a
copy pulled out and marked `com.apple.quarantine` still validates and is accepted. That
checksum match is load-bearing evidence: the notes' hash is computed *after* pass 2 replaces
the DMG, so a match proves the published file is the rebuilt, re-signed, re-notarized image
rather than the one Tauri originally produced.

**The runner.** `jr-release-mac`, runner v2.336.0 in `~/actions-runner`, labels
`self-hosted, macOS, ARM64, release`.

- **It is `--ephemeral`, which means single-use in a stronger sense than "exits after one
  job": GitHub deletes the registration** (`√ Removed .credentials` / `√ Removed .runner`)
  and the repo goes back to `total_count: 0`. Every release needs `config.sh` with a fresh
  registration token **and then** `run.sh`. Observed twice. Sequence is in `docs/ci.md`,
  "The runner is single-use".
- **Labels are case-insensitive.** It registers with GitHub's automatic `macOS`;
  `runs-on: [self-hosted, macos, release]` matches. Adding a lowercase `macos` is refused as
  a duplicate read-only label. A mismatch here queues a job nothing picks up, with no failure
  signal — the exact shape #48 was filed about.
- **`set-key-partition-list` has never been run and was not needed.** Proven by both runs:
  `codesign` signed the PDFium dylib with no prompt. That holds only because `run.sh` is
  started from a logged-in GUI Terminal with the keychain unlocked. Headless or over SSH it
  will be needed, and it wants the login password.

**Do not trust the timings as costs.** The release job took 6m20s (dry) and 7m20s (tag)
because `target/` and the cargo registry were warm from local builds the same session. A
cold runner pays the full release compile plus `ort`'s ~73MB ONNX Runtime fetch;
`timeout-minutes: 120` is still the right bound and neither run tested it.

**`docs/ci.md` said the runner rules exist "because this repo is public".** It is private
(#56), with no forks. Corrected in `f1125eb` — and deliberately *without* relaxing the rules:
a self-hosted runner has no job-level isolation, visibility limits who can trigger a workflow
rather than what it can do once it lands on a Mac holding a signing key, and the rules must
already exist if the repo ever goes public. The rule that carries the weight is keeping the
`release` label exclusive to `release.yml`.

**To cut the next release:** re-register the runner, start it, bump the version in all five
places (`check-version.sh` guards only three — the two lockfiles bite via `npm ci`), tag,
push. See `docs/ci.md` "Cutting a release".

### Session of 2026-08-22 — the first-run download, five silent failures, then the release path

Ten PRs, eight issues closed, in three threads.

**Thread one: the 383 MB Supertonic fetch from `huggingface.co`** — the one thing a fresh install
hits before it can play a word. It now reports real progress, can be cancelled, resumes instead
of restarting, and can only ever have one instance running. **Each ticket was found by the one
before it** — making the download cancellable is what made losing a partial reachable on purpose,
and making it resumable is what put the shared cancel flag under real scrutiny.

**Thread two: things that failed without saying so** — a forgotten export voice (#76) and four
swallowed errors (#62). Written up after the download thread below.

**Thread three: making the release worth running** — the release workflow ran no tests at all
(#66), and imported books' licence and attribution were captured and then used nowhere (#51).
Both are prerequisites for a beta that is still gated on #48.

**#52 → [PR #86](https://github.com/johnnylibretexts/libretexts-reader/pull/86)** (`ebea718`) —
the first Play fetched 383 MB behind one static string with all eight playback controls
disabled, so a working download and a hung app looked identical for several minutes. There is
now a determinate bar (`156 MB of 383 MB · 41%`) in both the reader header and the mini player,
a Cancel button on each, and a pre-warning in the library empty state that reads its figure from
the model manifest so it cannot drift. Play/Pause stays enabled throughout — pressing Pause
cancels. Skips and the section dropdown stay disabled; there is no audio yet to skip.

**The progress channel was never reached at all**, and that is the part worth keeping.
`fillSpeechBuffer` → `cachedSpeechBlob` reaches `ensureReady` *before* the sentence being spoken
does, and passes no status callback; by the time that sentence asks for its audio the prefetch
has already filled the cache, so the engine is never called and even the static string mostly
never rendered. Readiness now happens once, at the top of `speakWithBufferedSpeech`, where its
status has somewhere to go. **A prefetch that warms a cache will silently swallow any status the
on-demand path was supposed to report** — check which caller actually reaches the code that
emits, not which one looks like it should.

Cancellation is real rather than cosmetic: `SupertonicDownloadCancel` is Tauri managed state,
checked inside the progress closure that `download_verified` already `?`-propagates on every
chunk, so Cancel drops the HTTP stream mid-file instead of finishing the 256 MB one first. It is
deliberately **not** a `static` — the crate's tests run as threads in one process and would
share it.

**#87 → [PR #89](https://github.com/johnnylibretexts/libretexts-reader/pull/89)** (`5135ded`) —
and that Cancel button is what made the next defect reachable on purpose rather than only by bad
luck. A failed download threw away every byte fetched for the file in flight, so the single
256 MB file could fail at 90% and start again from byte zero, repeatedly. `download_verified`
now keeps the `.download` temp file, sends `Range: bytes=<existing>-`, and seeds both the byte
count and the SHA-256 hasher **from what is actually on disk** — not from the count the last
attempt believed it had written, because an interrupted write can land short and the digest has
to cover the real bytes.

Resuming is only ever an optimisation; it can never install bytes of unknown provenance:

| Answer | What happens |
| --- | --- |
| `206` | Append to the partial, digest over the whole file |
| `200` | The server ignored `Range` and sent everything — truncate the partial away rather than append to it |
| `416` | The partial runs past the end of the file — discard it, rather than let `error_for_status` turn a bad partial into a permanent failure |
| digest fails after a **resume** | Discard the partial, refetch from scratch, once |
| digest fails on a **fresh** fetch | Error. No retry loop. |

A partial already at or past the expected size is not a prefix of anything and is never resumed,
which is what makes the `416` row the one branch without a test: the Supertonic manifest always
states a size, so that guard makes 416 unreachable in practice. It is there for a manifest that
does not, and for a server whose file is shorter than the manifest claims.

**Two halves had to change and only one was in `download.rs`.** The caller at
`chapter_tts.rs:131-133` deleted the temp file at the top of *every* attempt, so a perfect
`Range` implementation would still have had nothing to resume from. Check the caller before
concluding that a networking fix lives entirely in the networking module.

**A tests-after trap worth naming.** Three of the five new tests *cannot* fail against `main` —
the old code always restarted, which is trivially correct — so watching them go red first was
impossible and passing proved nothing. Each was pinned by mutating the **new** code instead:
dropping the `206` check makes the ignores-`Range` test cost two full fetches; dropping the size
guard makes the oversized-partial test send a pointless `Range`. One mutation *survived* on the
first attempt, and the assertion was tightened until it did not. **When a fix's own safety net
makes a test un-failable against the old code, mutate the new code until the test dies** — the
repo's "revert the fix and watch the test fail" rule has no purchase there, and skipping the
step leaves a test that asserts nothing.

**#88 → [PR #91](https://github.com/johnnylibretexts/libretexts-reader/pull/91)** (`0585e1c`) —
the third and last of the chain, filed off the back of #86 and taken the same day. Two surfaces
can ask for the model — the player on first Play, and the Settings Download button — and nothing
kept them apart. Both cleared the same cancel flag on entry, so a Cancel the reader had already
pressed was voided by whichever request arrived next, and both wrote and renamed the same
`<file>.download` temp paths.

`SupertonicDownload` replaces `SupertonicDownloadCancel` as managed state and owns both the flag
and the one download slot. The first caller runs the download; every later one joins it and is
handed its result. **`clear()` now happens only where a download actually starts** — a caller
that merely joins does not clear, which is the whole fix.

Three things about the mechanism that are not obvious from reading it:

- A joiner subscribes to the result **under the same lock the leader publishes under**. That is
  what makes the handoff safe; a receiver created after the leader has taken its sender back out
  would miss the result entirely, because a `broadcast` subscriber never sees a value sent before
  it subscribed.
- The leader holds an RAII guard over the slot. A command future dropped or panicking mid-flight
  releases the slot and wakes its waiters; without it they park forever on a result nobody will
  ever publish.
- **`AppError` cannot be `Clone`** — it wraps `rusqlite::Error` and `reqwest::Error` — so a joiner
  inherits the *message*, rebuilt as `AppError::Model`. Exact for cancellation, which is already a
  `Model` error the webview matches by substring, and lossy but honest for everything else. Do not
  "fix" that with a reverse kind-to-variant map: `check-error-kinds.sh` guards Rust↔TypeScript, not
  a reverse mapping, so one would drift silently.

**Two of the five tests could not be written red-first here either**, and the mutation that killed
them is worth copying: a slot guard that never releases, plus a sender left in the slot after
publishing. Both are bounded by a 5s `timeout` **on purpose** — a captured slot does not *fail* a
test, it *stops* one, and #44 already cost this repo a CI run at the six-hour job cap. Never leave
a concurrency test unbounded in this repo.

**A `State<T>` that is not `manage`d panics at invoke time, not compile time.** Swapping the
managed type compiles clean either way, so every `State<'_, T>` any command takes was audited
against `lib.rs` by hand. If you add or change managed state, do that audit — the test suite will
not do it for you.

**#76 → [PR #93](https://github.com/johnnylibretexts/libretexts-reader/pull/93)** (`d9a3b6d`) —
`AppShell` switch-renders routes, so a trip to the Library unmounts the Reader outright and the
chapter-export panel's Voice and Language went with it. A new `chapterExport` store holds them
for the session. Deliberately **not** the `supertonic_voice_style` / `supertonic_language` rows:
the panel used to write those, and once playback started reading them that write switched the
narration of the book open in the same view, mid-chapter. #60 removed it and this does not bring
it back.

**Moving the drafts alone would have fixed nothing**, which is the part worth remembering.
`voiceChosen` / `languageChosen` were `useRef`s and `seededFrom` was `useState`; all three reset
on unmount, so the seeding effect ran again on the way back and overwrote the remembered pick.
State that decides *whether* to re-seed has to outlive the component exactly as far as the values
it guards. The `seedSignal`-computed-during-render rule from #78 is unchanged, and `voiceStyle`
now falls back to the app default during render so the `<select>` cannot go uncontrolled in the
commit hydration lands — a fallback that deliberately does **not** make `seeded` true.

**#62 → [PR #94](https://github.com/johnnylibretexts/libretexts-reader/pull/94)** (`b6ebb17`) —
four `catch` sites that disabled something durable with no sign. Three were as reported. The
fourth was not, and **the ticket is now the third on this repo to be wrong about its own
premise** — see the [premise comment](https://github.com/johnnylibretexts/libretexts-reader/issues/62#issuecomment-5382266557),
posted before any code was written.

- **Two dead subscriptions** (import progress, library auto-refresh) swallowed a failed
  `listen()`. Both now **guard on `isTauriRuntime()` first** — `listen` rejects in every jsdom
  test and any `vite` preview, and that is a missing runtime rather than a failure. Removing the
  swallow without the guard puts an error banner in front of every test run. Inside Tauri a
  rejection sets the store's existing `error`, which `ImportStatus` and `Library/Grid` already
  render, so no new components were needed.
- **The library subscription moved out of `AppShell` into the library store**, beside
  `attachImportListener`. A subscription belongs with the store it writes; as a component effect
  it also had no seam a test could reach.
- **The LibreTexts library filter** emptied its dropdown on failure, reading as "this Source has
  no libraries". It needed **its own** error state — the existing one is cleared at the top of
  every debounced catalog fetch, so a keystroke would have wiped the message.
- **The player was misdiagnosed.** `player.ts:751` (the ticket said 548; the file grew when #86
  landed) is in the *read-ahead worker*, not the playing path. Auto-advance re-enters
  `speakWithBufferedSpeech`, whose catch already stops playback, shows the reason and offers the
  Fish switch — so playback never silently stopped. The real defect was that one bad sentence
  ended the entire read-ahead; `return` became `continue`. **Nothing is reported from there on
  purpose**: surfacing a prefetch failure would stop playback that recovers on its own, which is
  what AC 2 literally asked for. That AC is still unamended on the closed ticket.

Two testing notes from #62 worth reusing. `createFakeEngine` gained **`failSynthesisFor`**,
because the existing `failSynthesis` is all-or-nothing and its own comment described the bug it
could not express. And **at a prefetch concurrency of 2 a single failure is invisible** — the
other worker absorbs it — so the test fails a contiguous run and asserts on the stranded tail.
Run 8× for stability, given #32.

**#66 → [PR #96](https://github.com/johnnylibretexts/libretexts-reader/pull/96)** (`66e8727`) —
`release.yml` ran **no tests at all**. `RELEASE.md` listed a pre-publish gate; the workflow ran
none of it, and a tag can point at any commit, so the automated path could publish a tree
`ci.yml` had never validated.

The gate now lives in **`.github/workflows/verify.yml`** as a `workflow_call`, and `ci.yml` and
`release.yml` both call it, with the release job `needs:`-ing it. **Extracted rather than
copied**: a copy satisfies that on the day it lands and drifts by the second edit. It also lets
the gate declare its own `contents: read` instead of inheriting `release.yml`'s
`contents: write` — a called workflow takes the caller's permissions by default.

Two consequences to know. The status check is now named **`verify / verify`**, not `verify`;
there is no branch protection today, but that is the name a required check would need. And the
job timeout went **60 → 120**, because 60 does not cover a cold `cargo build --release` (with
`ort`'s ~73MB fetch), a second full Tauri build, and `notarytool submit --wait` at up to 40
minutes — and hitting it burns the tag.

**#66's AC 3 was wrong, and was corrected on the ticket before implementing** — the third such
case on this repo. The `LIBRETEXTS_READER_REQUIRE_UPDATER_KEY` belt is *not* inert when the
updater is reintroduced without a key: `build.rs` returns early only when the `plugins.updater`
block is entirely absent, and a block with a missing or placeholder pubkey does panic. The real
gap was **dependency/config coherence**, which `build.rs` structurally cannot see. New
`scripts/ci/check-updater-key.sh` closes it, keyed on the **dependency** rather than the config:
an updater block with no plugin behind it is inert, a plugin with no block is not.

One mutation-testing lesson from it: deleting the missing-block branch still exits non-zero,
because the empty-pubkey check below catches the same input. A status-only assertion proved
nothing, and the reader would have been told the *pubkey* was missing when the whole *block*
was. That case now asserts the message.

**#51 → [PR #97](https://github.com/johnnylibretexts/libretexts-reader/pull/97)** (`22a2af5`) —
licence and attribution were captured at import and used nowhere: the only licence on screen was
pre-import, and a chapter MP3 left as a derivative work with the credit stripped.

**The fact that shaped the whole fix, and which the ticket does not mention:
`documents.attribution` is polymorphic by Source.** OpenStax, LibreTexts and article store a URL
there; **Pressbooks stores an author name**. It cannot render one way and cannot map to one ID3
frame — a URL written as `TPE1` fills every music player's artist column with a link, and
hyperlinking a person is worse. The shape decides: `WOAS` when it parses as http(s), `TPE1`
otherwise, in both the reader line and the tags. **Check the shape before touching either.**

In the app it is a line under the section title in `ReaderHeader` — the surface where the work is
actually consumed. Neither field present renders *nothing*: not an empty field, not a bare
separator, because a pasted-text import genuinely has no licence.

In exports, new `src-tauri/src/tts/tags.rs` **tags the file, not the bytes** — Fish returns MP3
data that may already carry a tag, and prepending a second one is not replacing it. Tagging
happens **on the way out rather than in the cache**, so a chapter cached before this still leaves
with credit attached, and `byte_length` now reports the file on disk rather than the pre-tag
buffer.

`id3` 1.17.1 (MIT, pure Rust) is a new dependency. Its notice is in `LICENSES/id3.txt` and named
in the README, **so #50 does not inherit a fresh gap from it** — bundling `LICENSES/` into the
`.app` is still #50's job.

### Session of 2026-08-21 — five PRs, four issues closed

**#78 → [PR #80](https://github.com/johnnylibretexts/libretexts-reader/pull/80)** — the three
low-severity findings from the #60 review. A Fish voice save started from the *Your voices*
dropdown set `savedFrom` and rendered it nowhere, so it spun and went silent; both controls now
confirm, in **separate** live regions (a shared one would mark a control the reader never
touched, which is the bug splitting `savedFrom` fixed). The settings store's `error` doc comment
claimed a failed hydrate is cleared by a later theme or provider action — it is not, only
another `hydrate` clears it. And the chapter export's `seeded` gate missed the retry path it
names.

**That last one is the reusable lesson: every effect in one commit sees that commit's
closures**, so a flag a *sibling effect* sets can only ever gate the first transition. On the
`hydrateFailed: true -> false` retry `seeded` was already true and gated nothing, so the
estimate priced the chapter for the pre-retry `DEFAULT_SETTINGS`. It is now **derived during
render** — which settings snapshot the drafts came from, against the one this render sees — so
it is false in the very commit a change arrives. If you touch that effect, keep the derivation;
a boolean set by the seeding effect cannot work.

**[PR #81](https://github.com/johnnylibretexts/libretexts-reader/pull/81)** — a test that failed
**5 runs in 12**. `findByLabelText("Your voices")` can resolve in the commit where `keyStatus`
has landed but the voices effect has not yet set `loadingVoices`, so the `<select>` is on screen
holding only "No voice models yet". **Wait on the `<option>`, not the label** — that is the
house pattern in that file now.

**#49 → [PR #82](https://github.com/johnnylibretexts/libretexts-reader/pull/82)** — the repo is
on **`0.1.0-beta.1`**. The version lives in **five** files, not the three `check-version.sh`
guards: the lockfiles matter because `release.yml` runs `npm ci`, which fails outright when
`package-lock.json` disagrees with `package.json`. The stale local `v0.1.0-beta` tag (188 commits
behind, never pushed) is deleted. Two User-Agents hardcoded the version and were lying to
Pressbooks servers and huggingface.co; both now derive from `CARGO_PKG_VERSION` with a test each.

**#48 → [PR #83](https://github.com/johnnylibretexts/libretexts-reader/pull/83)** —
`scripts/release-setup.sh` walks the four human-only provisioning steps and ends by dispatching
the dry run. **Everything it provisions is now in place and #48 is closed** (2026-08-23); the
snapshot that used to sit here — 0 identities, zero runners, zero repo variables, no `jr-notary`
profile — described 2026-08-21 and is no longer true. The wizard is still the right entry point
on a *fresh* Mac.

**#53 → [PR #84](https://github.com/johnnylibretexts/libretexts-reader/pull/84)** — Delete now
confirms, naming the book, from both the trash button and the context menu. Built on the native
`<dialog>`; **Cancel is first in the DOM on purpose**, because `showModal()` focuses the first
focusable child and on a destructive confirmation that must never be the destructive button.
Two bugs found while building it: `remove` reports failure by writing the store's `error` field
rather than rejecting, so `await remove(...)` resolves either way and the first version reset the
player on a *failed* delete; and resetting the player was itself arming a doomed load, because
`AppShell` owns the reader route and `Reader` re-fetches whenever its `documentId` and the
player's document disagree.

### Two things this session established that are easy to rediscover the hard way

**`npm run tauri:build` fails locally unless `CI=true` is set.** It compiles, bundles the `.app`,
then dies in `bundle_dmg.sh` with `Finder got an error: AppleEvent timed out. (-1712)`. That step
is Finder cosmetics and needs a GUI session. Tauri **swallows the script's stderr**, so it reads
as a broken build. The Actions runner sets `CI` itself, so `release.yml` is unaffected. First
successful bundle: `LibreTexts Reader_0.1.0-beta.1_aarch64.dmg`, 41.9 MB — exactly what
`release.yml:101` expects.

**jsdom 30 ships the `HTMLDialogElement` constructor but none of its methods.** `showModal` and
`close` are `undefined`, so anything built on `<dialog>` throws on open. `src/test/setup.ts`
models enough of the spec for component tests.


**There is no work in progress.** This section used to track an uncommitted change set; everything in it is merged and pushed. It is kept as a map of what the app gained most recently, newest wave first.

### 5. The Supertonic voice style reaches playback (2026-08-21, `38c9fc5`)

[PR #77](https://github.com/johnnylibretexts/libretexts-reader/pull/77) closed #60.
Playback had ignored the Voice style setting entirely: `player.ts` seeded a shared
`voice: "M1"` field that no component ever set, so every request carried the literal
`"M1"`. Export, Preview and Test each read the setting themselves, so the control looked
like it worked everywhere a reader might check it and did nothing in the one place it
matters. Supertonic now captures the style at construction, the way Fish already did with
`fishVoiceId`, and `SynthesisRequest.voice`, `PlayerState.voice` and `setVoice` are gone —
a shared field no engine honoured on any reachable path.

The Rust side did not change. Both files in that diff are comment-only.

Four invariants came out of it that are worth knowing before touching playback or settings:

- **`voiceKey` vs `engineKey`.** `voiceKey` is everything that changes how a sentence
  *sounds*, and is what `speechCacheKey` is built from. `engineKey` is `voiceKey` plus
  where the settings came from, and is what decides engine rebuilds. Anything the engine
  captures must be in `engineKey`; anything that leaves the audio identical must stay out
  of the cache key. Both are per provider, so a Supertonic row cannot re-key Fish's
  already-billed buffer.
- **The settings snapshot travels with the engine.** `activeEngine()` returns the pair,
  and nothing downstream re-reads the store to build a key. `fillSpeechBuffer` reads the
  live store only to decide whether to *stop* — a liveness check, never a key.
- **Settings rows are written through a per-row serializer and applied only once the
  write lands** (`writeRow` in `stores/settings.ts`). The store cannot disagree with
  SQLite in either direction. `setTheme` is the one optimistic writer, because the theme
  has to change the instant it is clicked; it reverts to `committedTheme` rather than to
  what it read before the write.
- **A failed settings load is distinguishable from a successful one** (`hydrateFailed`).
  It used to leave every row at `DEFAULT_SETTINGS` while reporting success — which, once
  playback read those rows, silently moved a Fish reader onto Supertonic and prompted for
  its ~383MB model. Supertonic refuses to fetch that model for a provider the fallback
  only guessed at, `hydrate` is retryable, and Settings blocks the writes that would
  overwrite rows the screen never showed.

Two non-blocking follow-ups came out of the review: **#78** (three low-severity items,
including a Fish voice picked from the dropdown showing no save confirmation) and **#76**
(the chapter-export panel forgets its voice when the Reader unmounts — it used to
"remember" by writing the shared settings rows, which is the write this PR removed).

### 4. Fish Audio, the optional cloud provider (2026-08-16, `64ead91`)

Spec B of the TTS direction. Supertonic stays the default and stays fully on-device; Fish Audio is a second engine the reader configures with their own API key, used for playback and chapter export. Full detail — the design decisions, what the review found, and what playback actually bills — is in **TTS direction** below. Read that section before touching provider selection, the audio cache, or the export gate.

The two things most likely to surprise someone new to this code: **one Play bills roughly ten sentences**, not one, because the player reads ahead and cannot cancel; and **the webview is the only place an engine is chosen** — no Rust command may re-derive it from the `tts_provider` setting.

### 3. Durable import state (2026-08-13, `97473cc`)

Imports no longer die when you navigate away from a catalog. `AppShell` renders the browsers conditionally, so unmounting one killed its progress display, its event listener, and — the damaging part — its concurrency guard, so clicking Add again started a *second* concurrent import. That is how two copies of General Biology (~308 MB) got downloaded.

New files a fresh reader will not otherwise find:

- `src/stores/imports.ts` — module-level Zustand store owning the import lifecycle. `start()` checks `active` and sets state **synchronously before `run()` is invoked**, so even a same-tick double click is refused. `run` is injected, so the store never imports the Tauri API and its whole suite (`src/stores/imports.test.ts`) runs unmocked. Also exports `attachImportListener()`, subscribed once at app level in `AppShell.tsx:89`.
- `src/components/ImportStatus.tsx` — progress strip in persistent chrome, outside the route switch. Catalog completion now notifies instead of navigating, so a finished import no longer yanks the reader out of the book you are in.
- `src/lib/importedBooks.ts` — `findImportedBook`, parameterised over the metadata key because LibreTexts uses `book_id` and OpenStax uses `book_uuid`. Both browsers use it to mark already-imported books.

Both catalogs disable *every* Add while an import is active, so the guard is **global across catalogs** — deliberately stronger than the bug required.

Spec and plan: `docs/superpowers/specs/2026-08-13-durable-import-state-design.md`, `docs/superpowers/plans/2026-08-13-durable-import-state.md`.

**Verified end to end through the UI on 2026-08-13**, all ten manual checks passing against the real library — including that navigating away and back leaves every Add disabled, and that a network failure releases the guard within the request timeout rather than stranding it.

**Two things were deliberately left open — pick these up before building on the store:**

1. ~~**No regression test on the final fix wave.**~~ **DONE** — `src/stores/imports.test.ts:132-200`
   now carries four mutation-killing tests and the suite is 12, not 7. Keep the reasoning,
   because it generalises: `beforeEach` resets the store to all-null, so
   `expect(error).toBeNull()` after a fresh success asserts a value that was *already* null and
   passes whether or not the code does anything. Each test now seeds a non-null value first —
   two before `start()` to pin the entry clear, two from inside `run()` so only the success and
   failure paths can satisfy them. **Verified by mutation on 2026-08-17**: each of the three
   `set` clears in `imports.ts` was deleted in turn and a test failed every time.
2. **A spec requirement no task implemented.** The spec requires an in-library card to show "In library" **with an Open action in place of + Add**; both browsers render a static span. Tell-tale: `findImportedBook` returns a full `Document` but both call sites use only its truthiness. Either implement Open or amend the spec.

Also carried, each belonging with other work: `active` has no user-clearable escape if `run()` never settles (mitigated by 15–20s request timeouts; belongs with cancellation), and quitting mid-import orphans image files (belongs with the deterministic-filenames follow-on — see `6c1262c`).

### 2. Kokoro removal (2026-08-13, `09d97b3`)

Supertonic is now the only bundled engine. See **TTS direction** below and ADR-0003 for the reasoning; migration `0007_drop_kokoro_voices` drops the `voices` table and rewrites any stored `kokoro` provider/voice id, and `db/settings.rs` additionally coerces a stored `kokoro` provider to `supertonic` on read for databases that somehow skip it. The `model_precision` setting and the whole first-run model chooser are gone with it.

### 1. Math, LibreTexts imports, and figures (2026-08-13)

- KaTeX-based math rendering in the reader.
- MathML token preservation for imported textbook math.
- More math-aware TTS normalization for the Supertonic path.
- LibreTexts import support and browser improvements.
- Downloaded textbook figures for LibreTexts and OpenStax.
- SQLite persistence for section images.
- Inline figure placement in the reader based on source order.
- Tauri asset protocol support so downloaded local images can display in the webview.
- OpenStax catalog cards now visually match LibreTexts catalog cards, including thumbnails and source links.
- OpenStax bundled catalog now has `coverUrl` values for 95 of 112 books from the OpenStax CMS books API.
- Tauri CSP now allows OpenStax cover asset hosts: `https://assets.openstax.org` and `https://images.openstax.org`.

**Status as of 2026-08-13: all three waves are committed, merged to `main`, and pushed to `origin`.** The worktree is clean and `main` is level with `origin/main`. The list of dirty files that used to appear here is gone because there are none; use `git log` rather than `git status` to see what landed.

## TTS direction (decided 2026-08-13; A and B both done)

**Decision: drop Kokoro. Supertonic becomes the only bundled engine. Add Fish Audio as an
optional provider where the user supplies their own API key.** Supertonic stays the default.

These were sequenced as **two separate specs**, A before B. A was mostly deletion and it
removed a broken engine plus its workarounds before anything new was added, so Fish landed
in a two-case registry instead of a three-case one. Keep that shape: a third provider should
arrive the same way.

### A. Remove Kokoro — DONE (2026-08-13)

Spec: `docs/superpowers/specs/2026-08-13-remove-kokoro-design.md`.
Plan: `docs/superpowers/plans/2026-08-13-remove-kokoro.md`. See ADR-0003.

### B. Add Fish Audio (bring-your-own API key) — MERGED (2026-08-16, PR #4, `64ead91`)

Spec: `docs/superpowers/specs/2026-08-13-fish-audio-design.md`.
Plan: `docs/superpowers/plans/2026-08-13-fish-audio.md`.
Task briefs and reports: `.superpowers/sdd/2026-08-13-fish-audio/`.

Reference docs the user supplied: <https://docs.fish.audio/overview/capabilities>,
<https://docs.fish.audio/features/text-to-speech>,
<https://docs.fish.audio/developer-guide/core-features/fine-grained-control>,
<https://fish.audio/blog/s2-1-pro-free-api/>. **Two skills exist for this** — prefer them
over reading the docs by hand: `fish-audio-api` (raw REST/WebSocket) and `fish-audio-sdk`
(official SDKs). Raw HTTP from Rust was the call taken.

How the open questions were answered, so they are not reopened by accident:

- **Where the key lives:** the OS keychain, via the `keyring` crate behind a `SecretStore`
  trait (`src-tauri/src/secrets.rs`). Never the `settings` table, never the webview. The
  trait exists so tests inject `MemorySecretStore` and never touch the login keychain.
- **When it is validated:** at entry. `set_fish_api_key` calls Fish's wallet endpoint, which
  proves the key and reads the balance without synthesizing (and therefore without billing);
  an invalid key is rejected without overwriting the stored one.
- **Whether cost is surfaced:** yes, at both places money is spent. Chapter export gates on
  `requiresExportConfirmation` (`src/components/Reader/exportGate.ts`) and shows the billable
  character count plus a freshly fetched credit balance; the Settings "Test voice" button
  gates the same way when the active provider bills.
- **What happens when the network drops mid-playback:** playback stops and says why. It
  never silently falls back to Supertonic — the switch is offered as a button the reader
  clicks (`canSwitchToSupertonic` / `switchToSupertonic` in `src/stores/player.ts`).

**Supertonic remains the default and must stay independent of all of this**: it needs no key,
no account and no network, and a Fish-only failure (an unreadable keychain, say) must never
be able to break it. See `fish_api_key_for` in `src-tauri/src/commands/chapter_tts.rs`.

Still true and still worth knowing: `SpeechEngine` (`src/lib/speech/types.ts`) is the seam,
and `createSpeechEngine` (`src/lib/speech/index.ts`) is the single place an engine is chosen.
A third provider is a case there and a case in `provider_for` on the Rust side, and nowhere
else.

#### What playback actually bills

Fish playback does **not** bill one sentence per Play. The player reads ten sentences ahead
at concurrency two on every play and every seek past the buffered window, and there is no
cancellation channel — so sentences fetched for a passage the reader skips are billed and
never heard. One Play is roughly ten sentences of spend. `README.md` says this plainly;
don't "simplify" it back to per-sentence.

#### Review round (2026-08-16) — 15 findings, all resolved

`/code-review xhigh` over `main...HEAD` found 15 issues; every one is fixed on the branch.
Two were outright blockers, and both are worth knowing because neither showed up as a
failing test or a red build:

1. **Fish could never be selected.** `migrate_removed_tts_provider` still listed `fish` as
   a retired provider, and `set_setting` applies that rewrite *before* the INSERT — so
   saving `tts_provider = "fish"` stored `"supertonic"` and returned `Ok`. The frontend
   twin `asTtsProvider` had been updated and even carried a comment saying `fish` was live
   again; the Rust half had not. **Both lists must move together.**
2. **Every Fish chapter export would fail after being billed.** `FISH_TIMEOUT_SECONDS` was
   set on the reqwest *client*, which makes it the total request budget including body
   download, and a chapter is minutes of MP3. `synthesis_timeout(text_len)` now scales the
   budget per request (capped at 15 minutes); the client keeps the 20s ceiling for the
   control-plane calls only.

Three others are worth carrying forward as facts rather than history:

- **The chapter cache moved to `cache/tts-audio/<version>/<hash>.mp3`.** The version used to
  be hashed into the filename only, which made superseded audio indistinguishable from live
  audio and impossible to reclaim. `cleanup::reclaim_stale_tts_cache_in` now sweeps
  non-current version directories at launch, so future bumps clean up after themselves.
- **Caching a chapter is best-effort; delivering it is not.** A paid provider has already
  billed by the time the bytes exist, so a cache-side failure writes straight to the
  reader's output path instead of aborting. See `deliver_synthesized_mp3`.
- **One review finding was wrong and should not be re-fixed.** It claimed an
  export-directory failure loses paid audio. It does not: the cache is written and renamed
  into place before `copy_cached_mp3` touches the output path, so that retry is free. Only
  the cache-side steps could lose billed audio, and that is what was fixed.

**The frontend can now test components.** `@testing-library/react`, `/user-event` and
`/jest-dom` are dev dependencies, and `src/test/setup.ts` registers `afterEach(cleanup)` —
required, because Testing Library only auto-cleans under `globals: true` and this project
does not set it. Three fixes that previously had no possible test now have one. When
retrofitting a test to an already-applied fix, **revert the fix and watch the test fail**
before trusting it; all four component tests here were verified that way.

### Why Kokoro is being dropped — do not re-investigate

Kokoro never produced audio in a bundled build. Two distinct faults were found:

**Fault 1 (root-caused and fixed, then discarded with the rest):** `onnxruntime-web` loads
its wasm backend with a dynamic module `import()`, and `@huggingface/transformers` defaults
that path to jsDelivr. The Tauri CSP allows jsDelivr in `connect-src` but **not**
`script-src`, so the import was blocked: `no available backend found. ERR: [wasm]
TypeError: Importing a module script failed`. It worked under `tauri:dev` — Vite serves
`node_modules` as `'self'` — and only failed in a bundled build, which is why it survived
to release. Fixing it required shipping `ort-wasm-simd-threaded.jsep.{mjs,wasm}` locally,
aliasing `kokoro-js` to its non-bundled node build (the browser build inlines transformers
and hardcodes the CDN with no `env` export to override), and setting `wasmPaths`.

**Fault 2 (never solved):** with the backend loading and the 92MB model read from disk,
`engine.generate()` still hung indefinitely — **0% CPU, zero network sockets, no requests
beyond the wasm binary** (confirmed by instrumented tracing and repeated sampling). It is
parked on a promise that never settles. Three hypotheses were tested and **all falsified**:

- voice embeddings fetched from HuggingFace at generate time — refuted, no sockets ever opened
- a missing `espeakng.worker.data` for the phonemizer — refuted, that data is inlined in
  `phonemizer.js` as base64 gzip
- multithreaded wasm starved of `SharedArrayBuffer` (no COOP/COEP, so `crossOriginIsolated`
  is false) — `numThreads = 1` changed nothing

Do not spend time re-testing those three. The remaining suspect was never confirmed.

All of that work is **discarded, not committed**. The tree was returned to clean.

## Session Notes — 2026-08-13 (repo migration + rename)

Two things happened, both fully merged to `main` and pushed.

**1. Repository moved.** `johnnyrobot/johnny-reader` → **`johnnylibretexts/libretexts-reader`** (private). `origin` was repointed and all branches preserved before cleanup; `main` is now the only branch. The old public repo still exists, untouched — anything previously pushed there is still public. This machine holds `gh` credentials for **both** accounts; confirm the active one (`gh auth status`) before any write. Issues now live on the new repo: see `docs/agents/issue-tracker.md`.

**2. Renamed *Johnny Reader* → *LibreTexts Reader*.** Five risk-tiered commits (`566ad5f`..`8dd2298`) plus a review fix wave (`a8b1565`):

- crate `johnny-reader` → `libretexts-reader`, lib `johnny_reader_lib` → `libretexts_reader_lib`
- env prefix `JOHNNY_READER_` → `LIBRETEXTS_READER_` (all 10 vars; two dropped a redundant `LIBRETEXTS` segment)
- bundle identifier `dev.johnnyrobot.reader` → **`dev.johnnylibretexts.reader`**, with `APP_DIR_NAME` changed in lockstep
- product name, export directory (`~/Documents/LibreTexts Reader`), and a non-affiliation notice in the README and Settings
- new migrations `0005_rebase_app_dir_paths` and `0006_rebase_export_directory` rebase absolute paths already stored in SQLite — a directory move alone is not enough, because `content/images.rs` persists absolute paths
- new CI gate `scripts/ci/check-identifier.sh` asserts identifier == `APP_DIR_NAME` and that every `assetProtocol.scope` entry is `$APPDATA`-relative

Design and plan: `docs/superpowers/specs/2026-08-13-rename-libretexts-reader-design.md` and `docs/superpowers/plans/2026-08-13-rename-libretexts-reader.md`.

**Verified:** `npm run build`, `npm test` (32), `cargo fmt`, `cargo clippy -D warnings`, `cargo test -p libretexts-reader` (60), all three CI gates. The app launches, creates its data under the new identifier, applies `0005`/`0006`, and reports the correct export directory.

**Not verified, and worth doing on the next import:** cover and figure rendering. There was no library on this machine, so the identifier/`$APPDATA` coupling has not been exercised end to end with real images. That is the one failure mode that produces no error — see `check-identifier.sh` and the asset-protocol gotcha.

**Open issues** on `johnnylibretexts/libretexts-reader`: **20 as of 2026-08-20** (#48-#69, less #56 and #68 closed by the private-beta decision),
all from the release-readiness audit — see "Known Limitations And Next Steps" below for
the map. This paragraph previously read "none as of 2026-08-17", which was true then and
is a good reminder that a hand-maintained count goes stale silently; prefer
`gh issue list` over trusting this line.

Historic closures from the era this section describes: #1 (the `check-identifier.sh` scope
guard) closed via PR #15, #12 (ffmpeg SONAME symlinks) via PR #16. #2 (the Rust suite
writing into the real app-data directory) was fixed on 2026-08-13, and #3 (model precision
is a one-way door) was closed as obsolete when the Kokoro removal deleted the setting it
described.

An empty `~/Library/Application Support/dev.johnnyrobot.reader` may still exist; it holds nothing and can be deleted.

## Latest Codex Session Notes

Date: 2026-05-20

The app was set up and built from a fresh clone of the repo.

Local machine setup:

- Rust was not installed initially. Installed Rust via `rustup`; verified with `rustc 1.95.0` and `cargo 1.95.0`.
- Node `24.13.1` was present, but Vite/Rollup native addon loading hung. Installed Node `22.20.0` with npm `10.9.3`, matching this handoff.
- `nvm alias default 22.20.0` was set, but commands in this repo should still explicitly run `source "$HOME/.nvm/nvm.sh" && nvm use 22.20.0` if the shell starts on Node 24.
- A copied Cargo `target` directory contained old absolute paths from a different machine; `cargo clean -p mp3lame-sys` fixed the Rust build.
- macOS provenance/quarantine metadata caused native Rollup `.node` loading to hang even after reinstalling `node_modules` and ad-hoc signing. The working solution was adding this dev dependency alias:

  ```json
  "rollup": "npm:@rollup/wasm-node@^4.60.2"
  ```

  This intentionally changes `package.json` and `package-lock.json` and removes many native Rollup optional package entries from the lockfile. Do not revert this unless native Rollup loading is fixed another way.

OpenStax catalog UI work completed:

- `src/components/OpenStaxBrowser/OpenStaxBrowser.tsx`
  - Added LibreTexts-style thumbnails with fallback icon.
  - Added loading and empty states.
  - Added OpenStax source link button.
  - Changed progress text to `Page current/total`.
  - Uses `book.coverUrl` when present.
- `src-tauri/resources/catalog/openstax.json`
  - Populated 95 `coverUrl` fields from `https://openstax.org/apps/cms/api/books/?format=json`.
  - 17 books still have no cover URL, mostly Spanish/Polish translated titles not present in that CMS payload.
- `src-tauri/tauri.conf.json`
  - Added OpenStax asset hosts to `img-src` CSP so remote thumbnails render in the Tauri webview.

OpenStax imported-book images:

- OpenStax image downloading was already implemented before this session:
  - `src-tauri/src/content/openstax.rs` extracts `img[src]` / `img[data-src]` in source order and anchors images to nearby paragraphs.
  - `src-tauri/src/content/images.rs` downloads and persists those images into the app data images directory.
  - `Reader.tsx` renders section images inline using `anchorParagraphOrdinal`.
- Existing OpenStax imports from before this work should be deleted and reimported to verify downloaded images.

Latest verification run:

```bash
source "$HOME/.nvm/nvm.sh" && nvm use 22.20.0 >/dev/null && npm run build
source "$HOME/.nvm/nvm.sh" && nvm use 22.20.0 >/dev/null && . "$HOME/.cargo/env" && npm run tauri -- build --debug --no-bundle
git diff --check
```

All passed. The rebuilt app was opened with the command below — **do not copy it**; on 2026-08-13 this was found to exit 0 without starting anything. Use `./target/debug/libretexts-reader` instead.

```bash
open target/debug/libretexts-reader
```

At the end of the session the debug app was running as:

```text
<repo>/target/debug/libretexts-reader
```

## Key Implementation Details

### Section Images

New DB table:

```text
section_images
```

Relevant files:

- `src-tauri/resources/migrations/0003_section_images.sql`
- `src-tauri/resources/migrations/0004_section_image_anchors.sql`
- `src-tauri/src/db/migrations.rs`
- `src-tauri/src/db/models.rs`
- `src-tauri/src/db/library.rs`
- `src-tauri/src/commands/library.rs`
- `src/lib/tauri.ts`
- `src/types/domain.ts`

Images now have `anchor_paragraph_ordinal`. The reader uses this to render each figure after the paragraph it followed in the source page. Images with a null anchor render before the first paragraph. Older imports that predate `0004` will have null anchors, so they will not show accurate placement until reimported.

### Image Download And Layout

Core image extraction/downloading is in:

```text
src-tauri/src/content/images.rs
```

LibreTexts source-order extraction is in:

```text
src-tauri/src/content/libretexts.rs
```

OpenStax source-order extraction is in:

```text
src-tauri/src/content/openstax.rs
```

The implementation does not preserve the full original textbook HTML/CSS layout. It preserves readable paragraph order and places figures inline after their source-adjacent paragraph, with captions. Tables, exercises, sidebars, and complex multi-column layouts are still mostly flattened or skipped by the text import pipeline.

### Reader Rendering

Relevant frontend files:

- `src/components/Reader/Reader.tsx`
- `src/components/Reader/ParagraphView.tsx`
- `src/components/Reader/MathText.tsx`
- `src/lib/mathContent.ts`
- `src/styles/index.css`

`Reader.tsx` no longer renders all section images as a gallery above text. It groups images by `anchorParagraphOrdinal` and renders them in flow.

### Tauri Asset Protocol

Downloaded local images are displayed with `convertFileSrc`, so the Tauri asset protocol must stay enabled:

- `src-tauri/tauri.conf.json`: `app.security.assetProtocol` enables and scopes `$APPDATA/covers/**` and `$APPDATA/images/**`.
- `src-tauri/Cargo.toml`: Tauri dependency includes the `protocol-asset` feature.
- `Cargo.lock`: includes `http-range`, pulled in by Tauri for asset serving.

If local images do not render, check CSP and asset protocol config first.

### Math Rendering And Speech

KaTeX was added to `package.json`.

Relevant files:

- `src/components/Reader/MathText.tsx`
- `src/lib/mathContent.ts`
- `src-tauri/src/content/normalize.rs`
- `src-tauri/src/content/openstax.rs`
- `src/main.tsx`
- `src/stores/player.ts`
- `src/components/Reader/SupertonicChapterExport.tsx`

OpenStax MathML is encoded as `[[mathml:<base64>]]` tokens during import, rendered in the reader, and normalized for TTS.

## Testing And Verification

Current counts on `main`: **226 Rust tests** (3 ignored — the live network import smoke plus
two others) and **240 frontend tests across 23 files**. Counts drift — run the suites rather than trusting these.

These commands were green before handoff:

```bash
npm run build
cargo check -p libretexts-reader
cargo test -p libretexts-reader
cargo test -p libretexts-reader live_imports_small_public_book_with_images -- --ignored --nocapture
npm run tauri -- build --debug --no-bundle
git diff --check
```

The live LibreTexts smoke test imports a small public book and verifies at least one downloaded image persists. It requires network access and uses a temporary app data directory.

## Manual Test Flow

For the LibreTexts image/layout feature:

1. Build and open the debug binary:

   ```bash
   npm run tauri -- build --debug --no-bundle
   ./target/debug/libretexts-reader
   ```

2. Import a LibreTexts book with figures, for example General Biology.
3. Open a chapter that has figures.
4. Confirm:
   - Images display inside the reader.
   - Captions display below images.
   - Navigation/listing thumbnails are not imported as figures.
   - Figures appear near the related paragraphs, not collected at the top.

Important: any LibreTexts book imported before the image-anchor change should be deleted and reimported to test accurate placement.

## Moving CI off macOS — DONE (2026-08-17, PR #11)

`verify` runs on `ubuntu-latest`. `release.yml` stays on `macos-14` because it
builds and signs the macOS app. The pre-flight analysis held up exactly as
written — no `cfg(target_os)` surprises, the XDG fallthrough in `paths.rs` is
real, `check-app-data-isolation.sh` already exported `XDG_DATA_HOME`, and
Tauri's documented Ubuntu package list was sufficient on the first try.

**What the analysis did not predict is the part worth reading.** Moving to Linux
did not merely need a package list; it *executed code that had never run
anywhere*, and that code was broken in two ways. `build.rs` is compiled for the
host on every platform, so the Linux-only branches typechecked on every machine
anyone had ever used while never once executing. Compile coverage 100%,
execution coverage 0% — the shape that survives code review indefinitely.

1. **The ffmpeg SHA-256 was pinned against BtbN's rolling `latest` tag**, which
   BtbN rebuilds daily under unchanged `ffmpeg-master-latest-*` filenames. All
   three BtbN pins (Linux x64, Linux arm64, **and Windows**) were stale and had
   been for days. macOS never noticed because it pulls ffmpeg from ColorsWind at
   a fixed tag. Fixed by pinning `FFMPEG_BTBN_RELEASE` to a dated `autobuild-*`
   release, where the tag, asset filename and SHA all move together. **Bumping
   it means changing the tag, all three asset names and all three SHAs as one
   edit** — regenerate the SHAs by downloading and hashing, never by copying the
   value out of a failure message.
2. **`extract_ffmpeg_tar` unpacked into a directory that was never created.**
   `extract_ffmpeg` removes `libs_dir` and never creates it, and
   `tar::Entry::unpack` does not create parents — unlike the zip branch, which
   writes through `copy_reader_to_path` and calls `create_dir_all` itself. Two
   extraction paths, only one with the safety built in.

A third defect in the same branch was issue #12, **fixed on 2026-08-17 via PR #16.**
The tar loop skipped every non-regular-file entry, and ffmpeg's `lib/` is 14
symlinks to 7 real files, so the SONAME links (`libavdevice.so.63` →
`libavdevice.so.63.2.100`) were dropped and the extracted Linux ffmpeg could not
resolve a single one of its libraries.

**Two things from that fix are worth carrying, and the second is a landmine.**

**1. There is now a gate, and it is the only thing that executes the bundle.**
`scripts/ci/check-ffmpeg-bundle.sh` runs `ldd` against the extracted sidecar with
`LD_LIBRARY_PATH` pointed at the extracted libs, and fails if any `DT_NEEDED`
entry is unresolved. It asks the dynamic loader rather than counting files —
a count passes on the 7 real `.so` files while the names the loader actually
wants are missing. Nothing else touches ffmpeg at runtime: it appears only in
`build.rs` and `tauri.conf.json`, with **zero** references anywhere under
`src-tauri/src`.

**2. Changing how an archive is extracted means bumping its `*_EXTRACT_VERSION`
in `build.rs`.** The symlink fix was committed, was correct, and did *nothing* —
CI reported the identical seven unresolvable libraries against it. `ensure_pdfium`
and `ensure_ffmpeg` early-return when the output exists and a `.sha256` marker
matches, and that marker recorded only `archive_sha256`, which identifies the
bytes downloaded and says nothing about what was unpacked from them. CI restored
a cached tree extracted by the *old* logic through the
`restore-keys: native-assets-<os>-` prefix fallback, the marker matched, and
extraction was skipped. The markers now record an extraction version too
(`<sha> extract2`), per asset. If you change an extractor and see no effect,
this is why.

The general lesson, stated because it cost two CI rounds to see: a cache keyed on
an *input* cannot detect a change in the *code that consumes it*. The ci.yml
comment used to assert the opposite outright.

**Timing, measured 2026-08-17.** Warm Linux is **5m39s**; cold was 17m25s. Do
not read the runs immediately after a merge as the steady state: `save-if`/`if:`
limit cache saves to `main`, so PR #13 started before `main`'s saving run had
finished and restored nothing — both its 16m41s and the first run's 17m29s were
effectively cold. The Linux entries exist now (~1.0GB rust, 61MB native assets,
50MB npm).

**The win is billing, not speed, and the difference matters if anyone proposes
reverting.** Linux is genuinely *slower* in wall clock than macOS was — 5m39s
against 2m52s — because the macOS runners have faster CPUs and this build is
compile-bound. What changed is the multiplier:

| | `macos-14` | `ubuntu-latest` |
| --- | --- | --- |
| Wall clock, warm | ~2m52s | 5m39s |
| Multiplier | 10× | 1× |
| **Billable minutes** | **~29** | **~6** |

So it is roughly a **5× cost improvement, not the 10× the raw multiplier
suggests**, bought with about three extra minutes of wall clock.

Also on 2026-08-17, **PR #13 moved the actions off deprecated Node 20**:
`actions/checkout` v4→v7, `actions/setup-node` v4→v7, `actions/cache` v4→v6.
`Swatinem/rust-cache@v2` was never in the annotation and is unchanged. The same
`checkout` bump was applied to `release.yml`, but **that half is unverified** —
`release.yml` only runs on a tag, so the next release is where it gets tested.

## Known Limitations And Next Steps

**The GitHub tracker is the source of truth for this, not this section.** A
release-readiness audit on 2026-08-20 filed 22 issues (#48-#69) covering everything below
and a good deal this section had never recorded. What follows is a map, not a list — read
it to know what kind of thing is wrong, then read the tracker for the specifics.

The previous version of this section was written before Fish Audio shipped and listed
none of the actual blockers. **If it looks stale again, distrust it and check the
tracker.**

### The plan: a private beta (decided 2026-08-20)

**Shape:** fewer than ten named testers, all reachable by email, given access to this
private repo and a Release published here. The build **is** signed and notarized. The repo
stays private; going public is a separate, later decision.

The issues that gate it carry the **`Private beta` milestone**. **That milestone is the
working list and is authoritative over this paragraph** — run
`gh issue list --milestone "Private beta"` rather than trusting what follows, which is a
snapshot and will drift.

Open as of 2026-08-23 — **seven on the tracker, zero on the milestone**. The `Private beta`
milestone is 11/11 closed; #48, #50, #54 and #102 are all done. Nothing below blocks a beta:

- **#59** — playback position is written but never read back. No resume, every progress bar
  stuck at zero. The most visible of these to someone actually listening to a textbook.
- **#61** — an import cannot be cancelled, and one running import disables Add across every
  catalog.
- **#65** — no third-party dependency attribution, and the Supertonic model's licence is
  unrecorded. The sharper half is the model: testers keep exported audio generated from it.
- **#57** — using "LibreTexts" as the product name while unaffiliated. Explicitly scoped to a
  *public* beta, so it does not gate the private one. Needs triage.
- **#63 / #64 / #69** — dead settings rows, seven unused CSP hosts, content-fidelity gaps.
  Cleanup. #69 needs triage.

**What #66 changed about the release.** `release.yml` runs the real gate before it builds
anything publishable, and its timeout has headroom for a slow notarization. Both halves are now
**proven**: the `ci.yml` side on every PR, and the `release.yml` side by the dry run and the
v0.1.0-beta.1 tag run on 2026-08-23. The caution that used to sit here — "do not read #66 as
'the release pipeline works'" — has been satisfied rather than removed.

**Cleared 2026-08-22** — the whole first-run download chain, in one day. #52, made visible and
cancellable ([PR #86](https://github.com/johnnylibretexts/libretexts-reader/pull/86), `ebea718`);
#87, made resumable ([PR #89](https://github.com/johnnylibretexts/libretexts-reader/pull/89),
`5135ded`); and #88, made single-flight
([PR #91](https://github.com/johnnylibretexts/libretexts-reader/pull/91), `0585e1c`). Each was
found by the one before it. Then two more the same day, off the milestone: **#76** (the
chapter-export panel forgot its voice,
[PR #93](https://github.com/johnnylibretexts/libretexts-reader/pull/93), `d9a3b6d`) and **#62**
(four silent error swallows,
[PR #94](https://github.com/johnnylibretexts/libretexts-reader/pull/94), `b6ebb17`). Then the
release thread: **#66** (release.yml ran no tests,
[PR #96](https://github.com/johnnylibretexts/libretexts-reader/pull/96), `66e8727`) and **#51**
(licence and attribution surfaced and tagged,
[PR #97](https://github.com/johnnylibretexts/libretexts-reader/pull/97), `22a2af5`).

**Eight issues, ten PRs, every CI run green first try.** Tests went 209 Rust / 203 frontend to
226 / 240. All are written up under "Session of 2026-08-22" in Recently Landed.

**Three tickets have now been wrong about their own premise: #30, #62 and #66.** All three were
caught by reproducing before implementing, and #62 and #66 were corrected on the ticket itself —
#62's AC 2 and #66's AC 3 now read as what actually landed, with the original struck through and
dated. **Reproduce first. It is three for three that the ticket text, not the code, was the
thing at fault.**

**`SupertonicDownloadCancel` is no longer managed state** — `SupertonicDownload` is, and it owns
both the cancel flag and the single download slot. Read the #88 entry before touching either.

**Cleared 2026-08-21** — #60, the voice-style setting reaching playback
([PR #77](https://github.com/johnnylibretexts/libretexts-reader/pull/77), `38c9fc5`). See
"5. The Supertonic voice style reaches playback" under Recently Landed for the invariants
it established; read that before touching playback or the settings store.

**Not blocking the beta, filed off the back of #60:** #78 (three low-severity review items,
one a real regression — a Fish voice picked from the dropdown shows no save confirmation),
closed 2026-08-21 by [PR #80](https://github.com/johnnylibretexts/libretexts-reader/pull/80),
and **#76 (the chapter-export panel forgets its voice when the Reader unmounts), still open**.
Neither was ever on the milestone.

**Cleared 2026-08-20** — the cheap three, all merged: #67 (`bundle.targets` now declares
only what a release builds), #55 (the false privacy claims corrected and `PRIVACY.md`
added with the full host table), #58 (the debug status table, placeholder route subtitles,
the permanently-disabled Export item, and stale empty-state copy all removed, with a new
`Grid.test.tsx` covering it).

**What the decision retired.** #56 (distribution) is closed — a private cohort needs no
public artifact URL. #68 (no auto-updater) is closed as deferred: ten emailable people can
simply be told to re-download, which defuses the ordering trap that made it urgent. **Both
should be reopened as live questions before any public step** — #68 especially, because by
then there will be two cohorts to migrate by hand rather than one.

**What it did *not* retire.** Distribution to testers is still distribution, so the LGPL
and CC attribution obligations (#50, #51) attach exactly as they would publicly. And #54
*escalates*: charging ten people you know personally, with no warning and no way to stop
it, is worse than doing it to strangers.

**Deferred to the public decision:** #57 (the LibreTexts name — but note renaming gets
strictly more expensive with every install, and ten testers is the cheapest it will ever
be), #69 (content fidelity), and the risk-tier issues #59, #61-#66.

### Import fidelity — still true, unchanged

These are design consequences of paragraph-flow import, not defects, and they have been
true since the beginning:

- The reader does not mirror source page layout. It preserves paragraph order and figures, not full HTML sections, sidebars, exercises, tables, or CSS. **Skipped content leaves no marker**, which is the part that reads as a bug to a user — see #69.
- Image placement is paragraph-anchored (`anchor_paragraph_ordinal`), not DOM-node exact.
- An image appearing before any readable paragraph has a null anchor and renders before the first paragraph.
- Section images load for the active section only.
- Math speech normalization is heuristic — common LaTeX/MathML patterns, not accessibility-grade math speech.
- Imports predating migration `0004` have null anchors and need reimporting. Irrelevant to a fresh install; only affects long-lived local databases.

### Longer-term direction, if layout fidelity becomes the goal

Unchanged from the previous version of this section, and still the right shape if that
decision is taken — but it is **not** the current priority, and #69 should settle the
question of how much fidelity is actually wanted first:

1. Introduce a structured content block model rather than paragraphs plus anchored images.
2. Add block types for headings, paragraphs, figures, tables, examples/exercises, callouts, and equations.
3. Persist blocks in SQLite with source ordinal; render a section as a sequence of blocks.
4. Add screenshot or UI tests over representative chapters with figures, tables, and callouts.
5. Add an import version so old imports can be detected and a reimport prompted.

## Cautions For The Next Codex Session

- Do not run `git reset --hard` or checkout files to "clean up" unless the user explicitly asks. (As of 2026-08-13 the tree is clean and everything is on `main`, but this rule stands for any new WIP.)
- Prefer `rg` and targeted file reads when orienting.
- Use `apply_patch` for edits.
- Re-run both frontend and Rust checks after touching shared models or migrations.
- If changing migrations while a local app database already exists, add a new migration instead of mutating an already-applied migration.
- When testing the running app, quit any old `target/debug/libretexts-reader` process before launching a newly built binary so macOS does not focus a stale instance.
- Launch the debug binary directly (`./target/debug/libretexts-reader`). `open` on a `--no-bundle` binary can exit 0 without starting anything, which looks like a broken build.
- Tests must not call `paths::` helpers at all. `src-tauri/src/paths.rs` calls `create_dir_all` on every path it resolves, so a test that asks for one silently creates and writes the real `~/Library/Application Support/dev.johnnylibretexts.reader` tree. Pass the directory in explicitly instead — `cache::cache_path_in` and `cleanup::reclaim_in` both do. Do **not** use `LIBRETEXTS_READER_APP_DATA_DIR` for this: `set_var` is process-global and Rust tests share one process, so it can race. Enforced by `scripts/ci/check-app-data-isolation.sh`, which runs the suite under a throwaway `$HOME` and fails if anything appears there. (Issue #2, fixed 2026-08-13.)
