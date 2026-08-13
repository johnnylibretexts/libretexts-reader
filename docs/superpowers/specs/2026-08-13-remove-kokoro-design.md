# Remove Kokoro — design

**Date:** 2026-08-13
**Status:** approved, not yet implemented

Remove the Kokoro speech engine and everything built only for it: the webview adapter, the
model-download and voice-download subsystems, the `voices` table, four settings, three UI
surfaces, and ~417 MB of model files on disk. Supertonic becomes the only engine and stays
the default.

This is spec **A** of two. Spec B adds Fish Audio as a bring-your-own-API-key provider.
A runs first so Fish lands in a two-case engine registry instead of a three-case one, and
so a broken engine and its workarounds are gone before anything new is added. The reasons
Kokoro is being dropped are recorded in `HANDOFF.md` §"Why Kokoro is being dropped" and are
not re-argued here.

## Decisions

| Question | Decision |
|---|---|
| Voice gallery + voice-download subsystem | Delete entirely. It is 55 downloadable Kokoro `.bin` embeddings; Supertonic's ten voices are a static list already in Settings. |
| Existing installs | Migrate settings, drop the `voices` table, and reclaim the orphaned files from disk. |
| Disk reclaim mechanism | Idempotent best-effort sweep at setup. `NotFound` is success; every other error is logged and swallowed. |
| Provider picker UI | Hide both `<select>`s and the second Test button. Keep `tts_provider`, `SpeechEngineId`, and `createSpeechEngine` as the seam for spec B. |
| `SpeechEngineId` | Narrow to `"supertonic"`. Do not collapse the abstraction. |

## Out of scope

Anything Fish-related · any change to Supertonic synthesis behaviour · CSP changes ·
API-key storage · the auto-updater · the `voices` route being replaced by anything.

## The central hazard: ordering

Every individual deletion here is trivially safe. Three of them are order-dependent against
a live database and a directory-creating path resolver, and getting the order wrong produces
a failure that does not look like a deletion bug.

```
db/connection.rs:38   seed_voice_catalog()   runs on EVERY pool open
paths.rs:27           voices_dir()           create_dir_all on resolve
lib.rs:55             paths::voices_dir()?   called during setup
```

1. **Unwire `seed_voice_catalog()` before dropping the table.** It runs on every pool open.
   Drop `voices` while that call is live and the app fails to start — on every launch, for
   every user, with a SQL error nobody will connect to a UI deletion.

2. **Delete `voices_dir()` and its call site before writing the sweep.** `paths.rs` calls
   `create_dir_all` on everything it resolves, so merely *asking* for the path materialises
   it. Leave the resolver in and the sweep deletes a directory that setup recreates
   milliseconds later, forever, silently, with the sweep reporting success each time.

3. **`models_dir()` stays — it is shared.** Supertonic lives at `models/<version>/`
   (`tts/supertonic/model.rs:205`). The sweep must name `kokoro-fp32.onnx` and
   `kokoro-q8.onnx` explicitly. Deleting `models/` would destroy the surviving engine.

## Frontend

### Delete outright

| Path | Size | Note |
|---|---|---|
| `src/lib/kokoro.ts` | 220 lines | |
| `src/lib/speech/kokoroEngine.ts` | 42 lines | |
| `src/components/FirstRun/ModelDownload.tsx` | 231 lines | `FirstRun/` empties; remove the directory |
| `src/components/VoiceGallery/` | `Gallery.tsx`, `VoiceCard.tsx` | |
| `kokoro-js` dependency | `package.json:21` | |

Also remove the `<ModelDownload />` render at `src/App.tsx:32`.

### Narrow, do not delete

- `src/lib/speech/types.ts:1` — `SpeechEngineId` becomes `"supertonic"`. The doc comment at
  line 29 explains cancellation in terms of `kokoro-js`; reword for Supertonic alone.
- `src/lib/speech/index.ts` — drop the `kokoro` case (line 35), the import (line 3), the
  re-export (line 7), and `modelPrecision` from `SpeechEngineSettings` (line 21).
  `createSpeechEngine` keeps its `switch`.
- `src/lib/speech/fakeEngine.ts:25` — default id `"kokoro"` → `"supertonic"`.
- `src/stores/settings.ts` — remove `ModelPrecision` (12), `modelPrecision` (26, 51, 163),
  `modelDownloaded` (30, 55, 167), `markModelDownloaded` (41, 93-96), and `asModelPrecision`
  (200). Change `defaultVoiceId` seed `"af_heart"` → `"M1"` (48).
- `src/lib/tauri.ts` — remove `ModelPrecision` (4) and the five wrappers `listVoices` (141),
  `downloadVoice` (143), `deleteVoice` (145), `ensureModelDownloaded` (147), `getModelPath`
  (149).
- `src/types/domain.ts:16` — the `Voice` type loses its last consumer; delete it.

### Route removal

The `voices` route leaves `AppShell.tsx` at seven sites — `RouteId` union (36), route label
map (58), render (266), the `route.id !== "voices"` guard (271), `routeIcon` (324),
`routeSubtitle` (349), and `routeStatusRows` (382-385, which hardcodes `"55 voices"` and
`"fp32 and q8"`) — plus the sidebar entry at `Sidebar.tsx:30` and the now-unused `Mic2`
import.

### Player

`src/stores/player.ts`:

- `activeEngine()` (291-297) caches the engine on a key of
  `ttsProvider:modelPrecision:supertonicLanguage`. Drop `modelPrecision` from the key.
- Line 319 — `engine.id === "kokoro" ? "Kokoro" : "Supertonic"` becomes the constant
  `"Supertonic"`.
- Line 504 — the in-memory speech-cache key includes `precision`. Remove it. The cache is a
  `Map` that does not survive a reload, so there is nothing to invalidate.
- **Keep** the engine-switch voice reset at 300-303 (`previous.id !== engine.id` →
  `set({ voice: engine.defaultVoice })`) and its explanatory comment. It is unreachable with
  one engine and becomes reachable again in spec B.

### Settings panel

`src/components/Settings/SettingsPanel.tsx`:

- Remove the provider `<select>` (262) and the provider-name display at 270.
- Remove the "Test Kokoro" button (385-393). `testProvider` loses its `providerToTest`
  parameter, its label ternary (154), its `setProvider` draft mutation (156), and its
  `voice:` ternary (176) — it tests Supertonic with the drafted `voiceStyle`, always.
- Remove the `modelPrecision` subscription (37) and its use at 165.

`src/components/Reader/PlaybackControls.tsx:105` — remove the provider `<select>`.

## Backend

### Delete outright

- `src-tauri/src/voices/` — `mod.rs`, `manifest.rs`, `models.rs`
- `src-tauri/resources/voices-manifest.json` — 55 Kokoro voice entries
- `models/manifest.json` (repo root) — both entries are Kokoro-82M URLs
- `src-tauri/src/commands/voices.rs` — all five commands
- `paths::voices_dir()` (`paths.rs:27-29`)
- `db::models::Voice`

Unregister from `lib.rs`: `mod voices` (8), the five `generate_handler!` entries (37-41),
and `paths::voices_dir()?` (55). Unregister `pub mod voices` from `commands/mod.rs:7`.
Remove the `seed_voice_catalog` call at `db/connection.rs:38` **first** (see hazard §1).

The `voice-download-progress` Tauri event disappears with `download_voice`; its only
listener is in the deleted `Gallery.tsx`.

Two environment variables die: `LIBRETEXTS_READER_MODEL_MANIFEST_PATH` and
`LIBRETEXTS_READER_VOICE_MANIFEST_PATH`.

### Keep, but rewrite the comments

Four comments explain live code in terms of an engine that no longer exists. Each is
load-bearing documentation, so each needs a replacement rather than a deletion:

- `src-tauri/src/tts/mod.rs:3` — "Kokoro is deliberately absent" no longer explains anything.
- `src-tauri/src/commands/tts.rs:29` — cites the `tts_provider: "kokoro"` seed as the reason
  the command stopped reading provider settings. The reason it *stays* provider-agnostic is
  now spec B.
- `src-tauri/src/commands/supertonic_tts.rs:261` — "switches from Kokoro to Supertonic
  mid-session".
- `src-tauri/src/tts/supertonic/voice.rs:107` — the cross-engine fallback test.

**`playback_voice_style`'s fallback stays.** With one engine it is no longer about carrying
a voice id across engines; it is what keeps a stale stored `default_voice_id` playing instead
of failing. The test at 105-113 keeps its assertions and gets that rationale in place of the
Kokoro one.

## Data migration

Three layers. All three are required; any one alone leaves a broken install.

### 1. Settings — read-time, `db/settings.rs`

Extend `migrate_removed_tts_provider` (line 88) so `"kokoro"` joins `"gemini" | "fish"` in
mapping to `"supertonic"`. This runs inside `load_settings`, which already rewrites the row
when the migration fires.

Add the same treatment for `default_voice_id`: any value not among the ten Supertonic styles
rewrites to `"M1"`. This catches `"af_heart"` and the 54 other Kokoro ids a user may have
selected. Without it, a stored Kokoro id survives and is silently swapped by
`playback_voice_style` on every single sentence.

Remove from `default_settings()` (line 71): `model_precision`, `model_downloaded`, and
change the `tts_provider` seed from `"kokoro"` to `"supertonic"`.

### 2. Schema — `0007_drop_kokoro_voices`

Next free number is **0007**, confirmed against both `src-tauri/resources/migrations/`
(highest file `0006_rebase_export_directory.sql`) and the hand-maintained `MIGRATIONS` array
in `db/migrations.rs` (highest entry `0006`). Both must be updated; the array is the source
of truth and a collision there registers under the wrong name and applies out of order.

The migration drops the `voices` table (created in `0001_initial_schema.sql:71`) and deletes
the `model_precision`, `model_downloaded`, and any Kokoro-valued `default_voice_id` /
`tts_provider` rows from `settings`.

### 3. Disk — idempotent sweep at setup

In `lib.rs` setup, after directory creation:

- `models/kokoro-fp32.onnx` (325 MB)
- `models/kokoro-q8.onnx` (92 MB)
- the entire `voices/` directory

`NotFound` is success. Any other error — permissions, a file held open — is logged and
swallowed. **Reclaiming 417 MB must never prevent the app from launching.** No flag row
tracks completion: the sweep is three `stat` calls after the first run, and a flag would be
wrong if a sweep half-failed.

This is the only new code in the spec. Everything else is deletion.

## Documentation

ADR-0001 is a decision record, so it is superseded rather than edited: add a banner at its
top pointing to a new **ADR-0003 — Supertonic is the only bundled engine**, which states the
removal and summarises the two faults already written up at `HANDOFF.md:134-161` so the
reasoning outlives the handoff file.

ADR-0003 should record the non-obvious win: ADR-0001's open consequence — *"the eSpeak NG
licensing question is not fully escaped by staying in the webview — `kokoro-js` bundles it as
WASM via `phonemizer`, and that ships in the app today. Tracked separately."* — is **closed**
by this work. Removing `kokoro-js` removes a GPL-3.0-or-later WASM payload from an Apache-2.0
binary. That is the largest consequence of this change and it is not visible in the diff.

Also update:

- `CLAUDE.md` — line 12 (`kokoro.ts` in the `lib/` inventory), line 22 ("TTS is split across
  two engines"), line 62 (math normalization "across system/Kokoro/Supertonic paths").
- `README.md:34` — the Kokoro playback paragraph.
- `HANDOFF.md` — line 67, line 374, and line 375. Line 375 records a large-chunk bundle
  warning for `kokoro.web`; that warning disappears entirely, so the note is deleted rather
  than reworded. `docs/superpowers/plans/2026-07-17-ci-release-automation.md:49` makes the
  same claim about an expected build warning and needs the same treatment.

## Testing

### Existing tests to change

- `src/stores/player.test.ts:165-167` builds a two-engine cross-engine test with a fake
  `"kokoro"` engine. Rewrite against one engine plus the fake; the cross-engine voice-reset
  path it exercised is unreachable until spec B, so that assertion is dropped, not faked.
- `src/lib/errors.test.ts:24,40` use `"kokoro failed to load"` only as an arbitrary error
  string. Retarget the wording; no behaviour change.

### New Rust tests

- Migration `0007` applied twice is a no-op, written mutation-killing in the style
  established in `8dd2298`.
- `default_voice_id` rewrite: a stored `"af_heart"` loads as `"M1"`; a stored `"F3"` is left
  alone.
- `tts_provider` rewrite: `"kokoro"` loads as `"supertonic"`, alongside the existing
  `"gemini"` / `"fish"` cases.
- Sweep: succeeds when the files are absent, succeeds when present and removes them, and
  does not return an error when removal fails.

Every test that touches paths **must** set `LIBRETEXTS_READER_APP_DATA_DIR`. `paths.rs`
calls `create_dir_all` on every path it resolves, so a test without the override writes into
the real `~/Library/Application Support/dev.johnnylibretexts.reader` and is indistinguishable
from real usage on disk.

### Gate

```bash
npm run build
npm test
cargo test -p libretexts-reader
cargo clippy -p libretexts-reader -- -D warnings
cargo fmt --check
scripts/ci/check-identifier.sh
git diff --check
```

### Manual verification

The failure modes here are all "starts fine on a clean machine, breaks on a real one", so a
clean-install test proves nothing. Launch a build against a **pre-existing** app-data
directory that has the Kokoro models and a populated `voices` table, and confirm:

1. The app starts. (Catches hazard §1 — `seed_voice_catalog` against a dropped table.)
2. Supertonic playback works and reports a real voice, not silence.
3. `models/kokoro-*.onnx` and `voices/` are gone, and `models/<version>/` is intact.
4. Relaunch: `voices/` has **not** reappeared. (Catches hazard §2.)
5. Covers and figures still render — the asset-protocol coupling is untested end-to-end on
   this machine per `HANDOFF.md:182`, and this spec touches `paths.rs`.

## Sequencing

Five commits, each independently green, ordered so the database is never left inconsistent
with the code that reads it:

1. **`refactor:`** unwire `seed_voice_catalog` from `db/connection.rs:38` — its only call
   site. One line. On an existing install nothing changes, because the rows are already
   there; on a fresh install the gallery is empty, which commit 2 makes moot.
2. **`feat:`** frontend removal — the five deleted files, the route, the pickers, the
   narrowed types, the updated frontend tests.
3. **`feat:`** backend removal — `voices/`, `commands/voices.rs`, the manifests, the handler
   registrations, `paths::voices_dir()` and its `lib.rs:55` call site, the comment rewrites.
4. **`feat:`** data migration — settings read-time migration, `0007`, the disk sweep, and the
   new Rust tests.
5. **`docs:`** ADR-0001 superseded banner, ADR-0003, `CLAUDE.md`, `README.md`, `HANDOFF.md`,
   the CI-release plan note.

`voices_dir()` cannot move earlier than commit 3: `commands/voices.rs:316` calls it, so
deleting it sooner breaks the build. That is the only thing pinning these two together.

Commit 1 before 4 is the hazard-§1 ordering — the table is dropped only once nothing can
seed it. Commit 3 before 4 is the hazard-§2 ordering — the resolver is gone before the sweep
that would otherwise race it.
