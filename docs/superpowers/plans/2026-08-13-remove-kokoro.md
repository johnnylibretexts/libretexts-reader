# Remove Kokoro Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the Kokoro speech engine and every subsystem built only for it, leaving Supertonic as the single bundled engine and reclaiming ~417 MB from existing installs.

**Architecture:** Six independently-green commits. UI surfaces come out first (they compile without any type change), then the engine layer narrows, then the Rust subsystem goes, then the data migration lands, then docs. The `SpeechEngine` seam survives narrowed to one member so the follow-on Fish Audio spec adds a case rather than rebuilding a registry.

**Tech Stack:** React 19 + TypeScript (strict) + Zustand + Vite 6 + vitest; Rust + Tauri 2 + rusqlite/r2d2.

**Spec:** `docs/superpowers/specs/2026-08-13-remove-kokoro-design.md`

## Global Constraints

- **`tsconfig.json` sets `noUnusedLocals: true` and `noUnusedParameters: true`.** Every deletion must also delete the imports, state variables, props and parameters it orphans, in the same commit, or `npm run build` fails. Each task below enumerates them; do not treat those as optional tidying.
- **Migration numbering:** the next free number is `0007`. `MIGRATIONS` in `src-tauri/src/db/migrations.rs` is hand-maintained and is the source of truth; the `src-tauri/resources/migrations/` directory listing is not. Update both. Never mutate an already-applied migration file.
- **Migration SQL must be idempotent.** The test helper `migration_sql(name)` in `db/migrations.rs` re-executes a migration's SQL against a database where `apply_migrations` has *already* run it. `DROP TABLE` without `IF EXISTS` fails that helper.
- **Settings values are stored as JSON.** A string setting's raw column value includes the quotes: `"kokoro"` is stored as the 8-character text `"kokoro"`. SQL comparisons must be written `value = '"kokoro"'`.
- **Tests that resolve app paths must not rely on `LIBRETEXTS_READER_APP_DATA_DIR`.** `paths.rs` calls `create_dir_all` on every path it resolves, so a test missing the override writes into the real `~/Library/Application Support/dev.johnnylibretexts.reader`. This plan avoids the hazard entirely by testing the sweep against an explicitly-passed directory rather than the env var — Rust tests run in parallel in one process and there is no `serial_test` dev-dependency to make env mutation safe.
- **`models/` is shared with Supertonic** (`tts/supertonic/model.rs:205` resolves `models/<version>/`). Never delete the `models` directory; only the two named `kokoro-*.onnx` files inside it.
- **Node 22.x.** If the shell starts on Node 24, run `source "$HOME/.nvm/nvm.sh" && nvm use 22.20.0` first.
- **Verification gate for every task:** `npm run build` && `npm test` && `cargo test -p libretexts-reader` && `cargo clippy -p libretexts-reader -- -D warnings` && `cargo fmt --check` && `git diff --check`. A task is not done until all six pass.

---

### Task 1: Unwire the voice catalog seeder

Isolates the ordering hazard into one reviewable line. `seed_voice_catalog` runs on **every** pool open; if it is still wired when Task 5 drops the `voices` table, the app fails to start on every launch with a SQL error that looks nothing like a UI deletion.

**Files:**
- Modify: `src-tauri/src/db/connection.rs:38`

**Interfaces:**
- Consumes: nothing.
- Produces: `init_pool(db_path: &Path) -> AppResult<DbPool>` no longer populates the `voices` table. `crate::voices::manifest::seed_voice_catalog` becomes uncalled but still compiles (the `voices` module carries `#![allow(dead_code)]` at `manifest.rs:1`).

- [ ] **Step 1: Delete the seeding call**

In `src-tauri/src/db/connection.rs`, the block currently reads:

```rust
    {
        let mut conn = pool.get()?;
        migrations::apply_migrations(&mut conn)?;
        crate::db::settings::seed_default_settings(&conn)?;
        crate::voices::manifest::seed_voice_catalog(&mut conn)?;
    }
```

Remove the last call so it reads:

```rust
    {
        let mut conn = pool.get()?;
        migrations::apply_migrations(&mut conn)?;
        crate::db::settings::seed_default_settings(&conn)?;
    }
```

`conn` is still declared `mut` because `apply_migrations` takes `&mut Connection`. Do not change it.

- [ ] **Step 2: Verify the crate still builds and tests pass**

Run: `cargo test -p libretexts-reader`
Expected: PASS. No test asserts on seeded voices — `commands/voices.rs` has no `mod tests`.

- [ ] **Step 3: Verify clippy is clean**

Run: `cargo clippy -p libretexts-reader -- -D warnings`
Expected: PASS. `seed_voice_catalog` is now uncalled, but `src-tauri/src/voices/manifest.rs:1` already carries `#![allow(dead_code)]`, so no dead-code warning fires.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/db/connection.rs
git commit -m "refactor: stop seeding the Kokoro voice catalog on pool open"
```

---

### Task 2: Remove the Kokoro UI surfaces

Everything a user can see or click. This task changes no types, so it compiles against the still-two-member `TtsProvider`.

**Files:**
- Delete: `src/components/VoiceGallery/Gallery.tsx`, `src/components/VoiceGallery/VoiceCard.tsx`, `src/components/FirstRun/ModelDownload.tsx`
- Modify: `src/App.tsx`, `src/components/AppShell.tsx`, `src/components/Sidebar.tsx`, `src/components/Reader/PlaybackControls.tsx`, `src/components/Settings/SettingsPanel.tsx`

**Interfaces:**
- Consumes: nothing from Task 1.
- Produces: `RouteId` loses the `"voices"` member. `SettingsPanel`'s `testProvider` becomes zero-argument (`testProvider(): Promise<void>`) and always tests Supertonic. `persistDraft` becomes zero-argument (`persistDraft(): Promise<void>`). No component reads `useSettingsStore(state => state.ttsProvider)` any more.

- [ ] **Step 1: Delete the three component files and the empty directories**

```bash
git rm src/components/VoiceGallery/Gallery.tsx src/components/VoiceGallery/VoiceCard.tsx
git rm src/components/FirstRun/ModelDownload.tsx
rmdir src/components/VoiceGallery src/components/FirstRun
```

- [ ] **Step 2: Unmount `ModelDownload` in `src/App.tsx`**

Delete the import at line 3 (`import { ModelDownload } from "./components/FirstRun/ModelDownload";`) and collapse the fragment. The return becomes:

```tsx
  return <AppShell />;
```

The `<>...</>` fragment wrapper is no longer needed with a single child.

- [ ] **Step 3: Remove the `voices` route from `src/components/AppShell.tsx`**

Seven edits in this file:

1. Line 7 — delete `Mic2,` from the `lucide-react` import list.
2. Line 23 — delete `import { VoiceGallery } from "./VoiceGallery/Gallery";`
3. Line 36 — delete `| "voices"` from the `RouteId` union.
4. Line 58 — delete `voices: { id: "voices", label: "Voices" },` from `ROUTES`.
5. Line 266 — delete `{route.id === "voices" ? <VoiceGallery /> : null}`.
6. Line 271 — delete the `route.id !== "voices" &&` line from the `StatusTable` guard, leaving:

```tsx
      {route.id !== "library" &&
      route.id !== "settings" &&
      route.id !== "reader" ? (
        <StatusTable rows={statusRows} />
      ) : null}
```

7. Delete all three `case "voices":` arms — in `routeIcon` (324-325), `routeSubtitle` (349-350), and `routeStatusRows` (382-387, the arm returning `{ area: "Catalog", state: "Seeded", detail: "55 voices" }` and `{ area: "Model", state: "Pinned", detail: "fp32 and q8" }`).

`RouteId` is consumed by exhaustive switches with `noFallthroughCasesInSwitch`, so leaving any `case "voices"` behind after narrowing the union is a compile error, not a warning.

- [ ] **Step 4: Remove the Voices entry from `src/components/Sidebar.tsx`**

Delete `Mic2,` from the `lucide-react` import (line 7) and delete the line `{ id: "voices", label: "Voices", icon: Mic2 },` from `primaryRoutes` (line 30).

- [ ] **Step 5: Remove the engine picker from `src/components/Reader/PlaybackControls.tsx`**

Delete the whole `<label>` block containing the Engine `<select>` (lines 95-107, from `<label className="ml-0 flex items-center gap-2 ...">` through its closing `</label>`).

That orphans four things, all of which must go in the same edit or `noUnusedLocals` fails the build:

- line 11: `import { type TtsProvider, useSettingsStore } from "../../stores/settings";` — delete the entire import; nothing else in this file uses either.
- lines 26-27: the `ttsProvider` and `setTtsProvider` selector calls.
- lines 28-33: the `changeProvider` function.

- [ ] **Step 6: Collapse the provider machinery in `src/components/Settings/SettingsPanel.tsx`**

This file assumed a choice existed. Removing the choice collapses eight things:

1. Line 262 — delete `<option value="kokoro">Kokoro</option>`, then delete the entire "Narration engine" `<label>` block (253-265) since a one-option select is a control with no effect.
2. Lines 267-278 — the engine-description `<div>` uses `provider === "supertonic" ? ... : ...`. Replace the whole `grid` wrapper (252-279) with the static description:

```tsx
          <div className="rounded-md border border-neutral-200 bg-stone-50 px-3 py-2 text-sm dark:border-neutral-800 dark:bg-neutral-950">
            <span className="font-medium">Supertonic</span>
            <span className="ml-2 text-neutral-500 dark:text-neutral-400">
              Local multilingual model
            </span>
          </div>
```

3. Line 281 — the `{provider === "supertonic" ? (` conditional wrapping the voice/language/model panel is now always true. Unwrap it: delete that line and the matching `) : null}` at line 364, leaving the inner `<div className="mt-5 rounded-md ...">` rendered unconditionally.
4. Lines 382-394 — delete the entire "Test Kokoro" `<button>`.
5. Lines 395-407 — the "Test Supertonic" button: change `onClick={() => void testProvider("supertonic")}` to `onClick={() => void testProvider()}` and `{testing === "supertonic" ? (` to `{testing ? (`.
6. Rewrite `testProvider` (153-188) as:

```tsx
  async function testProvider() {
    setTesting(true);
    setTestStatus("Loading Supertonic...");
    setTestError(null);

    try {
      const engine = createSpeechEngine({
        ttsProvider: "supertonic",
        modelPrecision,
        supertonicLanguage: language,
      });
      await engine.ensureReady(setTestStatus);

      setTestStatus("Generating Supertonic sample...");
      const blob = await engine.synthesize({
        text: SAMPLE_TEXT,
        voice: voiceStyle,
        speed: 1,
      });

      setTestStatus("Playing Supertonic sample...");
      await playBlob(blob);
      setTestStatus("Supertonic test complete.");
    } catch (error) {
      setTestError(displayError(error));
      setTestStatus(null);
    } finally {
      setTesting(false);
    }
  }
```

`modelPrecision` is still passed here. It is removed in Task 3, when `SpeechEngineSettings` stops requiring it — that keeps this task independently green.

7. Rewrite `persistDraft` (145-151) as zero-argument, dropping `ttsProvider` (the store defaults it from its own state):

```tsx
  async function persistDraft() {
    await saveTtsSettings({
      supertonicVoiceStyle: voiceStyle,
      supertonicLanguage: language,
    });
  }
```

8. Delete the now-orphaned state and subscriptions:

- line 30: `const ttsProvider = useSettingsStore((state) => state.ttsProvider);`
- lines 40-42: the `provider` / `setProvider` `useState`
- line 50: change `const [testing, setTesting] = useState<TtsProvider | null>(null);` to `const [testing, setTesting] = useState(false);`
- lines 63-67: the sync `useEffect` — delete the `setProvider(ttsProvider);` line and drop `ttsProvider` from the dependency array, leaving `[supertonicLanguage, supertonicVoiceStyle]`
- lines 208-210 in `downloadSupertonicModel`: delete `setProvider("supertonic");` and the two-line comment above it
- lines 18-21: delete `type TtsProvider,` from the settings import, keeping `useSettingsStore`
- line 384 and 397: both buttons use `disabled={Boolean(testing)}` — with a boolean `testing` this becomes `disabled={testing}`

- [ ] **Step 7: Run the frontend gate**

Run: `npm run build && npm test`
Expected: PASS. `tsc` is the real check here — it catches any orphan `noUnusedLocals` violation and any surviving `case "voices"`.

- [ ] **Step 8: Commit**

```bash
git add -A src/
git commit -m "feat: remove the Kokoro UI surfaces

Deletes the Voices gallery and its route, the first-run model download, both
TTS provider pickers and the Test Kokoro button. The provider pickers go
rather than becoming one-option selects: a visible control with no effect
reads as broken. tts_provider stays in the store and the database, and the
Fish Audio spec re-adds a picker when there is something to pick."
```

---

### Task 3: Narrow the speech-engine layer to Supertonic

**Files:**
- Delete: `src/lib/kokoro.ts`, `src/lib/speech/kokoroEngine.ts`
- Modify: `src/lib/speech/types.ts`, `src/lib/speech/index.ts`, `src/lib/speech/fakeEngine.ts`, `src/lib/tauri.ts`, `src/types/domain.ts`, `src/stores/settings.ts`, `src/stores/player.ts`, `src/components/Settings/SettingsPanel.tsx`, `package.json`
- Test: `src/stores/player.test.ts`, `src/lib/errors.test.ts`

**Interfaces:**
- Consumes: `testProvider()` from Task 2 (still passes `modelPrecision`; this task removes that argument).
- Produces: `type SpeechEngineId = "supertonic"`. `SpeechEngineSettings = { ttsProvider: SpeechEngineId; supertonicLanguage: SupertonicLanguage }` — **no `modelPrecision`**. `createFakeEngine(options?: { id?: SpeechEngineId; voices?: string[] })` defaults `id` to `"supertonic"`. `type TtsProvider = "supertonic"`. `SettingsState` loses `modelPrecision` and `modelDownloaded`; `SettingsStore` loses `markModelDownloaded`. `api` loses `listVoices`, `downloadVoice`, `deleteVoice`, `ensureModelDownloaded`, `getModelPath`. `Domain.Voice` no longer exists.

- [ ] **Step 1: Rewrite the two engine-selection tests first**

The existing test at `src/stores/player.test.ts:164-181` constructs `createFake({ id: "kokoro", ... })`, which stops typechecking the moment `SpeechEngineId` narrows. It tests something real, though — that `activeEngine()` rebuilds when its cache key changes. That behaviour survives via `supertonicLanguage`, which is still in the key.

Replace the whole `describe("engine selection", ...)` block (lines 152-182) with:

```ts
describe("engine selection", () => {
  it("builds the engine once and reuses it across sentences", async () => {
    const engine = await createFake();
    const { usePlayerStore, createSpeechEngine } = await loadPlayer([engine]);

    await usePlayerStore.getState().loadDocument("doc-1");
    await usePlayerStore.getState().play();
    await usePlayerStore.getState().skipForward();

    expect(createSpeechEngine).toHaveBeenCalledTimes(1);
  });

  it("rebuilds the engine when the Supertonic language changes", async () => {
    const english = await createFake({ voices: ["M1"] });
    const korean = await createFake({ voices: ["M1"] });
    const { usePlayerStore, createSpeechEngine } = await loadPlayer([
      english,
      korean,
    ]);
    const { useSettingsStore } = await import("./settings");

    await usePlayerStore.getState().loadDocument("doc-1");
    await usePlayerStore.getState().play();
    expect(createSpeechEngine).toHaveBeenCalledTimes(1);

    useSettingsStore.setState({ supertonicLanguage: "ko" });
    await usePlayerStore.getState().play();

    // The engine cache is keyed on language, so a language change must not
    // keep speaking through the engine built for the previous one.
    expect(createSpeechEngine).toHaveBeenCalledTimes(2);
    expect(korean.calls.length).toBeGreaterThan(0);
  });
});
```

The cross-engine voice-reset assertion is dropped rather than faked: with one engine the reset at `player.ts:300-303` is unreachable. The code stays (see Step 7) because the Fish Audio spec makes it reachable again.

- [ ] **Step 2: Run the tests to see the new one fail**

Run: `npx vitest run src/stores/player.test.ts`
Expected: FAIL on "rebuilds the engine when the Supertonic language changes" — as written today, `activeEngine()`'s key already contains `supertonicLanguage`, so this may pass immediately. **If it passes, that is the correct outcome**: the test is a regression guard for behaviour Task 3 must not break while stripping `modelPrecision` out of the same cache key. Record which it did and move on.

- [ ] **Step 3: Delete the Kokoro engine files and dependency**

```bash
git rm src/lib/kokoro.ts src/lib/speech/kokoroEngine.ts
npm uninstall kokoro-js
```

`npm uninstall` updates both `package.json` and `package-lock.json`. Do **not** hand-edit the lockfile.

- [ ] **Step 4: Narrow `src/lib/speech/types.ts`**

Line 1 becomes:

```ts
export type SpeechEngineId = "supertonic";
```

The interface doc comment at lines 26-32 explains cancellation in terms of an engine that no longer exists. Replace lines 27-31 with:

```
 * On cancellation: `signal` is honoured where it is cheap — work not yet
 * started is skipped, and a result that arrives after abort is discarded.
 * Synthesis already in flight cannot be aborted: the Rust command has no
 * cancellation channel, so the signal shortens nothing that has begun.
```

- [ ] **Step 5: Narrow `src/lib/speech/index.ts`**

Delete line 2 (`import type { ModelPrecision } ...`), line 3 (`import { createKokoroEngine } ...`) and line 7 (the `createKokoroEngine` re-export). Remove `modelPrecision` from `SpeechEngineSettings`, and drop the `kokoro` case. The file's tail becomes:

```ts
export interface SpeechEngineSettings {
  ttsProvider: SpeechEngineId;
  supertonicLanguage: SupertonicLanguage;
}

/**
 * The one place the app decides which engine speaks.
 *
 * Everything downstream holds a `SpeechEngine` and never sees the provider
 * string again. Adding an engine means adding a case here and nowhere else.
 */
export function createSpeechEngine(settings: SpeechEngineSettings): SpeechEngine {
  switch (settings.ttsProvider) {
    case "supertonic":
      return createSupertonicEngine({ language: settings.supertonicLanguage });
  }
}
```

Keep the `switch`. It is the seam the Fish Audio spec extends, and the doc comment above it is the reason this task does not collapse the abstraction.

- [ ] **Step 6: Retarget the fake engine's default id**

`src/lib/speech/fakeEngine.ts:25` becomes:

```ts
  const id = options.id ?? "supertonic";
```

- [ ] **Step 7: Strip `modelPrecision` from `src/stores/player.ts`**

Two edits, and one deliberate non-edit:

- Lines 292-297 — remove `settings.modelPrecision,` from the cache key array, leaving:

```ts
  const key = [settings.ttsProvider, settings.supertonicLanguage].join(":");
```

- Line 504 — remove `precision: settings.modelPrecision,` from the speech-cache key object. Check whether `settings` is still referenced elsewhere in that function; if not, remove its binding too (`noUnusedLocals`). The cache is a module-scope `Map` that does not survive a reload, so there is nothing to invalidate.
- Line 319 — `const label = engine.id === "kokoro" ? "Kokoro" : "Supertonic";` becomes:

```ts
  const label = "Supertonic";
```

- **Do not touch lines 298-304.** The `previous.id !== engine.id` voice reset and the comment above `activeEngine` are unreachable with one engine and stay on purpose.

- [ ] **Step 8: Strip the settings store**

In `src/stores/settings.ts`:

- line 12: delete `export type ModelPrecision = "fp32" | "q8";`
- line 14: `export type TtsProvider = "supertonic";`
- lines 26, 30: delete `modelPrecision` and `modelDownloaded` from `SettingsState`
- line 41: delete `markModelDownloaded` from `SettingsStore`
- lines 48, 51, 55, 56: in `DEFAULT_SETTINGS`, change `defaultVoiceId` to `"M1"`, delete `modelPrecision` and `modelDownloaded`, change `ttsProvider` to `"supertonic"`
- lines 93-99: delete the `markModelDownloaded` implementation
- lines 163, 167: delete the `modelPrecision:` and `modelDownloaded:` entries from `loadSettings`
- lines 200-202: delete `asModelPrecision`
- lines 204-216: `asTtsProvider` keeps its role as the single place stored provider values are interpreted. Add `"kokoro"` to the retired list and update the comment:

```ts
/**
 * The single place stored provider values are interpreted, including retired
 * ones. `system` was the Web Speech path, removed once every engine sat behind
 * SpeechEngine; `gemini` and `fish` predate Supertonic; `kokoro` was removed
 * once it proved it could not produce audio in a bundled build. All fall back
 * to the default rather than being written back — the next settings save
 * overwrites the stale row anyway.
 */
function asTtsProvider(value: unknown): TtsProvider | undefined {
  if (value === "gemini" || value === "fish" || value === "kokoro") {
    return "supertonic";
  }
  return value === "supertonic" ? value : undefined;
}
```

- [ ] **Step 9: Strip the invoke boundary**

In `src/lib/tauri.ts`: delete line 4 (`export type ModelPrecision = "fp32" | "q8";`) and the five wrappers at lines 141-150 (`listVoices`, `downloadVoice`, `deleteVoice`, `ensureModelDownloaded`, `getModelPath`).

**Keep `invokeWithBrowserFallback`.** `listVoices` was one of ten callers; the other nine (`list_documents`, `search_documents`, the three catalog listers, and the three settings commands) are untouched.

In `src/types/domain.ts`: delete the `Voice` type (line 16). `Domain.Voice` had exactly two consumers, both already gone — `tauri.ts` (this step) and `VoiceGallery/` (Task 2).

- [ ] **Step 10: Drop the last `modelPrecision` reference in the settings panel**

In `src/components/Settings/SettingsPanel.tsx`, delete line 37 (`const modelPrecision = useSettingsStore((state) => state.modelPrecision);`) and remove the `modelPrecision,` line from the `createSpeechEngine({...})` call inside `testProvider`.

- [ ] **Step 11: Retarget the incidental Kokoro strings in `src/lib/errors.test.ts`**

Lines 24 and 40 use `"kokoro failed to load"` purely as an arbitrary non-backend error message. Replace both occurrences with `"supertonic failed to load"` — three edits (the `it.each` row at 24, and both the argument and the expectation at 40). This is a wording change with no behaviour change.

Also update the doc comment in `src/lib/errors.ts:7`: `"Kokoro's in-webview failures"` becomes `"webview-side failures"`.

- [ ] **Step 12: Run the full gate**

Run: `npm run build && npm test`
Expected: PASS, 32 tests. Note in the commit if the count changed — Step 1 replaced one test with another, so it should still be 32.

- [ ] **Step 13: Commit**

```bash
git add -A src/ package.json package-lock.json
git commit -m "feat: narrow the speech-engine layer to Supertonic

Deletes src/lib/kokoro.ts, the Kokoro adapter and the kokoro-js dependency.
SpeechEngineId narrows to a single member and createSpeechEngine keeps its
switch: it is the seam the Fish Audio spec extends, so collapsing it now
would only mean rebuilding it next.

The cross-engine voice-reset in activeEngine() is deliberately kept. It is
unreachable with one engine and becomes reachable again when Fish lands.
Its test is dropped rather than faked, and replaced with one covering the
language cache key, which is reachable today."
```

---

### Task 4: Remove the Rust voices subsystem

**Files:**
- Delete: `src-tauri/src/voices/mod.rs`, `src-tauri/src/voices/manifest.rs`, `src-tauri/src/voices/models.rs`, `src-tauri/src/commands/voices.rs`, `src-tauri/resources/voices-manifest.json`, `models/manifest.json`
- Modify: `src-tauri/src/lib.rs`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/paths.rs`, `src-tauri/src/db/models.rs`, `src-tauri/src/tts/mod.rs`, `src-tauri/src/commands/tts.rs`, `src-tauri/src/commands/supertonic_tts.rs`, `src-tauri/src/tts/supertonic/voice.rs`

**Do not edit these two files**, even though they name things this task deletes: `docs/superpowers/specs/2026-08-13-rename-libretexts-reader-design.md:98-99` and `docs/superpowers/plans/2026-08-13-rename-libretexts-reader.md:228` list `LIBRETEXTS_READER_MODEL_MANIFEST_PATH` and `LIBRETEXTS_READER_VOICE_MANIFEST_PATH`. They are the completed record of a different change, and both variables did exist when it shipped. Same principle as ADR-0001 in Task 6: records get superseded, not rewritten. Those two documents are the only mentions of either variable outside `src-tauri/src/voices/`, so no live documentation needs updating when they die.

**Interfaces:**
- Consumes: the frontend no longer calls `list_voices`, `download_voice`, `delete_voice`, `ensure_model_downloaded` or `get_model_path` (Task 3, Step 9).
- Produces: `paths` module loses `voices_dir()`; the remaining resolvers are `app_data_dir`, `database_path`, `models_dir`, `covers_dir`, `images_dir`, `cache_dir`, `temp_dir`. `crate::voices` no longer exists. `db::models::Voice` no longer exists.

- [ ] **Step 1: Delete the modules and manifests**

```bash
git rm -r src-tauri/src/voices
git rm src-tauri/src/commands/voices.rs
git rm src-tauri/resources/voices-manifest.json
git rm models/manifest.json
```

`models/manifest.json` is the repo-root file whose two entries are both Kokoro-82M URLs; it is unrelated to the Supertonic model, which resolves its own files in `tts/supertonic/model.rs`.

- [ ] **Step 2: Unregister from `src-tauri/src/lib.rs`**

Three edits:

1. Line 8 — delete `mod voices;`
2. Lines 37-41 — delete all five `commands::voices::*` entries from `generate_handler!`
3. Line 55 — delete `paths::voices_dir()?;` from `setup`

Leave `paths::models_dir()?;` at line 54 alone — Supertonic needs it.

- [ ] **Step 3: Unregister from `src-tauri/src/commands/mod.rs`**

Line 7 — delete `pub mod voices;`

- [ ] **Step 4: Delete the path resolver**

In `src-tauri/src/paths.rs`, delete lines 27-29:

```rust
pub fn voices_dir() -> AppResult<PathBuf> {
    app_subdir("voices")
}
```

This must land before Task 5's sweep exists. `app_subdir` calls `create_dir_all`, so a surviving resolver would recreate the directory the sweep deletes, on every launch, with the sweep reporting success each time.

- [ ] **Step 5: Delete the `Voice` model**

In `src-tauri/src/db/models.rs`, delete lines 86-97 — the `Voice` struct together with its two derive attributes:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Voice {
    pub id: String,
    pub display_name: String,
    pub language: String,
    pub gender: String,
    pub is_bundled: bool,
    pub is_downloaded: bool,
    pub size_bytes: u64,
    pub preview_path: Option<String>,
}
```

Its only consumers were `commands/voices.rs` and `voices/manifest.rs`, both deleted in Step 1.

- [ ] **Step 6: Rewrite the four comments that explain live code via a dead engine**

Each of these documents something real; replace, do not just delete.

`src-tauri/src/tts/mod.rs:3` — currently `//! Kokoro is deliberately absent — it runs in the webview. See ADR-0001.` Replace with:

```rust
//! Supertonic is the only speech engine. See ADR-0003 for why Kokoro, which
//! ran in the webview, was removed.
```

`src-tauri/src/commands/tts.rs` — the doc comment on `synthesize_speech` ends with `It also meant the command failed by default, since settings seed `tts_provider` to `kokoro`.` Replace that final sentence with:

```rust
/// It also meant the command failed by default, because the seeded provider was
/// not the one this command serves. Keeping the decision in one place is what
/// lets a second provider be added without this command learning about it.
```

`src-tauri/src/commands/supertonic_tts.rs:261` — replace the phrase `a reader who switches from Kokoro to Supertonic mid-session` with `a reader who switches engines mid-session`.

`src-tauri/src/tts/supertonic/voice.rs:105-113` — the test `playback_falls_back_instead_of_failing` keeps every assertion, including the `"af_heart"` case. Only its comment changes:

```rust
    #[test]
    fn playback_falls_back_instead_of_failing() {
        // A stored voice id can predate the current engine — an install that
        // used Kokoro still has one of its ids in default_voice_id until the
        // settings migration rewrites it. Reading the chapter in another voice
        // beats silence.
        assert_eq!(
            playback_voice_style(Some("af_heart"), DEFAULT_VOICE_STYLE),
            "M1"
        );
```

- [ ] **Step 7: Run the Rust gate**

Run: `cargo test -p libretexts-reader && cargo clippy -p libretexts-reader -- -D warnings`
Expected: PASS, 60 tests. Clippy catches any import left behind by the deleted modules.

- [ ] **Step 8: Confirm no Kokoro references survive in Rust**

Run: `grep -rn -i "kokoro" src-tauri/src src-tauri/resources`
Expected: exactly two hits, both intentional — the ADR-0003 pointer in `tts/mod.rs` and the migration-rationale comment in `tts/supertonic/voice.rs`. Anything else is a miss.

- [ ] **Step 9: Commit**

```bash
git add -A src-tauri/ models/
git commit -m "feat: remove the Rust voices subsystem

The voice gallery's backing subsystem existed only for Kokoro: the manifest
is 55 .bin embeddings from the Kokoro-82M repo and models.rs matched literal
kokoro-*.onnx filenames. Supertonic's ten voices are a static list on the
frontend, so nothing here has a second consumer.

paths::voices_dir() goes with it. paths.rs calls create_dir_all on resolve,
so leaving the resolver would recreate the directory that the next commit's
sweep deletes, every launch, silently.

models_dir() stays — Supertonic lives at models/<version>/."
```

---

### Task 5: Migrate existing installs

The only new code in the whole plan. Three layers: settings values, schema, and disk.

**Files:**
- Create: `src-tauri/resources/migrations/0007_drop_kokoro_voices.sql`, `src-tauri/src/cleanup.rs`
- Modify: `src-tauri/src/db/migrations.rs`, `src-tauri/src/db/settings.rs`, `src-tauri/src/tts/supertonic/voice.rs`, `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `voices` table is no longer seeded (Task 1) and no longer read (Task 4).
- Produces: `voice::is_valid_supertonic_voice_style(voice_style: &str) -> bool`. `cleanup::reclaim_kokoro_artifacts()` — takes nothing, returns nothing, never panics. `cleanup::reclaim_in(app_data_dir: &Path)` — the testable inner function.

- [ ] **Step 1: Write the failing migration tests**

Append to the `mod tests` block in `src-tauri/src/db/migrations.rs`:

```rust
    #[test]
    fn drop_kokoro_voices_removes_the_voices_table() {
        let conn = migrated_conn();
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'voices')",
                [],
                |row| row.get(0),
            )
            .expect("query sqlite_master");
        assert!(!exists, "the voices table must not survive migration 0007");
    }

    #[test]
    fn drop_kokoro_voices_rewrites_a_stored_kokoro_provider() {
        let conn = migrated_conn();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('tts_provider', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params!["\"kokoro\""],
        )
        .expect("seed a stored kokoro provider");

        conn.execute_batch(migration_sql("0007_drop_kokoro_voices"))
            .expect("re-apply the migration");

        let value: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'tts_provider'",
                [],
                |r| r.get(0),
            )
            .expect("read tts_provider");
        assert_eq!(value, "\"supertonic\"");
    }

    #[test]
    fn drop_kokoro_voices_rewrites_a_stored_kokoro_voice_id() {
        let conn = migrated_conn();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('default_voice_id', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params!["\"af_heart\""],
        )
        .expect("seed a stored kokoro voice id");

        conn.execute_batch(migration_sql("0007_drop_kokoro_voices"))
            .expect("re-apply the migration");

        let value: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'default_voice_id'",
                [],
                |r| r.get(0),
            )
            .expect("read default_voice_id");
        assert_eq!(
            value, "\"M1\"",
            "a Kokoro voice id would otherwise be swapped for M1 on every sentence forever"
        );
    }

    #[test]
    fn drop_kokoro_voices_leaves_a_supertonic_voice_id_alone() {
        let conn = migrated_conn();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('default_voice_id', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params!["\"F3\""],
        )
        .expect("seed a chosen Supertonic voice");

        conn.execute_batch(migration_sql("0007_drop_kokoro_voices"))
            .expect("run once");
        conn.execute_batch(migration_sql("0007_drop_kokoro_voices"))
            .expect("run twice");

        let value: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'default_voice_id'",
                [],
                |r| r.get(0),
            )
            .expect("read default_voice_id");
        assert_eq!(
            value, "\"F3\"",
            "the migration must not flatten a voice the reader deliberately chose"
        );
    }

    #[test]
    fn drop_kokoro_voices_drops_the_dead_model_settings() {
        let conn = migrated_conn();
        conn.execute_batch(
            "INSERT INTO settings (key, value) VALUES ('model_precision', '\"q8\"')
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value;
             INSERT INTO settings (key, value) VALUES ('model_downloaded', 'true')
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value;",
        )
        .expect("seed the dead model settings");

        conn.execute_batch(migration_sql("0007_drop_kokoro_voices"))
            .expect("run once");
        conn.execute_batch(migration_sql("0007_drop_kokoro_voices"))
            .expect("run twice");

        let remaining: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM settings
                     WHERE key IN ('model_precision', 'model_downloaded')",
                [],
                |r| r.get(0),
            )
            .expect("count dead settings");
        assert_eq!(remaining, 0);
    }
```

These are mutation-killing rather than trivially-true: two of them seed a value, assert it *changed*, and run the migration twice — a no-op migration fails the first assertion, and a non-idempotent one fails the second.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p libretexts-reader drop_kokoro_voices`
Expected: FAIL — `migration_sql` panics with `migration 0007_drop_kokoro_voices is registered`, because nothing is registered yet.

- [ ] **Step 3: Write the migration SQL**

Create `src-tauri/resources/migrations/0007_drop_kokoro_voices.sql`:

```sql
-- Kokoro is removed. The voices table held only Kokoro voice embeddings
-- (55 .bin files from the Kokoro-82M ONNX repo); Supertonic's ten voice
-- styles are a static list and were never stored here.
DROP TABLE IF EXISTS voices;

-- model_precision and model_downloaded described the Kokoro ONNX file.
DELETE FROM settings WHERE key IN ('model_precision', 'model_downloaded');

-- Settings values are JSON, so a string's stored text includes its quotes.
UPDATE settings
   SET value = '"supertonic"'
 WHERE key = 'tts_provider'
   AND value = '"kokoro"';

-- A stored Kokoro voice id would otherwise be silently swapped for M1 by
-- playback_voice_style on every sentence: working audio, permanently wrong
-- setting, no error. Anything that is not one of Supertonic's ten styles is
-- rewritten to the default.
UPDATE settings
   SET value = '"M1"'
 WHERE key = 'default_voice_id'
   AND value NOT IN ('"M1"', '"M2"', '"M3"', '"M4"', '"M5"',
                     '"F1"', '"F2"', '"F3"', '"F4"', '"F5"');
```

Every statement is idempotent: `DROP TABLE IF EXISTS` and `DELETE` are naturally so, and both `UPDATE`s stop matching once they have run (`"supertonic"` is not `"kokoro"`; `"M1"` is in the allowed list).

- [ ] **Step 4: Register the migration**

In `src-tauri/src/db/migrations.rs`, append to the `MIGRATIONS` array after the `0006` entry:

```rust
    (
        "0007_drop_kokoro_voices",
        include_str!("../../resources/migrations/0007_drop_kokoro_voices.sql"),
    ),
```

The array is the source of truth. A name that does not match its file registers under the wrong key and applies out of order.

- [ ] **Step 5: Run the migration tests to verify they pass**

Run: `cargo test -p libretexts-reader drop_kokoro_voices`
Expected: PASS, 5 tests.

- [ ] **Step 6: Write the failing settings-migration tests**

The SQL migration runs once. The read-time migration is the guard for a value that reaches the settings table any other way, and it is where `tts_provider`'s retired values already live. Append to `src-tauri/src/db/settings.rs` a `mod tests` block (create it if the file has none):

```rust
#[cfg(test)]
mod tests {
    use super::{get_all_settings, get_setting, set_setting};
    use rusqlite::Connection;
    use serde_json::json;

    fn settings_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(
            "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
        )
        .expect("create settings table");
        conn
    }

    fn raw_value(conn: &Connection, key: &str) -> String {
        conn.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            rusqlite::params![key],
            |row| row.get(0),
        )
        .expect("read raw value")
    }

    #[test]
    fn reading_a_stored_kokoro_provider_migrates_and_persists_it() {
        let conn = settings_conn();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('tts_provider', ?1)",
            rusqlite::params!["\"kokoro\""],
        )
        .expect("seed kokoro");

        let value = get_setting(&conn, "tts_provider").expect("read").unwrap();

        assert_eq!(value, json!("supertonic"));
        assert_eq!(
            raw_value(&conn, "tts_provider"),
            "\"supertonic\"",
            "the migrated value must be written back, not just returned"
        );
    }

    #[test]
    fn writing_a_kokoro_provider_stores_supertonic_instead() {
        let conn = settings_conn();
        set_setting(&conn, "tts_provider", &json!("kokoro")).expect("write");
        assert_eq!(raw_value(&conn, "tts_provider"), "\"supertonic\"");
    }

    #[test]
    fn reading_a_stored_kokoro_voice_id_migrates_it() {
        let conn = settings_conn();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('default_voice_id', ?1)",
            rusqlite::params!["\"af_heart\""],
        )
        .expect("seed a kokoro voice id");

        let all = get_all_settings(&conn).expect("read all");

        assert_eq!(all.get("default_voice_id"), Some(&json!("M1")));
        assert_eq!(raw_value(&conn, "default_voice_id"), "\"M1\"");
    }

    #[test]
    fn a_chosen_supertonic_voice_id_is_left_alone() {
        let conn = settings_conn();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('default_voice_id', ?1)",
            rusqlite::params!["\"F3\""],
        )
        .expect("seed a chosen voice");

        let value = get_setting(&conn, "default_voice_id")
            .expect("read")
            .unwrap();

        assert_eq!(value, json!("F3"));
    }
}
```

- [ ] **Step 7: Run them to verify they fail**

Run: `cargo test -p libretexts-reader --lib db::settings`
Expected: FAIL — the kokoro-provider cases fail because `migrate_removed_tts_provider` only knows `gemini` and `fish`; the voice-id cases fail because no `default_voice_id` migration exists.

- [ ] **Step 8: Expose a voice-style validator**

In `src-tauri/src/tts/supertonic/voice.rs`, add alongside `is_valid_supertonic_language` (after line 24):

```rust
pub(crate) fn is_valid_supertonic_voice_style(voice_style: &str) -> bool {
    SUPERTONIC_VOICE_STYLES
        .iter()
        .any(|style| style.eq_ignore_ascii_case(voice_style))
}
```

`SUPERTONIC_VOICE_STYLES` is currently private to the module; this function is what crosses the boundary, so the constant stays private.

- [ ] **Step 9: Implement the settings migrations**

In `src-tauri/src/db/settings.rs`:

Add the import at the top:

```rust
use crate::tts::supertonic::voice::is_valid_supertonic_voice_style;
```

That path is correct as written: `tts/mod.rs` declares `pub mod supertonic;` and `tts/supertonic/mod.rs` declares `pub mod voice;`.

Replace `migrate_removed_tts_provider` (lines 87-95) with a pair of functions and one dispatcher:

```rust
/// Rewrite a stored value that names something the app no longer has.
///
/// Returns true when the value changed, which is the caller's signal to write
/// it back. Retired providers: `system` was the Web Speech path; `gemini` and
/// `fish` predate Supertonic; `kokoro` was removed once it proved it could not
/// produce audio in a bundled build.
fn migrate_removed_setting(key: &str, value: &mut Value) -> bool {
    match key {
        "tts_provider" => migrate_removed_tts_provider(value),
        "default_voice_id" => migrate_removed_voice_id(value),
        _ => false,
    }
}

fn migrate_removed_tts_provider(value: &mut Value) -> bool {
    match value.as_str() {
        Some("gemini" | "fish" | "kokoro") => {
            *value = json!("supertonic");
            true
        }
        _ => false,
    }
}

/// A voice id belonging to a removed engine is not merely stale: the Supertonic
/// adapter falls back rather than failing, so it would be silently swapped for
/// the default on every sentence, forever, with nothing surfaced to the reader.
fn migrate_removed_voice_id(value: &mut Value) -> bool {
    match value.as_str() {
        Some(voice_style) if !is_valid_supertonic_voice_style(voice_style) => {
            *value = json!("M1");
            true
        }
        _ => false,
    }
}
```

Then change the three call sites to use the dispatcher:

- `get_setting` (line 27): `if migrate_removed_setting(key, &mut value) {`
- `set_setting` (lines 39-41): replace the `if key == "tts_provider"` block with `migrate_removed_setting(key, &mut value);`
- `get_all_settings` (line 62): `if migrate_removed_setting(&key, &mut value) {`

Finally update `default_settings` (lines 72-84) to:

```rust
    Ok(vec![
        ("default_voice_id", json!("M1")),
        ("default_speed", json!(1.0)),
        ("export_directory", json!(default_export_directory())),
        ("theme", json!("system")),
        ("telemetry_opt_in", json!(false)),
        ("auto_check_updates", json!(true)),
        ("tts_provider", json!("supertonic")),
        ("supertonic_voice_style", json!("M1")),
        ("supertonic_language", json!("en")),
    ])
```

- [ ] **Step 10: Run the settings tests to verify they pass**

Run: `cargo test -p libretexts-reader --lib db::settings`
Expected: PASS, 4 tests.

- [ ] **Step 11: Write the failing sweep tests**

Create `src-tauri/src/cleanup.rs` containing only its test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::reclaim_in;
    use std::fs;
    use uuid::Uuid;

    /// A throwaway directory passed explicitly, never via
    /// LIBRETEXTS_READER_APP_DATA_DIR. Rust tests share one process and there
    /// is no serial_test dev-dependency, so mutating that env var here would
    /// let one test redirect another's app data mid-run.
    fn scratch_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("libretexts-reader-sweep-{}", Uuid::new_v4()));
        fs::create_dir_all(dir.join("models")).expect("create models dir");
        fs::create_dir_all(dir.join("voices")).expect("create voices dir");
        dir
    }

    #[test]
    fn reclaims_the_kokoro_models_and_the_voices_directory() {
        let dir = scratch_dir();
        fs::write(dir.join("models/kokoro-fp32.onnx"), b"x").expect("write fp32");
        fs::write(dir.join("models/kokoro-q8.onnx"), b"x").expect("write q8");
        fs::write(dir.join("voices/af_heart.bin"), b"x").expect("write voice");

        reclaim_in(&dir);

        assert!(!dir.join("models/kokoro-fp32.onnx").exists());
        assert!(!dir.join("models/kokoro-q8.onnx").exists());
        assert!(!dir.join("voices").exists());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn leaves_the_supertonic_model_directory_untouched() {
        let dir = scratch_dir();
        fs::create_dir_all(dir.join("models/supertonic-v1")).expect("create model dir");
        fs::write(dir.join("models/supertonic-v1/model.onnx"), b"x").expect("write model");

        reclaim_in(&dir);

        assert!(
            dir.join("models/supertonic-v1/model.onnx").exists(),
            "models/ is shared — deleting the directory would destroy the surviving engine"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_clean_install_is_not_an_error() {
        let dir = scratch_dir();
        fs::remove_dir_all(dir.join("voices")).expect("remove voices dir");

        // Nothing to reclaim. This must not panic and must not recreate
        // anything: the whole point is that launch never depends on it.
        reclaim_in(&dir);

        assert!(!dir.join("voices").exists());

        fs::remove_dir_all(&dir).ok();
    }
}
```

- [ ] **Step 12: Run them to verify they fail**

Register the module first — add `mod cleanup;` to `src-tauri/src/lib.rs` after `mod paths;`.

Run: `cargo test -p libretexts-reader cleanup`
Expected: FAIL to compile — `cannot find function reclaim_in in this scope`.

- [ ] **Step 13: Implement the sweep**

Prepend to `src-tauri/src/cleanup.rs`, above the test module:

```rust
//! One-time reclamation of files left behind by the removed Kokoro engine.
//!
//! Best-effort by design. An existing install holds ~417 MB of dead ONNX
//! models and a directory of voice embeddings; getting that space back is
//! worth doing and is never worth failing a launch over, so every error here
//! is reported and swallowed.

use std::path::Path;

use crate::paths;

const KOKORO_MODEL_FILES: &[&str] = &["kokoro-fp32.onnx", "kokoro-q8.onnx"];

pub fn reclaim_kokoro_artifacts() {
    match paths::app_data_dir() {
        Ok(dir) => reclaim_in(&dir),
        Err(error) => eprintln!("kokoro cleanup: could not resolve app data dir: {error}"),
    }
}

/// Split out from `reclaim_kokoro_artifacts` so tests can pass a throwaway
/// directory instead of mutating the app-data environment variable.
fn reclaim_in(app_data_dir: &Path) {
    // Only the two named files. models/ also holds the Supertonic model.
    let models_dir = app_data_dir.join("models");
    for file_name in KOKORO_MODEL_FILES {
        let path = models_dir.join(file_name);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => eprintln!("kokoro cleanup: could not remove {}: {error}", path.display()),
        }
    }

    let voices_dir = app_data_dir.join("voices");
    match std::fs::remove_dir_all(&voices_dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => eprintln!(
            "kokoro cleanup: could not remove {}: {error}",
            voices_dir.display()
        ),
    }
}
```

`eprintln!` rather than `tracing::warn!`: `tracing` is a declared dependency with no subscriber installed anywhere in `src`, so a warning would go nowhere. `eprintln!` matches the one existing precedent at `content/libretexts.rs:1200`.

- [ ] **Step 14: Run the sweep tests to verify they pass**

Run: `cargo test -p libretexts-reader cleanup`
Expected: PASS, 3 tests.

- [ ] **Step 15: Call the sweep from setup**

In `src-tauri/src/lib.rs`, add the call inside `.setup(...)` after the directory resolvers and before `init_pool`:

```rust
        .setup(|app| {
            let db_path = paths::database_path()?;
            paths::models_dir()?;
            paths::covers_dir()?;
            paths::images_dir()?;
            paths::cache_dir()?;
            paths::temp_dir()?;
            cleanup::reclaim_kokoro_artifacts();
            let pool = init_pool(&db_path)?;
            app.manage(pool);
            Ok(())
        })
```

Note it returns nothing and is not `?`-propagated — that is the point.

- [ ] **Step 16: Run the full Rust gate**

Run: `cargo test -p libretexts-reader && cargo clippy -p libretexts-reader -- -D warnings && cargo fmt --check`
Expected: PASS. Test count should be 60 + 5 (migrations) + 4 (settings) + 3 (cleanup) = 72.

- [ ] **Step 17: Commit**

```bash
git add -A src-tauri/
git commit -m "feat: migrate installs off Kokoro and reclaim its disk

Three layers, all needed. Migration 0007 drops the voices table and the dead
model settings. A read-time settings migration extends the existing retired-
provider path to kokoro and adds one for default_voice_id: a stored Kokoro
voice id is not merely stale, because playback_voice_style falls back rather
than failing, so it would be silently swapped for M1 on every sentence
forever with nothing surfaced. A best-effort startup sweep reclaims the two
kokoro-*.onnx files (~417MB) and the voices directory.

The sweep never fails a launch, and it names the two model files rather than
the models directory, which Supertonic shares."
```

---

### Task 6: Update the documentation

**Files:**
- Create: `docs/adr/0003-supertonic-is-the-only-bundled-engine.md`
- Modify: `docs/adr/0001-kokoro-runs-in-the-webview.md`, `CLAUDE.md`, `README.md`, `HANDOFF.md`, `docs/superpowers/plans/2026-07-17-ci-release-automation.md`

**Interfaces:**
- Consumes: everything from Tasks 1-5.
- Produces: no code.

- [ ] **Step 1: Mark ADR-0001 superseded**

An ADR is a record, so do not edit its argument. Insert immediately below the `# Kokoro runs in the webview; Supertonic runs in Rust` heading:

```markdown
> **Superseded by [ADR-0003](0003-supertonic-is-the-only-bundled-engine.md) (2026-08-13).**
> Kokoro was removed. The reasoning below is kept because it records why the two
> engines sat on opposite sides of the app, and because its open eSpeak NG
> consequence is what ADR-0003 closes.
```

Change nothing else in the file.

- [ ] **Step 2: Write ADR-0003**

Create `docs/adr/0003-supertonic-is-the-only-bundled-engine.md`:

```markdown
# Supertonic is the only bundled engine

Kokoro never produced audio in a bundled build. Two distinct faults were found, and the
second was never solved.

**Fault 1, root-caused and fixed.** `onnxruntime-web` loads its wasm backend with a dynamic
module `import()`, and `@huggingface/transformers` defaults that path to jsDelivr. The Tauri
CSP allows jsDelivr in `connect-src` but not `script-src`, so the import was blocked. It
worked under `tauri:dev`, where Vite serves `node_modules` as `'self'`, and failed only in a
bundled build — which is why it survived to release.

**Fault 2, never solved.** With the backend loading and the 92 MB model read from disk,
`engine.generate()` hung indefinitely at 0% CPU with zero network sockets, parked on a
promise that never settled. Three hypotheses were tested and all falsified: voice embeddings
fetched at generate time (no sockets ever opened), a missing `espeakng.worker.data` (that
data is inlined in `phonemizer.js` as base64 gzip), and multithreaded wasm starved of
`SharedArrayBuffer` (`numThreads = 1` changed nothing). Do not re-test those three.

Supertonic already ran through the Rust ONNX Runtime, produced audio, and covered playback
and chapter MP3 export. Keeping a second engine that had never worked meant carrying its
model-download subsystem, its 55-voice gallery, its `voices` table and its settings for no
delivered capability.

## Consequences

- `SpeechEngineId` has one member. `createSpeechEngine` keeps its `switch` — see
  ADR-0001's reasoning about where engine choice lives — so adding a provider is still a
  one-case change.
- The voice gallery is gone. Supertonic's ten voice styles are a static list chosen in
  Settings, and nothing about them needs downloading.
- **ADR-0001's open eSpeak NG consequence is closed.** `kokoro-js` bundled eSpeak NG as
  WASM via `phonemizer`, and that shipped in the app. Removing the dependency takes a
  GPL-3.0-or-later payload out of an Apache-2.0 binary. This is the largest effect of the
  change and it is invisible in the diff.
- Playback is single-engine until Fish Audio lands as a bring-your-own-API-key provider.
```

- [ ] **Step 3: Update `CLAUDE.md`**

Three edits:

- Line 12 — the `lib/` inventory reads ``` `kokoro.ts` + `supertonic.ts` (TTS engines)```. Change to ``` `supertonic.ts` (the TTS engine)```.
- Line 22 — replace the whole "**TTS is split across two engines:**" sentence with:

```markdown
**TTS runs in Rust.** **Supertonic** playback and chapter-MP3 export go through the **Rust ONNX Runtime** (`ort`) backend with on-demand model downloads. `ffmpeg` (external sidecar bin) + `mp3lame` handle encoding. Engine choice lives in one place, `createSpeechEngine` in `src/lib/speech/index.ts`; Kokoro was removed in favour of Supertonic (ADR-0003).
```

- Line 62 — change `across system/Kokoro/Supertonic paths` to `on the Supertonic path`.

- [ ] **Step 4: Update `README.md`**

Replace the paragraph at line 34 (`Kokoro playback runs through the bundled webview...`) with:

```markdown
Supertonic playback and chapter MP3 export run through the Rust ONNX Runtime
backend with on-demand model downloads.
```

- [ ] **Step 5: Update `HANDOFF.md`**

Four edits:

- Line 67 — change `More math-aware TTS normalization for system/Kokoro/Supertonic paths.` to `More math-aware TTS normalization for the Supertonic path.`
- Line 374 — change `Supertonic/Kokoro math speech normalization is heuristic.` to `Supertonic math speech normalization is heuristic.`
- Line 375 — **delete** the bullet about large-chunk bundle warnings for `kokoro.web`. That warning no longer exists; rewording it would document a problem that is gone.
- The "Next Up — TTS direction" section (lines 79-161): replace subsection **A. Remove Kokoro** (lines 88-101) with a two-line pointer to the shipped work:

```markdown
### A. Remove Kokoro — DONE (2026-08-13)

Spec: `docs/superpowers/specs/2026-08-13-remove-kokoro-design.md`.
Plan: `docs/superpowers/plans/2026-08-13-remove-kokoro.md`. See ADR-0003.
```

Leave section **B. Add Fish Audio** and the "Why Kokoro is being dropped" section intact — B is the next spec, and the fault write-up is still the reference ADR-0003 summarises.

- [ ] **Step 6: Update the stale CI-release plan note**

`docs/superpowers/plans/2026-07-17-ci-release-automation.md:49` says large-chunk warnings for `kokoro.web` are pre-existing and acceptable. Change that sentence to:

```markdown
Expected: PASS (tsc + vite build with no errors).
```

- [ ] **Step 7: Confirm nothing stale survives**

Run: `grep -rn -i "kokoro" --exclude-dir=node_modules --exclude-dir=target --exclude-dir=.git --exclude=package-lock.json .`

Expected hits, all intentional:
- `docs/adr/0001-*.md` — the superseded record
- `docs/adr/0003-*.md` — the new record
- `docs/superpowers/specs/2026-08-13-remove-kokoro-design.md` and this plan
- `HANDOFF.md` — the "Why Kokoro is being dropped" section and the section-A pointer
- `src-tauri/src/tts/mod.rs` — the ADR-0003 pointer
- `src-tauri/src/tts/supertonic/voice.rs` — the migration-rationale test comment
- `src/stores/settings.ts` — `asTtsProvider`'s retired-value list and its comment

Anything else is a miss. In particular there must be **zero** hits in `package.json`, `README.md`, `CLAUDE.md`, `models/`, or `src-tauri/resources/`.

- [ ] **Step 8: Run the whole gate one final time**

```bash
npm run build && npm test
cargo test -p libretexts-reader
cargo clippy -p libretexts-reader -- -D warnings
cargo fmt --check
scripts/ci/check-identifier.sh
scripts/ci/check-error-kinds.sh
git diff --check
```

Expected: all PASS.

- [ ] **Step 9: Commit**

```bash
git add -A docs/ CLAUDE.md README.md HANDOFF.md
git commit -m "docs: record the Kokoro removal

ADR-0001 is marked superseded rather than edited — its argument about why the
two engines sat on opposite sides of the app is still the record, and its open
eSpeak NG consequence is what ADR-0003 closes: removing kokoro-js takes a
GPL-3.0-or-later WASM payload out of an Apache-2.0 binary.

Deletes the HANDOFF note about kokoro.web bundle warnings rather than
rewording it. That warning no longer exists."
```

---

## Manual verification (after Task 6)

Automated tests cannot catch the failure modes this change actually risks, because all of them need a database and a directory that predate the change. A clean-machine launch proves nothing here.

- [ ] **Build a runnable binary**

```bash
npm run tauri -- build --debug --no-bundle
```

- [ ] **Point it at a pre-existing app-data directory**

Use the real one at `~/Library/Application Support/dev.johnnylibretexts.reader` if it already holds `models/kokoro-*.onnx` and a populated `voices` table. Otherwise make a stand-in from a copy before checking out this branch, so the DB carries the pre-`0007` schema.

- [ ] **Launch it directly — never with `open`**

```bash
./target/debug/libretexts-reader
```

`--no-bundle` produces a bare Mach-O with no `.app`; `open` on it can exit 0 while starting nothing, which reads as a broken build.

- [ ] **Check all five outcomes**

1. **The app starts.** Catches the Task 1 ordering hazard — a live `seed_voice_catalog` against a dropped table fails on every launch.
2. **Supertonic playback produces audio** and the mini-player reports a real voice. Catches a `default_voice_id` migration that did not fire.
3. **`ls "$APP_DATA/models"`** shows no `kokoro-*.onnx` and still shows the Supertonic version directory. Catches a sweep that deleted too much or too little.
4. **Quit, relaunch, then `ls "$APP_DATA"`** — `voices/` has **not** reappeared. Catches the Task 4 hazard, a surviving `voices_dir()` resolver quietly recreating what the sweep removed.
5. **Open an imported book with figures.** Covers and section images still render. This coupling has never been exercised end to end on this machine (`HANDOFF.md:182`) and this change touches `paths.rs`, so it rides along here.

- [ ] **Confirm the reclaim**

```bash
du -sh ~/Library/Application\ Support/dev.johnnylibretexts.reader
```

Expected: roughly 417 MB smaller than before the launch, absent a Supertonic model download in between.
