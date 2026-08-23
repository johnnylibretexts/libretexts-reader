# AGENTS.md

LibreTexts Reader is a free, open-source **Tauri 2 desktop app** that reads OpenStax/LibreTexts textbooks, EPUBs, PDFs, pasted text, and article URLs aloud with on-device neural TTS. Stack: **React 19 + Vite 6 + TypeScript + Tailwind v4** in the webview (`src/`); **Rust** for content import, SQLite persistence, and part of TTS (`src-tauri/`). No backend server — everything runs locally. Apache-2.0.

## Setup

Required tooling:
- **Node 22.x** (last verified 22.20.0 / npm 10.9.3). Do **not** use Node 24 — Vite/Rollup native addon loading hangs. If your shell defaults to 24: `source "$HOME/.nvm/nvm.sh" && nvm use 22.20.0`.
- **Rust stable** via rustup — `rust-toolchain.toml` pins it and adds `clippy` + `rustfmt` (workspace `rust-version = 1.88`).
- macOS: Xcode Command Line Tools. First build needs **network** (Cargo crates + `src-tauri/build.rs` fetching bundled PDFium).

```bash
npm install
cargo check -p libretexts-reader
npm run build
```

If a copied `target/` fails with stale absolute paths, `cargo clean` fixes it.

## Build & Run

```bash
npm run tauri:dev          # live development (Vite + Rust, hot reload)

# fast runnable binary without packaging installers:
npm run tauri -- build --debug --no-bundle
./target/debug/libretexts-reader           # run it directly; quit any stale instance first

npm run tauri:build        # full release bundle → target/release/bundle/{dmg,macos}/...
```

Run the debug binary **directly**, not via `open`. `--no-bundle` produces a bare Mach-O with no `.app`, and `open` on it can report success while starting nothing at all — no window, no process, no error.

Frontend-only build/typecheck: `npm run build` (`tsc && vite build`). Vite dev server is pinned to port 1420 (`strictPort`).

## Testing

```bash
npm run build                               # TypeScript typecheck + frontend build (the frontend gate)
npm test                                    # vitest run — frontend unit tests (jsdom)
cargo check -p libretexts-reader                # Rust typecheck
cargo test -p libretexts-reader                 # Rust unit/integration tests
git diff --check                            # whitespace/conflict-marker gate
```

- **Before a change is done**, the frontend build, `npm test`, and `cargo test -p libretexts-reader` must all pass; re-run the Rust and frontend gates **both** whenever you touch shared DB models or migrations.
- Live network smoke (opt-in, uses a temp app-data dir): `cargo test -p libretexts-reader live_imports_small_public_book_with_images -- --ignored --nocapture`.
- **Frontend unit tests run under vitest** with a jsdom environment (`npm test`, or `npx vitest` to watch). Coverage is currently the pure-logic seams — `src/lib/errors.test.ts`, `src/lib/mathContent.test.ts`, `src/stores/player.test.ts` — not components. Rendering and playback behaviour still need verifying by running the debug binary; point runs at a scratch library with `LIBRETEXTS_READER_APP_DATA_DIR=/tmp/ltr-test`. **Warning:** `app.security.assetProtocol.scope` in `tauri.conf.json` does not follow this env var — it stays hardcoded to the real `$APPDATA` scope — so covers and section-figure images will not render under an overridden app-data dir. Do not use this technique to verify image rendering; use the real app-data dir (or the live-import smoke test) for that.

## Code Style

- Rust: `cargo fmt` + `cargo clippy`. Commands are `#[tauri::command]` fns in `src-tauri/src/commands/`, registered in `generate_handler!` in `src-tauri/src/lib.rs`. Content importers live in `src-tauri/src/content/`, DB access in `src-tauri/src/db/` (`rusqlite` + `r2d2` pool).
- TypeScript/React: functional components, **Zustand** stores (`src/stores/`), Tailwind v4. All calls into Rust go through the typed wrappers in `src/lib/tauri.ts`; payload types live in `src/types/domain.ts`. Keep the wrapper list, the `generate_handler!` list, and the type definitions in sync.
- Database migrations: add a new numbered SQL file in `src-tauri/resources/migrations/`, numbered one past the highest existing file (currently `0006`, so the next free number is `0007`). The `MIGRATIONS` array in `src-tauri/src/db/migrations.rs` is hand-maintained and is the actual source of truth — always check both it and the directory listing before picking a number; a collision registers under the wrong name and silently applies out of order. **Never edit an already-applied migration** — a new one is required so existing local databases upgrade cleanly.

## Commit & PR Conventions

- Git repo (default branch `main`, PRs used; Dependabot enabled). Commits use short imperative prefixes: `build:`, `deps:`, `fix:`, `chore:`, `license:`, `review:`.
- Re-run frontend + Rust checks before committing. Keep the intentionally-dirty WIP in mind: **do not `git reset --hard` or checkout files to "clean up" unless explicitly asked** — uncommitted feature work is the source of truth (see `HANDOFF.md`).

## Security & Data

- **On-device / offline by design.** The library, downloaded books, TTS models, and images live in the OS app-data dir (`~/Library/Application Support/dev.johnnylibretexts.reader`), never in the repo. Nothing is uploaded.
- **The CSP governs what the webview may load, not what the app may reach.** Every outbound request is Rust's, through `reqwest`, which the webview CSP never sees — which is why `api.fish.audio` has no entry despite being contacted on every Fish request, and why **adding a Source is not a reason to widen the CSP**. `connect-src` is `'self'`: the webview makes no requests of its own, and `scripts/ci/check-csp.sh` keeps it that way. `img-src` does carry publisher hosts, because the catalog browsers render cover thumbnails straight from their URLs (`OpenStaxBrowser.tsx:253` and its two siblings). Keep the `assetProtocol` scope tight (`$APPDATA/covers/**`, `images/**`). What the app actually contacts is tabulated in `PRIVACY.md`.
- Bundled native binaries/models are gitignored (`src-tauri/binaries/`, `resources/pdfium/`, `resources/voices/`) — never commit them.
- **Release builds are signed + notarized** (macOS Developer ID). `libpdfium.dylib` must be signed manually with hardened runtime before `tauri:build`, and notarization secrets are stored in a `notarytool` keychain profile — never in the shell or the repo. Full checklist: `RELEASE.md`.
- Distributed bundles include third-party components under their own licenses. `LICENSES/NOTICE-third-party.md` covers the whole dependency tree and is **generated** — run `scripts/generate-notices.sh` after changing a dependency and commit the result; `scripts/ci/check-notices.sh` fails the build when it drifts from the lockfiles. The bundled natives (PDFium; `id3`, `mp4ameta`) keep their own full notices beside it, and `build.rs` mirrors the whole directory into `src-tauri/resources/LICENSES/` so it ships inside the `.app`. No LGPL component is bundled any more: ffmpeg was removed unused, and LAME went with the move to AudioToolbox (ADR-0004). The Supertonic voice model is downloaded on the reader's machine rather than distributed, which is why it is recorded separately in `LICENSES/supertonic-model.md` rather than in the notice file.

## Agent skills

### Issue tracker

Issues live in GitHub Issues on the private repo `johnnylibretexts/libretexts-reader`, driven via the `gh` CLI. Note this machine also holds credentials for the `johnnyrobot` account this project was developed under — confirm the active account before writes. See `docs/agents/issue-tracker.md`.

### Triage labels

Default vocabulary — each canonical role's label string equals its name: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: `CONTEXT.md` + `docs/adr/` at the repo root. The React frontend and the Rust/Tauri backend are two layers of one application, not two bounded contexts. See `docs/agents/domain.md`.
