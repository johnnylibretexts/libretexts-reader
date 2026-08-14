# LibreTexts Reader Handoff

Last updated: 2026-08-13

This repo is an in-progress Tauri desktop app for reading and listening to OpenStax, LibreTexts, EPUB, PDF, pasted text, and article imports with local TTS.

**The working tree is now clean and everything is committed on `main`.** The previously-uncommitted feature work described below was committed and merged on 2026-08-13; treat `main` as the source of truth, not the worktree. The standing caution against `git reset --hard` still applies to any new WIP, but there is none as of this update.

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

**There is no work in progress.** This section used to track an uncommitted change set; everything in it is merged and pushed. It is kept as a map of what the app gained most recently, newest wave first.

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

1. **No regression test on the final fix wave.** A reviewer mechanically reverted the store change and re-ran the suite: all 7 tests still passed. `beforeEach` resets the store to all-null, so `expect(error).toBeNull()` after a success asserts a value that was already null — it never exercises the stale-error interleaving. A mutation-killing test seeds a non-null `error` via `useImportsStore.setState(...)` before a resolving `start()` and asserts it clears, plus the mirror for `completed`. ~15 lines. Without it, deleting `error: null` ships green.
2. **A spec requirement no task implemented.** The spec requires an in-library card to show "In library" **with an Open action in place of + Add**; both browsers render a static span. Tell-tale: `findImportedBook` returns a full `Document` but both call sites use only its truthiness. Either implement Open or amend the spec.

Also carried, each belonging with other work: `active` has no user-clearable escape if `run()` never settles (mitigated by 15–20s request timeouts; belongs with cancellation), and quitting mid-import orphans image files (belongs with the deterministic-filenames follow-on — see `6c1262c`).

### 2. Kokoro removal (2026-08-13, `09d97b3`)

Supertonic is now the only bundled engine. See **Next Up** below and ADR-0003 for the reasoning; migration `0007_drop_kokoro_voices` drops the `voices` table and rewrites any stored `kokoro` provider/voice id, and `db/settings.rs` additionally coerces a stored `kokoro` provider to `supertonic` on read for databases that somehow skip it. The `model_precision` setting and the whole first-run model chooser are gone with it.

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

## Next Up — TTS direction (decided 2026-08-13; A done, B not yet started)

**Decision: drop Kokoro. Supertonic becomes the only bundled engine. Add Fish Audio as an
optional provider where the user supplies their own API key.** Supertonic stays the default.

Sequence these as **two separate specs**, A before B. A is mostly deletion and it removes a
broken engine plus its workarounds before anything new is added, so Fish lands in a
two-case registry instead of a three-case one.

### A. Remove Kokoro — DONE (2026-08-13)

Spec: `docs/superpowers/specs/2026-08-13-remove-kokoro-design.md`.
Plan: `docs/superpowers/plans/2026-08-13-remove-kokoro.md`. See ADR-0003.

### B. Add Fish Audio (bring-your-own API key)

Reference docs the user supplied: <https://docs.fish.audio/overview/capabilities>,
<https://docs.fish.audio/features/text-to-speech>,
<https://docs.fish.audio/developer-guide/core-features/fine-grained-control>,
<https://fish.audio/blog/s2-1-pro-free-api/>. **Two skills exist for this** — prefer them
over reading the docs by hand: `fish-audio-api` (raw REST/WebSocket) and `fish-audio-sdk`
(official SDKs). Raw HTTP from Rust is likely the right call here; see below.

Three things are known and settled from exploration:

1. **The abstraction is ready.** `SpeechEngine` (`src/lib/speech/types.ts`) needs only `id`,
   `defaultVoice`, `synthesize()`, `ensureReady()`, `listVoices()`. `createSpeechEngine` is
   explicitly documented as the single place engines are chosen. Fish maps cleanly:
   `ensureReady` validates the key, `listVoices` lists the account's voice models.
2. **There is no secret storage in this app, and this is the central open question.**
   Settings are plain rows in the SQLite `settings` table. Installed Tauri plugins are only
   `dialog`, `fs`, `shell` — no keychain, no stronghold, and no `keyring` crate in
   `src-tauri/Cargo.toml`. Decide deliberately where a user's API key lives before writing
   any code. Storing it as a plain settings row is the path of least resistance and the
   least defensible.
3. **The CSP will block Fish until changed.** `connect-src` in
   `src-tauri/tauri.conf.json` lists OpenStax, LibreTexts, HuggingFace, GitHub and jsDelivr.
   `https://api.fish.audio` must be added. Note the app also claims "on-device / offline by
   design" in `CLAUDE.md` — a cloud TTS provider needs that claim reworded, and needs to
   degrade sanely with no network.

Open questions for the B spec: where the key is stored; whether the key is validated at
entry or first use; what the onboarding flow looks like (Fish account → developer account →
generated key); whether usage/cost is surfaced; what happens mid-playback when the network
drops.

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

**Open issues** on `johnnylibretexts/libretexts-reader` as of 2026-08-13: **#1** (harden the `check-identifier.sh` scope guard against two false-pass paths) and **#2** (the Rust test suite writes into the real macOS app-data directory — `cache_path_for_chapter` creates dirs as a side effect). **#3** (model precision is a one-way door) was closed as obsolete: the Kokoro removal deleted the setting it described.

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

## Known Limitations And Next Steps

- The reader does not yet mirror full LibreTexts page layout. It approximates the textbook flow by preserving paragraphs and figures, not full HTML sections, sidebars, exercises, tables, or CSS.
- Image placement is paragraph-anchored, not DOM-node exact. It is good enough for figure-in-reading-flow behavior, but not a pixel-perfect textbook clone.
- Existing imported documents before migration `0004` lack anchors and should be reimported.
- Chapter/section images are currently loaded for the active section only.
- If a page starts with an image before any readable paragraph, that image has a null anchor and renders before the first paragraph.
- Supertonic math speech normalization is heuristic. It handles common LaTeX/MathML patterns, not a full accessibility-grade math speech system.

Recommended next work:

1. Decide whether "mirror LibreTexts layout" means richer HTML preservation. If yes, introduce a structured content block model instead of only paragraphs plus anchored images.
2. Add block types for headings, paragraphs, figures, tables, examples/exercises, callouts, and equations.
3. Persist blocks in SQLite with source ordinal, then render a section as a sequence of blocks.
4. Add UI tests or Playwright/manual screenshot checks for representative LibreTexts chapters with figures, tables, and callouts.
5. Consider a migration/import version so old imports can be detected and prompted for reimport.

## Cautions For The Next Codex Session

- Do not run `git reset --hard` or checkout files to "clean up" unless the user explicitly asks. (As of 2026-08-13 the tree is clean and everything is on `main`, but this rule stands for any new WIP.)
- Prefer `rg` and targeted file reads when orienting.
- Use `apply_patch` for edits.
- Re-run both frontend and Rust checks after touching shared models or migrations.
- If changing migrations while a local app database already exists, add a new migration instead of mutating an already-applied migration.
- When testing the running app, quit any old `target/debug/libretexts-reader` process before launching a newly built binary so macOS does not focus a stale instance.
- Launch the debug binary directly (`./target/debug/libretexts-reader`). `open` on a `--no-bundle` binary can exit 0 without starting anything, which looks like a broken build.
- Tests must not call `paths::` helpers at all. `src-tauri/src/paths.rs` calls `create_dir_all` on every path it resolves, so a test that asks for one silently creates and writes the real `~/Library/Application Support/dev.johnnylibretexts.reader` tree. Pass the directory in explicitly instead — `cache::cache_path_in` and `cleanup::reclaim_in` both do. Do **not** use `LIBRETEXTS_READER_APP_DATA_DIR` for this: `set_var` is process-global and Rust tests share one process, so it can race. Enforced by `scripts/ci/check-app-data-isolation.sh`, which runs the suite under a throwaway `$HOME` and fails if anything appears there. (Issue #2, fixed 2026-08-13.)
