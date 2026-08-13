# CLAUDE.md

LibreTexts Reader is a **Tauri 2 desktop app** for listening to OpenStax/LibreTexts textbooks, EPUBs, PDFs, pasted text, and article URLs with **on-device neural TTS**. React 19 + Vite 6 + TypeScript webview frontend; Rust backend does content import, persistence, and part of TTS. Everything runs locally; no app server. Apache-2.0.

## Architecture

Two halves that talk over Tauri's `invoke` bridge:

**Frontend — `src/`** (React 19 + Zustand + Tailwind v4, Vite dev server on fixed port 1420)
- `components/` — `AppShell`, `Sidebar`, `MiniPlayer`, and feature dirs: `Reader/` (`Reader.tsx`, `ParagraphView.tsx`, `MathText.tsx`), `Import/`, `Library/`, `OpenStaxBrowser/`, `LibreTextsBrowser/`, `VoiceGallery/`, `Settings/`, `FirstRun/`.
- `stores/` — Zustand: `library.ts`, `player.ts`, `settings.ts`.
- `lib/` — `tauri.ts` (typed wrappers over every Rust command; **the invoke boundary**), `kokoro.ts` + `supertonic.ts` (TTS engines), `mathContent.ts` (MathML/KaTeX handling), `errors.ts`.

**Backend — `src-tauri/src/`** (Rust; crate `libretexts-reader`, lib `libretexts_reader_lib`)
- `lib.rs` — Tauri builder: registers all `#[tauri::command]`s in `generate_handler!`, initializes the SQLite pool, and creates app-data subdirs on `setup`. **Adding a command = add the fn + register it here + add a wrapper in `src/lib/tauri.ts`.**
- `commands/` — `content.rs` (imports + catalog listing), `library.rs`, `playback.rs`, `settings.rs`, `tts.rs`, `supertonic_tts.rs`, `voices.rs`.
- `content/` — importers/normalizers: `openstax.rs`, `libretexts.rs`, `epub.rs`, `pdf.rs` (PDFium), `article.rs` (readability), `images.rs` (download + persist figures), `normalize.rs`, `tokenize.rs`, `document.rs`.
- `db/` — `rusqlite` + `r2d2` pool (`connection.rs`), `migrations.rs` applies SQL files from `resources/migrations/`, `models.rs`, `library.rs`, `settings.rs`.
- `voices/` — TTS voice/model manifest + download bookkeeping.
- `build.rs` — downloads/prepares bundled **PDFium** and **ffmpeg** assets on first build (needs network); `paths.rs` resolves the app-data dir.

**TTS is split across two engines:** Kokoro runs **in the webview** via `kokoro-js` with an app-downloaded model; **Supertonic** playback and chapter-MP3 export run through the **Rust ONNX Runtime** (`ort`) backend with on-demand model downloads. `ffmpeg` (external sidecar bin) + `mp3lame` handle encoding.

**App data** lives outside the repo at `~/Library/Application Support/dev.johnnylibretexts.reader` (SQLite DB, downloaded books, TTS models, cover/section images). Point tests/local runs elsewhere with `LIBRETEXTS_READER_APP_DATA_DIR`.

## Commands

```bash
npm install                                 # JS deps
npm run tauri:dev                           # live dev (Vite + Rust, hot reload)
npm run build                               # frontend typecheck + build: tsc && vite build
npm test                                    # vitest run — frontend unit tests (jsdom)
cargo check -p libretexts-reader                # Rust typecheck
cargo test -p libretexts-reader                 # Rust tests
npm run tauri:build                         # full signed/bundled release build (dmg/app/msi/nsis)

# fast runnable binary without installers:
npm run tauri -- build --debug --no-bundle && open target/debug/libretexts-reader

# live network import smoke (ignored by default):
cargo test -p libretexts-reader live_imports_small_public_book_with_images -- --ignored --nocapture
```

Pre-commit/verification gate: `npm run build`, `npm test`, `cargo test -p libretexts-reader`, `git diff --check`.

## Conventions

- **Toolchains are pinned.** Rust: stable via rustup (`rust-toolchain.toml` adds `clippy`+`rustfmt`; workspace `rust-version = 1.88`). Use `cargo fmt` / `cargo clippy`. Frontend has no separate linter — its gates are TypeScript strictness via `tsc` in `npm run build` plus the vitest suite (`npm test`). CI enforces both.
- **Node 22.x is required** (last verified 22.20.0 / npm 10.9.3). See gotcha below.
- Frontend↔Rust contract: keep `src/lib/tauri.ts` and the `generate_handler!` list in `lib.rs` in sync; mirror payload shapes in `src/types/domain.ts`.
- DB: **add a new numbered migration** in `src-tauri/resources/migrations/`, numbered one past the highest file already there (currently `0006`, so the next free number is `0007`). The `MIGRATIONS` array in `src-tauri/src/db/migrations.rs` is hand-maintained and is the actual source of truth — check both it and the directory listing before picking a number, since a collision registers under the wrong name and silently applies out of order. Never mutate an already-applied migration file.
- Commits: Conventional-Commits-ish prefixes (`build:`, `deps:`, `license:`, `fix:`, `chore:`), imperative.

## Gotchas & Constraints

- **Node 24 hangs on Vite/Rollup native addons.** The fix in place is a dev-dependency alias `"rollup": "npm:@rollup/wasm-node@^4.60.2"` (this intentionally alters `package.json`/`package-lock.json` and drops native Rollup optional entries). **Do not revert it** unless native Rollup loading is fixed another way; prefer running on Node 22.
- **Tauri asset protocol must stay enabled** for downloaded local images to render (`convertFileSrc` → `asset:`). It's wired in `tauri.conf.json` (`app.security.assetProtocol` scoped to `$APPDATA/covers/**` + `images/**`, and the CSP `img-src`) and the `protocol-asset` Tauri feature in `Cargo.toml`. If local images don't show, check CSP + asset protocol first.
- **Content import is paragraph-flow, not a layout clone.** Figures are anchored to a nearby paragraph (`anchor_paragraph_ordinal`); tables/sidebars/exercises are flattened or skipped. Imports made before migration `0004` have null anchors — reimport to test placement.
- **Math** is encoded as `[[mathml:<base64>]]` tokens at import, rendered with KaTeX in the reader, and normalized heuristically for TTS across system/Kokoro/Supertonic paths — it is not accessibility-grade math speech.
- **Release signing is manual for bundled natives.** Tauri does not sign the bundled ffmpeg `.dylib`s or `libpdfium.dylib`; sign the source libs with Developer ID + hardened runtime **before** `tauri:build`, or notarization fails. Full runbook: `RELEASE.md`. The auto-updater is disabled in v0.1.0.
- `build.rs` needs **network on first build** to fetch PDFium/ffmpeg. Bundled binaries/models live in gitignored paths (`src-tauri/binaries/`, `resources/pdfium/`, `resources/voices/`).
- The working tree is often intentionally dirty with uncommitted feature work — **do not `git reset --hard`/checkout to "clean up" unless asked.** See `HANDOFF.md` for current WIP and full context.

## Agent skills

### Issue tracker

Issues live in GitHub Issues on the private repo `johnnylibretexts/libretexts-reader`, driven via the `gh` CLI. Note this machine also holds credentials for the `johnnyrobot` account this project was developed under — confirm the active account before writes. See `docs/agents/issue-tracker.md`.

### Triage labels

Default vocabulary — each canonical role's label string equals its name. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: `CONTEXT.md` + `docs/adr/` at the repo root — both now exist. The React frontend and the Rust/Tauri backend are two layers of one application, not two bounded contexts. See `docs/agents/domain.md`.
