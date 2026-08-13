# Rename to LibreTexts Reader — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rename the application from *Johnny Reader* to *LibreTexts Reader* across its crate, package, bundle identifier, environment variables, and user-visible strings, moving the on-disk app data to match without losing the existing library.

**Architecture:** Five risk-tiered commits, ordered by blast radius, each independently green and revertible. The two hazards driving the order: the bundle identifier / `APP_DIR_NAME` / `$APPDATA` triple names one directory with nothing linking them, and image paths are persisted absolute so a directory move invalidates database rows. Each tier that changes a persisted string ships its own SQL migration rewriting only strings that tier introduces.

**Tech Stack:** Rust 1.88 (Tauri 2, rusqlite), React 19 + Vite 6 + TypeScript, vitest, bash CI scripts, GitHub Actions.

**Source spec:** `docs/superpowers/specs/2026-08-13-rename-libretexts-reader-design.md`

## Global Constraints

- **Node 22.x required** (last verified 22.20.0 / npm 10.9.3). Node 24 hangs on Vite/Rollup native addons. If your shell defaults to 24: `source "$HOME/.nvm/nvm.sh" && nvm use 22.20.0`.
- **Do not revert the rollup alias** `"rollup": "npm:@rollup/wasm-node@^4.60.2"` in `package.json`.
- **Never edit an already-applied migration.** `0001`–`0004` are frozen. New behaviour requires a new numbered file *and* a new entry in `const MIGRATIONS` in `src-tauri/src/db/migrations.rs` — the array is the registry; a `.sql` file alone is inert.
- **Do not `git reset --hard` or checkout files to "clean up"** unless explicitly asked. Uncommitted work may be the source of truth (see `HANDOFF.md`).
- **Full CI gate for every task** — `ci.yml` enforces format and lint, so tests alone are not enough:
  ```sh
  npm run build
  npm test
  cargo fmt --all --check
  cargo clippy --all-targets -- -D warnings
  cargo test -p libretexts-reader      # johnny-reader until Task 2 lands
  git diff --check
  ```
- **Exact strings** (copy verbatim, do not paraphrase):
  - old identifier `dev.johnnyrobot.reader` → new `dev.johnnylibretexts.reader`
  - old product `Johnny Reader` → new `LibreTexts Reader`
  - old crate `johnny-reader` → new `libretexts-reader`; old lib `johnny_reader_lib` → new `libretexts_reader_lib`
  - old env prefix `JOHNNY_READER_` → new `LIBRETEXTS_READER_`
  - old repo `johnnyrobot/johnny-reader` → new `johnnylibretexts/libretexts-reader`

## File Structure

| File | Responsibility | Tier |
|---|---|---|
| `.github/workflows/release.yml` | release guard, crate flag, bundle paths | 1, 2, 5 |
| `.github/workflows/ci.yml` | crate flag, new identifier gate | 2, 4 |
| `src-tauri/Cargo.toml`, `Cargo.toml`, `package.json` | package identity | 2 |
| `src-tauri/src/main.rs` | lib entry point name | 2 |
| 10 Rust files + `release.yml` | env var prefix | 3 |
| `src-tauri/tauri.conf.json` | identifier, productName, window title | 4, 5 |
| `src-tauri/src/paths.rs` | `APP_DIR_NAME` | 4 |
| `scripts/ci/check-identifier.sh` + `.test.sh` | **new** — encodes the identifier↔`APP_DIR_NAME` coupling as a gate | 4 |
| `src-tauri/resources/migrations/0005_*.sql` | **new** — rebase stored absolute image paths | 4 |
| `src-tauri/resources/migrations/0006_*.sql` | **new** — rebase stored export directory | 5 |
| `src-tauri/src/db/migrations.rs` | migration registry + tests | 4, 5 |
| `src-tauri/src/db/settings.rs` | single export-directory implementation | 3, 5 |
| `src-tauri/src/tts/supertonic/cache.rs`, `mod.rs` | drop duplicate export-dir impl, delegate | 5 |
| 4 React components, `index.html`, `build.rs` | user-visible strings | 5 |
| `README.md` | non-affiliation note | 5 |

---

### Task 1: Repoint the release guard

The release workflow is gated on the *old* repository name, so it silently skips on every tag today. Independent of the rename; ships first so the fix is not entangled with naming changes.

**Files:**
- Modify: `.github/workflows/release.yml:22`

**Interfaces:**
- Consumes: nothing
- Produces: nothing (workflow-only change)

- [ ] **Step 1: Confirm the guard is currently wrong**

```bash
grep -n "github.repository ==" .github/workflows/release.yml
```

Expected: `22:    if: github.repository == 'johnnyrobot/johnny-reader'` — a predicate that is now false.

- [ ] **Step 2: Repoint it**

Replace line 22 with:

```yaml
    if: github.repository == 'johnnylibretexts/libretexts-reader'
```

- [ ] **Step 3: Lint the workflow**

```bash
actionlint .github/workflows/release.yml
```

Expected: no output (success). If `actionlint` is not installed, skip with `brew install actionlint` or accept that CI runs it — the repo ships `.github/actionlint.yaml`.

- [ ] **Step 4: Verify no other stale repo references remain in CI**

```bash
grep -rn "johnnyrobot/johnny-reader" .github/
```

Expected: no output.

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci: repoint release guard at johnnylibretexts/libretexts-reader"
```

**Note on verification limits:** this cannot be fully verified without pushing a tag. Lint plus the grep is the available evidence; do not claim the release pipeline is proven working.

---

### Task 2: Rename the crate and package

**Files:**
- Modify: `src-tauri/Cargo.toml:2,12`
- Modify: `src-tauri/src/main.rs:5`
- Modify: `Cargo.toml:11`
- Modify: `package.json:2`
- Modify: `.github/workflows/ci.yml:50`
- Modify: `.github/workflows/release.yml:70`
- Modify: `README.md`, `RELEASE.md`, `HANDOFF.md`, `AGENTS.md`, `CLAUDE.md`
- Regenerated: `Cargo.lock`

**Interfaces:**
- Consumes: nothing
- Produces: crate `libretexts-reader`, lib `libretexts_reader_lib`, debug binary at `target/debug/libretexts-reader`. Tasks 3–5 use `cargo test -p libretexts-reader`.

- [ ] **Step 1: Rename the package and lib in the crate manifest**

In `src-tauri/Cargo.toml`:

```toml
[package]
name = "libretexts-reader"
```

```toml
[lib]
name = "libretexts_reader_lib"
```

- [ ] **Step 2: Update the binary entry point**

`src-tauri/src/main.rs:5`:

```rust
    libretexts_reader_lib::run()
```

- [ ] **Step 3: Update the workspace repository URL**

`Cargo.toml:11`:

```toml
repository = "https://github.com/johnnylibretexts/libretexts-reader"
```

- [ ] **Step 4: Update the npm package name**

`package.json:2`:

```json
  "name": "libretexts-reader",
```

- [ ] **Step 5: Run the Rust build to regenerate the lockfile and catch missed references**

```bash
cargo check -p libretexts-reader
```

Expected: PASS. A failure naming `johnny_reader_lib` means Step 2 was missed.

- [ ] **Step 6: Update both workflows**

`.github/workflows/ci.yml:50`:

```yaml
        run: cargo test -p libretexts-reader
```

`.github/workflows/release.yml:70`:

```yaml
          cargo build --release -p libretexts-reader
```

- [ ] **Step 7: Update the docs' build commands and binary paths**

Replace every `cargo check -p johnny-reader`, `cargo test -p johnny-reader`, and `target/debug/johnny-reader` in `README.md`, `RELEASE.md`, `HANDOFF.md`, `AGENTS.md`, `CLAUDE.md` with the `libretexts-reader` forms. The binary path changes with the crate name; docs pointing at `target/debug/johnny-reader` would reference a file that no longer exists.

- [ ] **Step 8: Verify no crate references remain**

```bash
grep -rn "johnny-reader\|johnny_reader" --include="*.toml" --include="*.rs" --include="*.json" --include="*.yml" --include="*.md" . \
  | grep -v node_modules | grep -v "^./target" | grep -v package-lock.json | grep -v docs/superpowers
```

Expected: no output. (`docs/superpowers/` is excluded — the spec and this plan quote the old names deliberately.)

- [ ] **Step 9: Run the full gate**

```bash
npm run build && npm test && cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test -p libretexts-reader && git diff --check
```

Expected: all PASS.

- [ ] **Step 10: Commit**

```bash
git add -A
git commit -m "chore: rename crate johnny-reader to libretexts-reader"
```

---

### Task 3: Rename the environment variable prefix

All 10 variables across 15 sites, in **one** commit. Atomicity is load-bearing: `content/libretexts.rs:1193` *sets* `…_APP_DATA_DIR` in a test while `paths.rs:8` *reads* it. Renaming only one side leaves that test silently writing into the real app-data directory instead of its temp dir.

**Files:**
- Modify: `src-tauri/build.rs` (4 sites), `src-tauri/src/paths.rs:8`, `src-tauri/src/db/settings.rs:105`, `src-tauri/src/content/openstax.rs:108`, `src-tauri/src/content/libretexts.rs:198,807,1193`, `src-tauri/src/voices/models.rs:25`, `src-tauri/src/voices/manifest.rs:30`, `src-tauri/src/tts/supertonic/model.rs:101`, `src-tauri/src/tts/supertonic/engine.rs:177`
- Modify: `.github/workflows/release.yml:94`
- Modify: `AGENTS.md`, `CLAUDE.md`, `HANDOFF.md`, `RELEASE.md`

**Interfaces:**
- Consumes: crate name from Task 2
- Produces: env vars `LIBRETEXTS_READER_APP_DATA_DIR`, `LIBRETEXTS_READER_DOCUMENTS_DIR`, `LIBRETEXTS_READER_REQUIRE_UPDATER_KEY`, `LIBRETEXTS_READER_OPENSTAX_BASE_URL`, `LIBRETEXTS_READER_COMMONS_BASE_URL`, `LIBRETEXTS_READER_LIBRARY_BASE_URL`, `LIBRETEXTS_READER_MODEL_MANIFEST_PATH`, `LIBRETEXTS_READER_VOICE_MANIFEST_PATH`, `LIBRETEXTS_READER_SUPERTONIC_MODEL_MANIFEST_PATH`, `LIBRETEXTS_READER_SUPERTONIC_FAKE_AUDIO`

- [ ] **Step 1: Rename the eight straightforward vars**

Apply the simple prefix swap across all Rust sources and the workflow:

```bash
grep -rl "JOHNNY_READER_" --include="*.rs" --include="*.yml" . | grep -v "^./target" | \
  xargs sed -i '' 's/JOHNNY_READER_/LIBRETEXTS_READER_/g'
```

This produces two doubled names (`LIBRETEXTS_READER_LIBRETEXTS_COMMONS_BASE_URL`, `LIBRETEXTS_READER_LIBRETEXTS_LIBRARY_BASE_URL`) which Step 2 shortens.

- [ ] **Step 2: Shorten the two doubled names**

```bash
grep -rl "LIBRETEXTS_READER_LIBRETEXTS_" --include="*.rs" --include="*.yml" . | grep -v "^./target" | \
  xargs sed -i '' 's/LIBRETEXTS_READER_LIBRETEXTS_/LIBRETEXTS_READER_/g'
```

Result: `LIBRETEXTS_READER_COMMONS_BASE_URL` (`content/libretexts.rs:198`) and `LIBRETEXTS_READER_LIBRARY_BASE_URL` (`content/libretexts.rs:807`). The redundant source segment is dropped because the app name already carries it.

- [ ] **Step 3: Verify nothing is left behind and nothing doubled**

```bash
grep -rn "JOHNNY_READER_\|LIBRETEXTS_READER_LIBRETEXTS_" --include="*.rs" --include="*.yml" . | grep -v "^./target"
```

Expected: no output.

- [ ] **Step 4: Confirm all ten vars are present with the new prefix**

```bash
grep -rhoE "LIBRETEXTS_READER_[A-Z_]+" --include="*.rs" --include="*.yml" . | grep -v "^./target" | sort -u
```

Expected: exactly ten names, matching the Produces list above.

- [ ] **Step 5: Run the Rust tests — this is what proves the atomic rename worked**

```bash
cargo test -p libretexts-reader
```

Expected: PASS, 54 tests. A test that writes to your real app-data directory means Steps 1–2 missed the set/read pair in `content/libretexts.rs:1193` / `paths.rs:8`.

- [ ] **Step 6: Update the docs**

Replace `JOHNNY_READER_APP_DATA_DIR` with `LIBRETEXTS_READER_APP_DATA_DIR` in `AGENTS.md` and `CLAUDE.md` and `HANDOFF.md`, and `JOHNNY_READER_REQUIRE_UPDATER_KEY` with `LIBRETEXTS_READER_REQUIRE_UPDATER_KEY` in `RELEASE.md`.

- [ ] **Step 7: Run the full gate**

```bash
npm run build && npm test && cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test -p libretexts-reader && git diff --check
```

Expected: all PASS.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "chore: rename JOHNNY_READER_ env prefix to LIBRETEXTS_READER_"
```

---

### Task 4: Adopt the new bundle identifier

The riskiest tier. Three declarations name one directory and nothing in the codebase links them; Tauri resolves `$APPDATA` from the **identifier** while `paths.rs` hardcodes the same string independently. This task changes both together, adds a CI gate so they can never drift again, and rebases the absolute paths already stored in the database.

**Files:**
- Modify: `src-tauri/tauri.conf.json:5`
- Modify: `src-tauri/src/paths.rs:5`
- Create: `src-tauri/resources/migrations/0005_rebase_app_dir_paths.sql`
- Modify: `src-tauri/src/db/migrations.rs` (registry entry + tests)
- Create: `scripts/ci/check-identifier.sh`
- Create: `scripts/ci/check-identifier.test.sh`
- Modify: `.github/workflows/ci.yml`
- Modify: `AGENTS.md`, `CLAUDE.md` (app-data path references)

**Interfaces:**
- Consumes: crate name (Task 2), env var names (Task 3)
- Produces: migration named `0005_rebase_app_dir_paths` in `MIGRATIONS`; test helper `migration_sql(name: &str) -> &'static str` in `migrations.rs` tests, reused by Task 5

- [ ] **Step 1: Write the failing migration test**

Add to the `mod tests` block in `src-tauri/src/db/migrations.rs`, alongside the existing `migrated_conn()` helper:

```rust
    fn migration_sql(name: &str) -> &'static str {
        super::MIGRATIONS
            .iter()
            .find(|(n, _)| *n == name)
            .unwrap_or_else(|| panic!("migration {name} is registered"))
            .1
    }

    #[test]
    fn rebase_app_dir_paths_rewrites_the_old_identifier() {
        let conn = migrated_conn();
        conn.execute_batch(
            "INSERT INTO documents (id, title, source_type, source_metadata, imported_at, cover_image_path)
                 VALUES ('doc1', 'D', 'pasted', '{}', 'now',
                         '/Users/x/Library/Application Support/dev.johnnyrobot.reader/covers/c.png');
             INSERT INTO sections (id, document_id, ordinal, title)
                 VALUES ('sec1', 'doc1', 0, 'S');
             INSERT INTO section_images (id, section_id, ordinal, source_url, local_path)
                 VALUES ('img1', 'sec1', 0, 'https://e.test/i.png',
                         '/Users/x/Library/Application Support/dev.johnnyrobot.reader/images/i.png');",
        )
        .expect("seed rows carrying the old identifier");

        conn.execute_batch(migration_sql("0005_rebase_app_dir_paths"))
            .expect("re-apply the rebase migration");

        let cover: String = conn
            .query_row("SELECT cover_image_path FROM documents WHERE id = 'doc1'", [], |r| r.get(0))
            .expect("read cover path");
        let image: String = conn
            .query_row("SELECT local_path FROM section_images WHERE id = 'img1'", [], |r| r.get(0))
            .expect("read image path");

        assert!(cover.contains("dev.johnnylibretexts.reader"), "cover not rebased: {cover}");
        assert!(!cover.contains("dev.johnnyrobot.reader"), "old prefix survived: {cover}");
        assert!(image.contains("dev.johnnylibretexts.reader"), "image not rebased: {image}");
        assert!(!image.contains("dev.johnnyrobot.reader"), "old prefix survived: {image}");
    }

    #[test]
    fn rebase_app_dir_paths_is_idempotent_and_leaves_other_paths_alone() {
        let conn = migrated_conn();
        conn.execute_batch(
            "INSERT INTO documents (id, title, source_type, source_metadata, imported_at, cover_image_path)
                 VALUES ('doc2', 'D', 'pasted', '{}', 'now', '/somewhere/else/c.png');",
        )
        .expect("seed an unrelated path");

        conn.execute_batch(migration_sql("0005_rebase_app_dir_paths")).expect("first run");
        conn.execute_batch(migration_sql("0005_rebase_app_dir_paths")).expect("second run");

        let cover: String = conn
            .query_row("SELECT cover_image_path FROM documents WHERE id = 'doc2'", [], |r| r.get(0))
            .expect("read cover path");
        assert_eq!(cover, "/somewhere/else/c.png");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test -p libretexts-reader rebase_app_dir_paths
```

Expected: FAIL — panic `migration 0005_rebase_app_dir_paths is registered`, because neither the file nor the registry entry exists yet.

- [ ] **Step 3: Create the migration file**

`src-tauri/resources/migrations/0005_rebase_app_dir_paths.sql`:

```sql
-- The bundle identifier changed from dev.johnnyrobot.reader to
-- dev.johnnylibretexts.reader, which moves the app-data directory. Image and
-- cover paths are persisted absolute (see content/images.rs), so the stored
-- rows must be rebased or every figure silently fails to render.
-- Guarded by LIKE on the old prefix: idempotent forward, inert once rebased.

UPDATE section_images
   SET local_path = replace(local_path,
       'dev.johnnyrobot.reader', 'dev.johnnylibretexts.reader')
 WHERE local_path LIKE '%dev.johnnyrobot.reader%';

UPDATE documents
   SET cover_image_path = replace(cover_image_path,
       'dev.johnnyrobot.reader', 'dev.johnnylibretexts.reader')
 WHERE cover_image_path LIKE '%dev.johnnyrobot.reader%';
```

- [ ] **Step 4: Register the migration**

A `.sql` file alone is inert — `migrations.rs` embeds each one via `include_str!`. Append to `const MIGRATIONS` in `src-tauri/src/db/migrations.rs`, after the `0004` entry:

```rust
    (
        "0005_rebase_app_dir_paths",
        include_str!("../../resources/migrations/0005_rebase_app_dir_paths.sql"),
    ),
```

- [ ] **Step 5: Run the tests to verify they pass**

```bash
cargo test -p libretexts-reader rebase_app_dir_paths
```

Expected: PASS, 2 tests.

- [ ] **Step 6: Change the identifier and `APP_DIR_NAME` together**

`src-tauri/tauri.conf.json:5`:

```json
  "identifier": "dev.johnnylibretexts.reader",
```

`src-tauri/src/paths.rs:5`:

```rust
const APP_DIR_NAME: &str = "dev.johnnylibretexts.reader";
```

Leave `assetProtocol.scope` untouched — it is written as `$APPDATA/covers/**` and `$APPDATA/images/**`, which follow the identifier automatically.

- [ ] **Step 7: Write the consistency gate script**

Create `scripts/ci/check-identifier.sh`:

```bash
#!/usr/bin/env bash
# Verify the Tauri bundle identifier matches APP_DIR_NAME in paths.rs.
# Both independently name the same app-data directory, and Tauri resolves the
# assetProtocol "$APPDATA" scope from the identifier -- so if they drift, the
# build stays green and every cover and figure silently stops rendering.
# Usage: check-identifier.sh
# Env:   ROOT  repo root (default: git rev-parse --show-toplevel)
set -euo pipefail

ROOT="${ROOT:-$(git rev-parse --show-toplevel)}"

# Deliberately no Node/jq dependency: this runs in CI before the toolchain is
# set up, alongside the other cheap pre-toolchain checks.
identifier="$(sed -n 's/.*"identifier"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$ROOT/src-tauri/tauri.conf.json" | head -1)"
app_dir="$(sed -n 's/^const APP_DIR_NAME: &str = "\(.*\)";$/\1/p' "$ROOT/src-tauri/src/paths.rs")"

if [ -z "$identifier" ]; then
  echo "check-identifier: no 'identifier' in src-tauri/tauri.conf.json" >&2
  exit 1
fi

if [ -z "$app_dir" ]; then
  echo "check-identifier: no APP_DIR_NAME in src-tauri/src/paths.rs" >&2
  exit 1
fi

if [ "$identifier" != "$app_dir" ]; then
  echo "identifier mismatch: tauri.conf.json has '$identifier', paths.rs APP_DIR_NAME has '$app_dir'" >&2
  exit 1
fi

echo "identifier OK: tauri.conf.json and paths.rs both == $identifier"
```

Make it executable:

```bash
chmod +x scripts/ci/check-identifier.sh
```

- [ ] **Step 8: Write the gate's own tests**

Create `scripts/ci/check-identifier.test.sh`, mirroring `check-version.test.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
script="$here/check-identifier.sh"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/src-tauri/src"

write_conf() { printf '{ "identifier": "%s" }\n' "$1" > "$tmp/src-tauri/tauri.conf.json"; }
write_paths() { printf 'const APP_DIR_NAME: &str = "%s";\n' "$1" > "$tmp/src-tauri/src/paths.rs"; }

# Case 1: identifier and APP_DIR_NAME agree -> exit 0
write_conf dev.example.reader
write_paths dev.example.reader
ROOT="$tmp" bash "$script" >/dev/null
echo "PASS: matching identifier accepted"

# Case 2: they disagree -> non-zero
write_paths dev.other.reader
if ROOT="$tmp" bash "$script" >/dev/null 2>&1; then
  echo "FAIL: mismatch not detected" >&2; exit 1
fi
echo "PASS: mismatched identifier rejected"

# Case 3: identifier missing from tauri.conf.json -> non-zero
printf '{ "productName": "X" }\n' > "$tmp/src-tauri/tauri.conf.json"
write_paths dev.example.reader
if ROOT="$tmp" bash "$script" >/dev/null 2>&1; then
  echo "FAIL: missing identifier not detected" >&2; exit 1
fi
echo "PASS: missing identifier rejected"

# Case 4: APP_DIR_NAME missing from paths.rs -> non-zero
write_conf dev.example.reader
printf 'const SOMETHING_ELSE: &str = "x";\n' > "$tmp/src-tauri/src/paths.rs"
if ROOT="$tmp" bash "$script" >/dev/null 2>&1; then
  echo "FAIL: missing APP_DIR_NAME not detected" >&2; exit 1
fi
echo "PASS: missing APP_DIR_NAME rejected"

echo "ALL TESTS PASSED"
```

```bash
chmod +x scripts/ci/check-identifier.test.sh
```

- [ ] **Step 9: Run the gate and its tests**

```bash
scripts/ci/check-identifier.test.sh
scripts/ci/check-identifier.sh
```

Expected: `ALL TESTS PASSED`, then `identifier OK: … dev.johnnylibretexts.reader`.

- [ ] **Step 10: Wire the gate into CI**

In `.github/workflows/ci.yml`, add a step immediately after the existing "Error kind sync check" (which ends at line 26) and **before** the `actions/setup-node@v4` step:

```yaml
      - name: Identifier sync check
        run: |
          scripts/ci/check-identifier.test.sh
          scripts/ci/check-identifier.sh
```

This slot is why Step 7's script uses `sed` rather than `node` to read the JSON: the surrounding steps run before any toolchain is installed — that is the point of the "Cheap, needs no toolchain" comment above them. A Node-dependent script placed here would depend on whatever the runner image happens to ship.

It belongs in `ci.yml`, not `release.yml` where `check-version.sh` runs: identifier drift breaks image rendering on any commit, so it must fail every pull request.

- [ ] **Step 11: Update the app-data paths in the docs**

Replace `~/Library/Application Support/dev.johnnyrobot.reader` with `~/Library/Application Support/dev.johnnylibretexts.reader` in `AGENTS.md` and `CLAUDE.md`.

- [ ] **Step 12: Run the full gate**

```bash
npm run build && npm test && cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test -p libretexts-reader && git diff --check
```

Expected: all PASS.

- [ ] **Step 13: Commit**

```bash
git add -A
git commit -m "feat: adopt dev.johnnylibretexts.reader bundle identifier"
```

- [ ] **Step 14: Move the app data (one-off, local, app closed)**

Quit the app first, then:

```bash
mv ~/Library/"Application Support"/dev.johnnyrobot.reader \
   ~/Library/"Application Support"/dev.johnnylibretexts.reader
```

`mv` within a volume is atomic. If the old directory does not exist, there is nothing to move — skip.

- [ ] **Step 15: Manual verification the test suite cannot cover**

Build and launch the debug binary:

```bash
npm run tauri -- build --debug --no-bundle
open target/debug/libretexts-reader
```

Confirm, in order: the library lists your existing documents; cover images render in the library grid; opening a document with figures renders its images inline.

This step is not optional. The failure mode here — a valid path pointing at nothing, plus an asset-protocol scope mismatch — produces no error and no failing test. Rendering is the only evidence.

---

### Task 5: Adopt the product name

**Files:**
- Modify: `src-tauri/tauri.conf.json:3,15`
- Modify: `index.html:6`
- Modify: `src/components/Sidebar.tsx:67`, `src/components/AppShell.tsx:141`, `src/components/Settings/SettingsPanel.tsx:23`, `src/components/FirstRun/ModelDownload.tsx:134`
- Modify: `src-tauri/build.rs:597`
- Modify: `.github/workflows/release.yml:101,112`
- Modify: `src-tauri/src/db/settings.rs` (make export-dir helper crate-visible, rename)
- Modify: `src-tauri/src/tts/supertonic/cache.rs` (delete duplicate), `src-tauri/src/tts/supertonic/mod.rs:14` (repoint import)
- Create: `src-tauri/resources/migrations/0006_rebase_export_directory.sql`
- Modify: `src-tauri/src/db/migrations.rs` (registry entry + test)
- Modify: `README.md`

**Interfaces:**
- Consumes: `migration_sql(name: &str) -> &'static str` test helper from Task 4
- Produces: `pub(crate) fn default_export_directory() -> String` in `crate::db::settings` — the single implementation; `crate::tts::supertonic::cache::default_export_directory` is removed

- [ ] **Step 1: Write the failing migration test**

Add to `mod tests` in `src-tauri/src/db/migrations.rs`:

```rust
    #[test]
    fn rebase_export_directory_rewrites_the_old_product_name() {
        let conn = migrated_conn();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('export_directory', ?1)",
            rusqlite::params!["\"/Users/x/Documents/Johnny Reader\""],
        )
        .expect("seed the old export directory");

        conn.execute_batch(migration_sql("0006_rebase_export_directory"))
            .expect("re-apply the rebase migration");

        let value: String = conn
            .query_row("SELECT value FROM settings WHERE key = 'export_directory'", [], |r| r.get(0))
            .expect("read export directory");

        assert_eq!(value, "\"/Users/x/Documents/LibreTexts Reader\"");
    }

    #[test]
    fn rebase_export_directory_leaves_a_custom_path_alone() {
        let conn = migrated_conn();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('export_directory', ?1)",
            rusqlite::params!["\"/Users/x/Music/Exports\""],
        )
        .expect("seed a custom export directory");

        conn.execute_batch(migration_sql("0006_rebase_export_directory")).expect("run once");
        conn.execute_batch(migration_sql("0006_rebase_export_directory")).expect("run twice");

        let value: String = conn
            .query_row("SELECT value FROM settings WHERE key = 'export_directory'", [], |r| r.get(0))
            .expect("read export directory");

        assert_eq!(value, "\"/Users/x/Music/Exports\"");
    }
```

Note the escaped quotes: `settings.value` stores JSON, so a string setting is quoted on disk.

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test -p libretexts-reader rebase_export_directory
```

Expected: FAIL — panic `migration 0006_rebase_export_directory is registered`.

- [ ] **Step 3: Create the migration file**

`src-tauri/resources/migrations/0006_rebase_export_directory.sql`:

```sql
-- The product name changed from "Johnny Reader" to "LibreTexts Reader", which
-- changes the default export directory. The chosen directory is persisted in
-- settings (JSON-encoded), so an existing install would keep pointing at the
-- old path. Matches on "/Johnny Reader" so a custom path chosen by the user is
-- left untouched. Idempotent forward, inert once rebased.

UPDATE settings
   SET value = replace(value, '/Johnny Reader', '/LibreTexts Reader')
 WHERE key = 'export_directory'
   AND value LIKE '%/Johnny Reader%';
```

- [ ] **Step 4: Register the migration**

Append to `const MIGRATIONS` in `src-tauri/src/db/migrations.rs`, after the `0005` entry:

```rust
    (
        "0006_rebase_export_directory",
        include_str!("../../resources/migrations/0006_rebase_export_directory.sql"),
    ),
```

- [ ] **Step 5: Run the tests to verify they pass**

```bash
cargo test -p libretexts-reader rebase_export_directory
```

Expected: PASS, 2 tests.

- [ ] **Step 6: Collapse the duplicate export-directory implementation**

There are two implementations building `~/Documents/Johnny Reader`, and they already disagree — `db/settings.rs:99` honours the documents-dir override, `tts/supertonic/cache.rs:147` reads `HOME` directly and ignores it. Keep the override-respecting one.

In `src-tauri/src/db/settings.rs`, make the helper crate-visible and rename the folder:

```rust
pub(crate) fn default_export_directory() -> String {
    documents_dir()
        .join("LibreTexts Reader")
        .to_string_lossy()
        .to_string()
}
```

In `src-tauri/src/tts/supertonic/cache.rs`, **delete** the whole `pub(crate) fn default_export_directory()` function (lines 147-155).

In `src-tauri/src/tts/supertonic/mod.rs:14`, repoint the import:

```rust
use crate::db::settings::default_export_directory;
```

- [ ] **Step 7: Verify the dedupe compiles and only one implementation remains**

```bash
cargo test -p libretexts-reader
grep -rn "fn default_export_directory" --include="*.rs" src-tauri/
```

Expected: tests PASS; the grep returns exactly one line, in `src-tauri/src/db/settings.rs`.

- [ ] **Step 8: Rename the product in the Tauri config and page title**

`src-tauri/tauri.conf.json:3`:

```json
  "productName": "LibreTexts Reader",
```

`src-tauri/tauri.conf.json:15` (the window `title`):

```json
        "title": "LibreTexts Reader",
```

`index.html:6`:

```html
    <title>LibreTexts Reader</title>
```

- [ ] **Step 9: Rename the product in the UI strings**

`src/components/Sidebar.tsx:67`:

```tsx
          <p className="truncate text-sm font-semibold">LibreTexts Reader</p>
```

`src/components/AppShell.tsx:140-142` — the name is a text node inside a `<p>`, so change only line 141:

```tsx
              <p className="truncate text-sm text-neutral-500 dark:text-neutral-400">
                LibreTexts Reader
              </p>
```

`src/components/Settings/SettingsPanel.tsx:23`:

```tsx
const SAMPLE_TEXT = "LibreTexts Reader voice test.";
```

`src/components/FirstRun/ModelDownload.tsx:134`:

```tsx
              LibreTexts Reader stores the model locally for offline playback.
```

- [ ] **Step 10: Rename the product in the LGPL notice**

`src-tauri/build.rs:597` — the notice text names the application to the user; update `Johnny Reader` to `LibreTexts Reader` in that string.

- [ ] **Step 11: Update the release bundle paths**

`.github/workflows/release.yml:101`:

```yaml
          DMG="target/release/bundle/dmg/LibreTexts Reader_${{ steps.meta.outputs.version }}_aarch64.dmg"
```

`.github/workflows/release.yml:112`:

```yaml
          xcrun stapler staple "target/release/bundle/macos/LibreTexts Reader.app"
```

These paths are derived from `productName`; leaving them stale makes the release job fail at the notarization step.

- [ ] **Step 12: Add the non-affiliation note**

In `README.md`, near the top under the project description:

```markdown
> **Not affiliated with LibreTexts.** LibreTexts Reader is an independent open-source
> project. It is not affiliated with, endorsed by, or sponsored by LibreTexts or OpenStax.
```

`SettingsPanel.tsx` has **no About section** — do not go looking for one. The panel is a single `<section>` whose closing tag is at line 440, preceded by a run of conditional status paragraphs. Add the note as the last child, immediately before `</section>`:

```tsx
      <p className="text-xs text-neutral-500 dark:text-neutral-400">
        LibreTexts Reader is an independent open-source project. It is not affiliated
        with, endorsed by, or sponsored by LibreTexts or OpenStax.
      </p>
    </section>
```

The muted `text-xs` styling matches the surrounding status paragraphs; this is a disclosure, not a call to action.

- [ ] **Step 13: Verify no stale product names remain**

```bash
grep -rn "Johnny Reader" --include="*.rs" --include="*.ts" --include="*.tsx" --include="*.json" --include="*.html" --include="*.yml" . \
  | grep -v node_modules | grep -v "^./target" | grep -v docs/superpowers | grep -v resources/migrations
```

Expected: no output. (`resources/migrations/` is excluded — `0006` quotes the old name deliberately, as its match target.)

- [ ] **Step 14: Run the full gate**

```bash
npm run build && npm test && cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test -p libretexts-reader && git diff --check
```

Expected: all PASS.

- [ ] **Step 15: Commit**

```bash
git add -A
git commit -m "feat: adopt LibreTexts Reader product name"
```

- [ ] **Step 16: Move existing exports (one-off, local)**

The migration repoints the setting; it cannot move files. Existing exported audio stays where it was:

```bash
mv ~/Documents/"Johnny Reader" ~/Documents/"LibreTexts Reader"
```

If the old directory does not exist, skip.

- [ ] **Step 17: Manual verification**

```bash
npm run tauri -- build --debug --no-bundle
open target/debug/libretexts-reader
```

Confirm: the window title and sidebar read *LibreTexts Reader*; Settings shows the export directory as `~/Documents/LibreTexts Reader`; the About section carries the non-affiliation note.

---

## Rollback

Each task is independently revertible with `git revert`. Tasks 4 and 5 are the two with state outside the repo — reverse the corresponding `mv`.

Both migrations are one-way by construction: `replace()` guarded by `WHERE … LIKE '<old>'`, so once rows carry the new value the predicate stops matching and re-running is a no-op. A `git revert` therefore does **not** undo the data rewrite; the fix is a one-line `UPDATE` in the other direction. This is why each task rewrites only strings its own task introduces — a revert spanning tasks would leave data and code disagreeing.
