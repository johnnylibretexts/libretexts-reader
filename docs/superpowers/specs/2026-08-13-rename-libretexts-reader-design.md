# Rename to LibreTexts Reader — design

**Date:** 2026-08-13
**Status:** approved, not yet implemented

Rename the application from *Johnny Reader* to *LibreTexts Reader* across its crate,
package, bundle identifier, environment variables, and user-visible strings — and move
the on-disk app data to match the new identifier without losing the existing library.

## Decisions

| Question | Decision |
|---|---|
| Existing installs | Only the author's own machines. No in-app migration UI; a one-off local move suffices. |
| Bundle identifier | `dev.johnnyrobot.reader` → `dev.johnnylibretexts.reader` |
| Product (display) name | `Johnny Reader` → `LibreTexts Reader` |
| Crate / package | `johnny-reader` → `libretexts-reader`; lib `johnny_reader_lib` → `libretexts_reader_lib` |
| Env var prefix | `JOHNNY_READER_` → `LIBRETEXTS_READER_` (all 10 vars) |
| Sequencing | Five risk-tiered commits, each independently green |

The display name carries an affiliation question: this app imports LibreTexts content but
is not a LibreTexts project. Mitigation is a prominent non-affiliation line in the README
and the About/Settings screen (tier 5). The identifier deliberately stays under the
author's own `dev.<handle>.*` namespace rather than `org.libretexts.*`.

## Out of scope

In-app migration UI · version bump · auto-updater (removed in `8964722`) · repository
rename (already done) · any refactor beyond the export-directory dedupe in tier 5.

## The central hazard

Three declarations name the same directory, and nothing in the codebase links them:

```
src-tauri/tauri.conf.json   "identifier"      ──┐
src-tauri/src/paths.rs      APP_DIR_NAME      ──┼── all name ONE directory
src-tauri/tauri.conf.json   assetProtocol     ──┘
                            "$APPDATA/covers/**", "$APPDATA/images/**"
```

Tauri resolves `$APPDATA` from the **identifier**; `paths.rs` hardcodes the same string
independently. Changing one without the other leaves the build green and every cover and
figure silently unrenderable — the symptom already documented as a gotcha in `CLAUDE.md`.

A second hazard compounds it: image paths are stored **absolute**. `content/images.rs:143`
builds `images_dir.join(...)` and line 152 persists `path.to_string_lossy()`, which the
frontend passes straight to `convertFileSrc` (`Reader.tsx:108`, `DocumentCard.tsx:43`).
Moving the directory therefore invalidates every stored row. A `mv` alone is not enough.

## Commit sequence

### Tier 1 — `ci: repoint release guard at johnnylibretexts`

`.github/workflows/release.yml:22` reads
`if: github.repository == 'johnnyrobot/johnny-reader'`. That predicate is **already false**
after the repository move, so the release job silently skips on every tag. Repoint it to
`johnnylibretexts/libretexts-reader`.

Independent of the rename; correct on its own merits and shipped first so the fix is not
entangled with naming changes.

**Verify:** `actionlint` (config at `.github/actionlint.yaml`). Full verification requires
a tag push, which is out of scope here — note the limitation rather than claiming it.

### Tier 2 — `chore: rename crate johnny-reader → libretexts-reader`

| File | Change |
|---|---|
| `src-tauri/Cargo.toml:2` | `name = "libretexts-reader"` |
| `src-tauri/Cargo.toml:12` | `[lib] name = "libretexts_reader_lib"` |
| `src-tauri/src/main.rs:5` | `libretexts_reader_lib::run()` |
| `Cargo.toml:11` | `repository = ".../johnnylibretexts/libretexts-reader"` |
| `package.json:2` | `"name": "libretexts-reader"` |
| `.github/workflows/ci.yml:50` | `cargo test -p libretexts-reader` |
| `.github/workflows/release.yml:70` | `cargo build --release -p libretexts-reader` |
| `Cargo.lock` | regenerated |
| `README.md`, `RELEASE.md`, `HANDOFF.md`, `AGENTS.md`, `CLAUDE.md` | `cargo` commands |

The debug binary path changes with the crate name — `target/debug/johnny-reader` →
`target/debug/libretexts-reader`. It appears in `AGENTS.md`, `CLAUDE.md`, `HANDOFF.md`
and must be updated in the same commit or the docs point at a file that no longer exists.

**Verify:** `cargo test -p libretexts-reader`, `npm run build`, `npm test`.

### Tier 3 — `chore: rename JOHNNY_READER_* → LIBRETEXTS_READER_*`

All 10 variables across 15 sites, in **one** commit:

| Variable | Sites |
|---|---|
| `REQUIRE_UPDATER_KEY` | `build.rs` ×4, `release.yml` ×1 |
| `APP_DATA_DIR` | `paths.rs:8`, `content/libretexts.rs:1193` |
| `DOCUMENTS_DIR` | `db/settings.rs:105` |
| `OPENSTAX_BASE_URL` | `content/openstax.rs:108` |
| `LIBRETEXTS_COMMONS_BASE_URL` | `content/libretexts.rs:198` |
| `LIBRETEXTS_LIBRARY_BASE_URL` | `content/libretexts.rs:807` |
| `MODEL_MANIFEST_PATH` | `voices/models.rs:25` |
| `VOICE_MANIFEST_PATH` | `voices/manifest.rs:30` |
| `SUPERTONIC_MODEL_MANIFEST_PATH` | `tts/supertonic/model.rs:101` |
| `SUPERTONIC_FAKE_AUDIO` | `tts/supertonic/engine.rs:177` |

Atomicity is load-bearing: `content/libretexts.rs:1193` **sets** `…_APP_DATA_DIR` in a test
while `paths.rs:8` **reads** it. Renaming only one side leaves the test silently writing to
the real app-data directory instead of its temp dir.

Note the resulting `LIBRETEXTS_READER_LIBRETEXTS_*` doubling on two vars. It is ugly but
correct — the second `LIBRETEXTS` names the content source, not the app. Left as-is
rather than inventing an inconsistent shortening.

**Verify:** `cargo test -p libretexts-reader`, then
`grep -rn "JOHNNY_READER_" --include="*.rs" --include="*.yml" .` returns nothing.

### Tier 4 — `feat: adopt dev.johnnylibretexts.reader identifier`

Repo changes:

- `src-tauri/tauri.conf.json:5` — `"identifier": "dev.johnnylibretexts.reader"`
- `src-tauri/src/paths.rs:5` — `APP_DIR_NAME = "dev.johnnylibretexts.reader"`
- new `src-tauri/resources/migrations/0005_rebase_app_dir_paths.sql`
- new `scripts/ci/check-identifier.sh` + `scripts/ci/check-identifier.test.sh`
- `.github/workflows/ci.yml` — add a step beside the existing "Error kind sync check"

`assetProtocol.scope` needs no edit: it is written in terms of `$APPDATA`, which follows
the identifier automatically. It only breaks if the identifier and `APP_DIR_NAME` disagree.

**Migration `0005_rebase_app_dir_paths.sql`:**

```sql
UPDATE section_images
   SET local_path = replace(local_path,
       'dev.johnnyrobot.reader', 'dev.johnnylibretexts.reader')
 WHERE local_path LIKE '%dev.johnnyrobot.reader%';

UPDATE documents
   SET cover_image_path = replace(cover_image_path,
       'dev.johnnyrobot.reader', 'dev.johnnylibretexts.reader')
 WHERE cover_image_path LIKE '%dev.johnnyrobot.reader%';
```

Chosen over a manual `sqlite3` command because it is idempotent, machine-independent, a
no-op on fresh databases, runs automatically, and matches the repo rule of adding a new
numbered migration rather than editing an applied one. `0004` is the current highest.

**One-off local move**, performed with the app closed:

```sh
mv ~/Library/"Application Support"/dev.johnnyrobot.reader \
   ~/Library/"Application Support"/dev.johnnylibretexts.reader
```

`mv` within a volume is atomic. The next app launch applies `0005` and rebases the rows.

**`check-identifier.sh`:** extracts `identifier` from `tauri.conf.json` and `APP_DIR_NAME`
from `paths.rs`, exits non-zero if they differ. Mirrors `check-version.sh` in shape,
including a `.test.sh` companion covering the match, mismatch, and missing-field cases.
This converts the silent coupling into a red build.

It belongs in **`ci.yml`**, not `release.yml` where `check-version.sh` runs. Version drift
only matters when cutting a release; identifier drift breaks image rendering on any commit,
so it must fail every pull request. The two scripts share a shape, not a home.

**Verify:** `cargo test`, `check-identifier.test.sh`, then a **manual** launch of the debug
binary confirming the library lists, covers render, and a section's figures load. The
automated suite cannot cover this: the failure mode is a valid path that points nowhere.

### Tier 5 — `feat: product name LibreTexts Reader`

| File | Change |
|---|---|
| `src-tauri/tauri.conf.json:3,15` | `productName` + window `title` |
| `index.html:6` | `<title>` |
| `src/components/Sidebar.tsx:67` | sidebar label |
| `src/components/AppShell.tsx:141` | shell heading |
| `src/components/Settings/SettingsPanel.tsx:23` | TTS sample text |
| `src/components/FirstRun/ModelDownload.tsx:134` | first-run copy |
| `src-tauri/build.rs:597` | LGPL notice text |
| `.github/workflows/release.yml:101,112` | DMG and `.app` bundle paths |

**Export-directory dedupe.** Two independent implementations build
`~/Documents/Johnny Reader`:

- `src-tauri/src/db/settings.rs:99` — honours `…_DOCUMENTS_DIR`
- `src-tauri/src/tts/supertonic/cache.rs:147` — reads `HOME` directly, ignoring the override

They already disagree under test. Collapse to one implementation (the settings.rs form,
which respects the override) and have `cache.rs` call it, then rename to
`LibreTexts Reader`. Renaming both copies separately would preserve a bug this rename is
already touching.

The default export directory is **persisted in the settings row**, so an existing install
keeps its old configured path after the rename. On the author's machine this is a one-line
manual update or a re-pick in Settings; it is not worth migration code for one user.

**Non-affiliation note** in `README.md` and the About/Settings screen: LibreTexts Reader is
an independent project, not affiliated with or endorsed by LibreTexts.

**Verify:** `npm run build`, `npm test`, `cargo test`, and
`grep -rn "Johnny Reader"` returns **nothing** — the `build.rs:597` LGPL notice is renamed
along with everything else, since it is user-facing text naming the application.

## Rollback

Each tier is independently revertible. Tier 4 is the only one with state outside the repo:
reverse the `mv` and `git revert` the tier. Migration `0005` is a no-op in that direction
because its `WHERE` clause no longer matches once the rows carry the new prefix.

## Testing summary

Every tier must pass the full CI gate locally, not just the tests — `ci.yml` enforces
formatting and lint, so a tier that skips them passes locally and fails the pull request:

```sh
npm run build
npm test
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test -p libretexts-reader
git diff --check
```

Additionally — tier 3: grep proves zero `JOHNNY_READER_` remain. Tier 4: `check-identifier`
tests, plus the manual render check above. Tier 5: grep proves zero stale product names.
