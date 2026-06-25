# Johnny Reader Handoff

Last updated: 2026-05-20

This repo is an in-progress Tauri desktop app for reading and listening to OpenStax, LibreTexts, EPUB, PDF, pasted text, and article imports with local TTS. The current working tree has substantial uncommitted feature work. Do not reset, checkout, or discard local changes unless the user explicitly asks.

## Project Location

Work from wherever you cloned the repo, for example:

```bash
cd "$HOME/code/johnny-reader"
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
cargo check -p johnny-reader
npm run build
```

For a runnable desktop binary without packaging installers:

```bash
npm run tauri -- build --debug --no-bundle
open target/debug/johnny-reader
```

For live development:

```bash
npm run tauri:dev
```

If the copied folder includes `node_modules`, `target`, or `src-tauri/resources/pdfium` and `src-tauri/binaries`, setup may be faster, but the canonical setup is still `npm install` plus the build commands above.

## App Data

The app database and downloaded models/images are not stored in the repo. On macOS the app data directory is:

```bash
~/Library/Application Support/dev.johnnyrobot.reader
```

Copying only the project folder will not copy the local library, downloaded books, TTS models, cover images, or downloaded section images. To preserve a test library across machines, copy that app data directory too. The app can also use `JOHNNY_READER_APP_DATA_DIR` to point tests or local runs at a temporary data directory.

## Current Work In Progress

The current change set adds:

- KaTeX-based math rendering in the reader.
- MathML token preservation for imported textbook math.
- More math-aware TTS normalization for system/Kokoro/Supertonic paths.
- LibreTexts import support and browser improvements.
- Downloaded textbook figures for LibreTexts and OpenStax.
- SQLite persistence for section images.
- Inline figure placement in the reader based on source order.
- Tauri asset protocol support so downloaded local images can display in the webview.
- OpenStax catalog cards now visually match LibreTexts catalog cards, including thumbnails and source links.
- OpenStax bundled catalog now has `coverUrl` values for 95 of 112 books from the OpenStax CMS books API.
- Tauri CSP now allows OpenStax cover asset hosts: `https://assets.openstax.org` and `https://images.openstax.org`.

The working tree is intentionally dirty. At the time of this handoff, `git status --short` showed modified files across frontend, Rust content import, DB migrations, Tauri config, and new files:

```text
src-tauri/resources/migrations/0003_section_images.sql
src-tauri/resources/migrations/0004_section_image_anchors.sql
src-tauri/src/content/images.rs
src/components/Reader/MathText.tsx
src/lib/mathContent.ts
```

There are also many modified tracked files. Treat the local worktree as the source of truth, not the single initial commit.

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

All passed. The rebuilt app was opened with:

```bash
open target/debug/johnny-reader
```

At the end of the session the debug app was running as:

```text
<repo>/target/debug/johnny-reader
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
cargo check -p johnny-reader
cargo test -p johnny-reader
cargo test -p johnny-reader live_imports_small_public_book_with_images -- --ignored --nocapture
npm run tauri -- build --debug --no-bundle
git diff --check
```

The live LibreTexts smoke test imports a small public book and verifies at least one downloaded image persists. It requires network access and uses a temporary app data directory.

## Manual Test Flow

For the LibreTexts image/layout feature:

1. Build and open the debug binary:

   ```bash
   npm run tauri -- build --debug --no-bundle
   open target/debug/johnny-reader
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
- Supertonic/Kokoro math speech normalization is heuristic. It handles common LaTeX/MathML patterns, not a full accessibility-grade math speech system.
- Frontend bundle warnings remain for large chunks, especially `kokoro.web`. This was pre-existing and not addressed.

Recommended next work:

1. Decide whether "mirror LibreTexts layout" means richer HTML preservation. If yes, introduce a structured content block model instead of only paragraphs plus anchored images.
2. Add block types for headings, paragraphs, figures, tables, examples/exercises, callouts, and equations.
3. Persist blocks in SQLite with source ordinal, then render a section as a sequence of blocks.
4. Add UI tests or Playwright/manual screenshot checks for representative LibreTexts chapters with figures, tables, and callouts.
5. Consider a migration/import version so old imports can be detected and prompted for reimport.

## Cautions For The Next Codex Session

- Do not run `git reset --hard` or checkout files to "clean up" unless the user explicitly asks. The feature work is uncommitted.
- Prefer `rg` and targeted file reads when orienting.
- Use `apply_patch` for edits.
- Re-run both frontend and Rust checks after touching shared models or migrations.
- If changing migrations while a local app database already exists, add a new migration instead of mutating an already-applied migration.
- When testing the running app, quit any old `target/debug/johnny-reader` process before opening a newly built binary so macOS does not focus a stale instance.
