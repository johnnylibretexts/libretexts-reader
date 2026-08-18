# CLAUDE.md

LibreTexts Reader is a **Tauri 2 desktop app** for listening to OpenStax/LibreTexts textbooks, EPUBs, PDFs, pasted text, and article URLs with neural TTS. React 19 + Vite 6 + TypeScript webview frontend; Rust backend does content import, persistence, and part of TTS. There is no app server. Apache-2.0.

**Local by default, with one optional cloud provider.** The bundled **Supertonic** engine is on-device and works fully offline — no account, no key, no network. **Fish Audio** is an optional second engine the reader configures with their own API key; only requests to Fish (playback synthesis, chapter export) leave the machine, and only once a key is saved. Everything else — import, library, playback control, Supertonic itself — has no server dependency.

## Architecture

Two halves that talk over Tauri's `invoke` bridge:

**Frontend — `src/`** (React 19 + Zustand + Tailwind v4, Vite dev server on fixed port 1420)
- `components/` — `AppShell`, `Sidebar`, `MiniPlayer`, and feature dirs: `Reader/` (`Reader.tsx`, `ParagraphView.tsx`, `MathText.tsx`), `Import/`, `Library/`, `OpenStaxBrowser/`, `LibreTextsBrowser/`, `Settings/`.
- `stores/` — Zustand: `library.ts`, `player.ts`, `settings.ts`.
- `lib/` — `tauri.ts` (typed wrappers over every Rust command; **the invoke boundary**), `supertonic.ts` (the TTS engine), `mathContent.ts` (MathML/KaTeX handling), `errors.ts`.

**Backend — `src-tauri/src/`** (Rust; crate `libretexts-reader`, lib `libretexts_reader_lib`)
- `lib.rs` — Tauri builder: registers all `#[tauri::command]`s in `generate_handler!`, initializes the SQLite pool, and creates app-data subdirs on `setup`. **Adding a command = add the fn + register it here + add a wrapper in `src/lib/tauri.ts`.**
- `commands/` — `content.rs` (imports + catalog listing), `library.rs`, `playback.rs`, `settings.rs`, `tts.rs`, `chapter_tts.rs`, `fish.rs` (key management, no getter).
- `content/` — importers/normalizers: `openstax.rs`, `libretexts.rs`, `epub.rs`, `pdf.rs` (PDFium), `article.rs` (readability), `images.rs` (download + persist figures), `normalize.rs`, `tokenize.rs`, `document.rs`.
- `db/` — `rusqlite` + `r2d2` pool (`connection.rs`), `migrations.rs` applies SQL files from `resources/migrations/`, `models.rs`, `library.rs`, `settings.rs`.
- `secrets.rs` — `SecretStore` trait over the OS keychain (`keyring` crate), holding the one secret this app has: the Fish Audio API key.
- `build.rs` — downloads/prepares bundled **PDFium** and **ffmpeg** assets on first build (needs network); `paths.rs` resolves the app-data dir.

**TTS runs in Rust, behind a `TtsProvider` trait** (`src-tauri/src/tts/provider.rs`), mirroring `SpeechEngine` in `src/lib/speech/types.ts` — same idea, two layers. Two implementations today: **Supertonic** (on-device, `Rust ONNX Runtime` / `ort`, on-demand model downloads) and **Fish Audio** (cloud, plain HTTP via `reqwest`, not the webview). `ffmpeg` (external sidecar bin) + `mp3lame` handle encoding for both. Kokoro was removed in favour of Supertonic (ADR-0003).

- **The webview is the single place an engine is chosen.** `createSpeechEngine` in `src/lib/speech/index.ts` picks the frontend `SpeechEngine`, and every Rust command that synthesizes speech (`synthesize_speech`, the chapter-export commands) takes the provider as a field on the request and dispatches on *that* — never by reading the `tts_provider` settings row itself. A command that fell back to a settings read would make the choice twice, from two sources with no ordering guarantee, which is the bug this rule replaced. Settings still supply the *parameters* a chosen provider needs (voice ids, Supertonic language) — just never which provider runs.
- **The Fish Audio API key lives in the OS keychain, not the SQLite `settings` table**, via `SecretStore`/`KeyringSecretStore` in `secrets.rs`. There is deliberately no Tauri command that returns the key to the webview — only presence/validity (`get_fish_key_status`) and a live balance (`get_fish_credit`). A getter would put the secret into the webview and into any devtools session, which is the one thing the keychain choice exists to prevent.
- Fish's HTTP client runs entirely in Rust (`reqwest`), so **it is not subject to the webview's Content-Security-Policy** — no `connect-src` entry for `api.fish.audio` is needed or present.

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

# fast runnable binary without installers (run it directly — NOT via `open`, see Gotchas):
npm run tauri -- build --debug --no-bundle && ./target/debug/libretexts-reader

# live network import smoke (ignored by default):
cargo test -p libretexts-reader live_imports_small_public_book_with_images -- --ignored --nocapture
```

Pre-commit/verification gate: `npm run build`, `npm test`, `cargo test -p libretexts-reader`, `git diff --check`.

## Conventions

- **Toolchains are pinned.** Rust: stable via rustup (`rust-toolchain.toml` adds `clippy`+`rustfmt`; workspace `rust-version = 1.88`). Use `cargo fmt` / `cargo clippy`. Frontend has no separate linter — its gates are TypeScript strictness via `tsc` in `npm run build` plus the vitest suite (`npm test`). CI enforces both.
- **Components are testable — use it.** `@testing-library/react` + `/user-event` + `/jest-dom` are wired up, and `src/test/setup.ts` registers `afterEach(cleanup)` (required: Testing Library only auto-cleans under `globals: true`, which this project does not set). Test files are `*.test.tsx` beside the component. This arrived late, so most components still have no test and several pure helpers exist only because a component could not be tested — `exportGate.ts` is the clearest case. Prefer a real component test now; don't extract a helper *solely* for testability. **When adding a test for a fix that is already applied, revert the fix and watch the test fail before trusting it** — a test written after the fact passes immediately and proves nothing.
- **Node 22.x is required** (last verified 22.20.0 / npm 10.9.3). See gotcha below.
- Frontend↔Rust contract: keep `src/lib/tauri.ts` and the `generate_handler!` list in `lib.rs` in sync; mirror payload shapes in `src/types/domain.ts`.
- DB: **add a new numbered migration** in `src-tauri/resources/migrations/`, numbered one past the highest file already there (currently `0009`, so the next free number is `0010`). The `MIGRATIONS` array in `src-tauri/src/db/migrations.rs` is hand-maintained and is the actual source of truth — check both it and the directory listing before picking a number, since a collision registers under the wrong name and silently applies out of order. Never mutate an already-applied migration file.
- Commits: Conventional-Commits-ish prefixes (`build:`, `deps:`, `license:`, `fix:`, `chore:`), imperative.

## Gotchas & Constraints

- **`open target/debug/libretexts-reader` does not launch the app.** `--no-bundle` produces a bare Mach-O with no `.app`, and `open` on it can exit 0 while starting nothing — no window, no process, no error, so it reads as "the app is broken". Run the binary directly: `./target/debug/libretexts-reader`.
- **Three declarations name the app-data directory and nothing in the code links them:** `identifier` in `tauri.conf.json`, `APP_DIR_NAME` in `src-tauri/src/paths.rs`, and the `assetProtocol` `$APPDATA/**` scope. Tauri derives `$APPDATA` from the identifier. If they drift the build stays green and every cover and figure silently stops rendering. `scripts/ci/check-identifier.sh` gates all three — keep it passing, and never change one of them alone.
- **`paths.rs` creates every directory it resolves** (`create_dir_all`). Merely *asking* for the app-data path materialises the whole tree, so a test that calls a `paths::` helper writes into the real `~/Library/Application Support/dev.johnnylibretexts.reader` and is indistinguishable from real usage on disk. **In tests, pass the directory in explicitly** — see `cache::cache_path_in` and `cleanup::reclaim_in`, which both take a root parameter and touch no filesystem. Do **not** reach for `LIBRETEXTS_READER_APP_DATA_DIR` in a unit test: `set_var` is process-global and Rust runs tests as threads in one process, so one test's override can race another's. `scripts/ci/check-app-data-isolation.sh` wraps `cargo test` and fails if anything lands under `$HOME`.
- **Node 24 hangs on Vite/Rollup native addons.** The fix in place is a dev-dependency alias `"rollup": "npm:@rollup/wasm-node@^4.60.2"` (this intentionally alters `package.json`/`package-lock.json` and drops native Rollup optional entries). **Do not revert it** unless native Rollup loading is fixed another way; prefer running on Node 22.
- **Tauri asset protocol must stay enabled** for downloaded local images to render (`convertFileSrc` → `asset:`). It's wired in `tauri.conf.json` (`app.security.assetProtocol` scoped to `$APPDATA/covers/**` + `images/**`, and the CSP `img-src`) and the `protocol-asset` Tauri feature in `Cargo.toml`. If local images don't show, check CSP + asset protocol first.
- **Content import is paragraph-flow, not a layout clone.** Figures are anchored to a nearby paragraph (`anchor_paragraph_ordinal`); tables/sidebars/exercises are flattened or skipped. Imports made before migration `0004` have null anchors — reimport to test placement.
- **Math** is encoded as `[[mathml:<base64>]]` tokens at import, rendered with KaTeX in the reader, and normalized heuristically for TTS on the Supertonic path — it is not accessibility-grade math speech.
- **Release signing is manual for bundled natives.** Tauri does not sign the bundled ffmpeg `.dylib`s or `libpdfium.dylib`; sign the source libs with Developer ID + hardened runtime **before** `tauri:build`, or notarization fails. Full runbook: `RELEASE.md`. The auto-updater is disabled in v0.1.0.
- `build.rs` needs **network on first build** to fetch PDFium/ffmpeg. Bundled binaries/models live in gitignored paths (`src-tauri/binaries/`, `resources/pdfium/`).
- The working tree is often intentionally dirty with uncommitted feature work — **do not `git reset --hard`/checkout to "clean up" unless asked.** See `HANDOFF.md` for current WIP and full context.

## Agent skills

### Issue tracker

Issues live in GitHub Issues on the private repo `johnnylibretexts/libretexts-reader`, driven via the `gh` CLI. Note this machine also holds credentials for the `johnnyrobot` account this project was developed under — confirm the active account before writes. See `docs/agents/issue-tracker.md`.

### Triage labels

Default vocabulary — each canonical role's label string equals its name. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: `CONTEXT.md` + `docs/adr/` at the repo root — both now exist. The React frontend and the Rust/Tauri backend are two layers of one application, not two bounded contexts. See `docs/agents/domain.md`.
