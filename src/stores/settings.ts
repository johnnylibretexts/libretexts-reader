import { create } from "zustand";
import {
  asSupertonicLanguage,
  asSupertonicVoiceStyle,
  type SupertonicLanguage,
  type SupertonicVoiceStyle,
} from "../lib/supertonic";
import { api } from "../lib/tauri";
import { displayError } from "../lib/errors";
import type { TtsProvider } from "../types/domain";

export type AppTheme = "light" | "dark" | "system";
/**
 * Mirrors `SpeechEngineId` -- every provider here is one a reader can pick.
 * Re-exported from `types/domain.ts`, which is also where `tauri.ts` gets it
 * for the `provider` field Rust requires on synthesis and chapter requests;
 * declaring it there once means this store and the invoke layer cannot drift
 * apart without a compile error.
 */
export type { TtsProvider };

/**
 * No `ttsProvider`. `setTtsProvider` is the sole writer of that row: it also
 * applies the value only once the write lands, tracks what committed, and is
 * what `switchToSupertonic` goes through. A second way in here would be a
 * second set of those rules to keep in step, for a field no caller passes.
 */
export interface TtsSettingsPatch {
  supertonicVoiceStyle?: SupertonicVoiceStyle;
  supertonicLanguage?: SupertonicLanguage;
  /** `null` is the durable form of “Original language”. */
  translationTargetLang?: SupertonicLanguage | null;
  fishVoiceId?: string | null;
}

export interface SettingsState {
  defaultSpeed: number;
  exportDirectory: string;
  theme: AppTheme;
  ttsProvider: TtsProvider;
  supertonicVoiceStyle: SupertonicVoiceStyle;
  supertonicLanguage: SupertonicLanguage;
  /** The language narration is translated into, or null to keep each book original. */
  translationTargetLang: SupertonicLanguage | null;
  /**
   * The reader's chosen Fish voice id, or null when none has been picked yet.
   * Settings UI for this lands in a later task; declared here now because
   * `createSpeechEngine` requires it on every `SpeechEngineSettings`, and this
   * store hands its whole state to that function.
   */
  fishVoiceId: string | null;
}

export interface SettingsStore extends SettingsState {
  hydrated: boolean;
  /**
   * True when `hydrate` fell back to DEFAULT_SETTINGS because the load
   * failed. `hydrated` alone cannot say this, and the difference matters:
   * every value on screen is then a built-in default rather than the
   * reader's, so anything that would write those values back over their real
   * rows has to refuse. Playback reads two of these rows now, which is what
   * turned a cosmetic fallback into one that can overwrite.
   */
  hydrateFailed: boolean;
  loading: boolean;
  /**
   * The banner for settings actions with nowhere else to report -- a failed
   * hydrate, and the theme and provider controls, neither of which renders an
   * error of its own. Deliberately not written by `saveTtsSettings`: both of
   * its callers render what they catch next to the control that caused it,
   * and SettingsPanel is the only view of this field, so a copy here would be
   * a message nothing displays -- one that then outlives the component that
   * was supposed to display it.
   *
   * Each action owns this field: it clears it when it next succeeds and
   * writes it when it next fails. Nothing else clears it, so a failed
   * provider switch keeps saying so while an unrelated save succeeds beside
   * it -- which is accurate, the provider change really did not stick.
   *
   * `hydrate` is no exception, and it is the case where that matters most:
   * a failed load holds this field until another `hydrate` clears it, which
   * in practice means the reader's retry. A theme or provider action
   * succeeding beside it does nothing -- `releaseBanner` no-ops for anyone
   * but the owner. That is deliberate: the panel is showing DEFAULT_SETTINGS
   * rather than the reader's stored rows, and an unrelated save succeeding
   * does not make that any less true.
   */
  error: string | null;
  hydrate: () => Promise<void>;
  setTheme: (theme: AppTheme) => Promise<void>;
  setTtsProvider: (provider: TtsProvider) => Promise<void>;
  saveTtsSettings: (settings: TtsSettingsPatch) => Promise<void>;
}

const DEFAULT_SETTINGS: SettingsState = {
  defaultSpeed: 1,
  exportDirectory: "",
  theme: "system",
  ttsProvider: "supertonic",
  supertonicVoiceStyle: "M1",
  supertonicLanguage: "en",
  translationTargetLang: null,
  fishVoiceId: null,
};

/** One settings row a `saveTtsSettings` patch asked to change. */
interface SettingRow {
  name: string;
  /**
   * How this row is named to the reader when its own write fails. Rows are
   * written independently, so some land while others do not -- a bare reason
   * reads as "nothing saved" while the rows that did land are on disk and in
   * force, including after the next launch.
   */
  label: string;
  value: unknown;
  apply: Partial<SettingsState>;
}

function merged(patches: Partial<SettingsState>[]): Partial<SettingsState> {
  return Object.assign({}, ...patches) as Partial<SettingsState>;
}

/**
 * Serializes writes to one settings row.
 *
 * The controls that drive these fire unawaited, so two clicks inside one
 * write's window are two concurrent invokes; left to race, the store keeps
 * whichever *resolved* last while SQLite keeps whichever *committed* last.
 * Chaining makes commit order click order, so the two agree and the last
 * click is what both hold.
 *
 * The cost of that ordering: a slow write holds up the next one on the same
 * row. Bounded -- `set_setting` waits on the r2d2 pool, which times out --
 * and no worse in kind than applying after the write already is, since no
 * click can show until its own write lands.
 */
function rowSerializer() {
  let queue: Promise<unknown> = Promise.resolve();
  return (write: () => Promise<void>) => {
    const next = queue.then(write, write);
    queue = next.catch(() => undefined);
    return next;
  };
}

/**
 * One serializer per row, made on first use.
 *
 * Every writer of a row has to go through this or the ordering guarantee is
 * worth nothing -- including `switchToSupertonic`, the MiniPlayer's escape
 * hatch out of a Fish failure, which awaits a `tts_provider` write before
 * resuming playback. So a write already stuck on the pool delays that button
 * too, at the moment it is most wanted; the alternative is worse, since an
 * unordered escape hatch could commit "supertonic" and then have a Settings
 * write land "fish" on top of it.
 *
 * Per row rather than global: rows are independent, and making a slow voice
 * save hold up a theme click would be latency for nothing.
 */
const rowWriters = new Map<string, ReturnType<typeof rowSerializer>>();

/**
 * Which action's message the shared `error` currently holds, so only that
 * action can clear it.
 *
 * The rows have independent queues and so run concurrently: without this, a
 * provider switch landing after a theme save failed wiped the theme's
 * message on its way past -- and the Sidebar's theme buttons swallow their
 * rejection and rely on this field alone, so the reader saw the theme snap
 * back and no reason why. Not store state: no view needs it, only the store
 * decides who may clear what.
 */
let bannerOwner: "hydrate" | "theme" | "tts-provider" | null = null;

function writeRow(name: string, value: unknown): Promise<void> {
  let writer = rowWriters.get(name);
  if (!writer) {
    writer = rowSerializer();
    rowWriters.set(name, writer);
  }
  return writer(() => api.setSetting(name, value));
}
/** Tracks the newest `setTheme` call so an older one cannot apply over it. */
let themeSeq = 0;
/** The same for `setTtsProvider`, and for a sharper reason -- see there. */
let providerSeq = 0;
/**
 * The provider last seen on disk, for the same job `committedTheme` does.
 *
 * A superseded write still commits, and the click that replaced it can then
 * fail -- at which point neither has applied anything and the store is left
 * naming a provider SQLite does not hold. The next launch would start on the
 * other one, billing through a provider the screen said was not selected.
 */
let committedProvider: TtsProvider | undefined;
/**
 * The theme last seen on disk -- loaded, or written and committed.
 *
 * What a failed write reverts to. The value the call started from cannot
 * serve: it is read before the write, so a load resolving in between makes it
 * stale, and reverting to it puts the store and localStorage on something
 * SQLite never held -- which `hydrate`'s failure path then reads back at the
 * next launch. Two failed clicks in a row have the same problem, the second
 * reverting to the first's optimistic value.
 */
let committedTheme: AppTheme | undefined;

/**
 * Rows that have been written since the in-flight `hydrate` read began.
 *
 * `hydrate` is retryable now, so its read is no longer the first thing that
 * happens: a write can commit while it is in the air, making the values it
 * comes back with older than what SQLite holds. Applying them wholesale would
 * revert that write in the store while the DB kept it -- the same divergence
 * `setTtsProvider` and `saveTtsSettings` were both reshaped to prevent. These
 * are layered back over the loaded values so the newer write wins.
 */
let writesSinceLoad: Partial<SettingsState> = {};

/** Records a row write so an older in-flight load cannot undo it. */
function recordWrite(applied: Partial<SettingsState>) {
  writesSinceLoad = { ...writesSinceLoad, ...applied };
}

export const useSettingsStore = create<SettingsStore>((set, get) => {
  /** Records who the banner belongs to as it is written. */
  function ownBanner(owner: NonNullable<typeof bannerOwner>, message: string) {
    bannerOwner = owner;
    return { error: message };
  }

  /** Clears the banner only if it is still this action's to clear. */
  function releaseBanner(owner: NonNullable<typeof bannerOwner>) {
    if (bannerOwner !== owner) {
      return {};
    }
    bannerOwner = null;
    return { error: null };
  }

  return {
  ...DEFAULT_SETTINGS,
  hydrated: false,
  hydrateFailed: false,
  loading: false,
  error: null,
  hydrate: async () => {
    const { hydrated, hydrateFailed, loading } = get();
    // `hydrateFailed` reopens the door: a load that failed left every row at
    // DEFAULT_SETTINGS, and playback builds its engine from two of them, so
    // without a retry one transient failure means a whole session read in
    // "M1" -- and a Fish reader silently moved onto Supertonic and asked to
    // download its model. Restarting the app was the only way out.
    if ((hydrated && !hydrateFailed) || loading) {
      return;
    }

    set({ loading: true, ...releaseBanner("hydrate") });
    writesSinceLoad = {};
    try {
      // Legacy stored values are coerced in asTtsProvider, so anything that
      // survives loadSettings is already a provider a reader can pick.
      const loaded = await loadSettings();
      // Same precedence as the `set` below, where `writesSinceLoad` spreads
      // last: a write that committed while the read was in flight is newer
      // than the read. Preferring the read would make this name a value
      // SQLite no longer holds, and the next failed click would revert the
      // store and localStorage onto it.
      committedProvider =
        writesSinceLoad.ttsProvider ??
        definedSettings(loaded).ttsProvider ??
        DEFAULT_SETTINGS.ttsProvider;
      committedTheme =
        writesSinceLoad.theme ??
        (definedSettings(loaded).theme as AppTheme | undefined) ??
        DEFAULT_SETTINGS.theme;
      set({
        ...DEFAULT_SETTINGS,
        ...definedSettings(loaded),
        // Anything committed while the read was in flight is newer than the
        // read -- see `writesSinceLoad`.
        ...writesSinceLoad,
        hydrated: true,
        hydrateFailed: false,
        loading: false,
      });
    } catch (error) {
      // Only the load's own outcome. Resetting the rows to DEFAULT_SETTINGS
      // was harmless while this ran once -- they were already the defaults --
      // but on a retry it reverts writes that did commit, leaving the store
      // saying Supertonic while SQLite says Fish and the next launch flips
      // back. A failed load learns nothing about those rows, so it changes
      // nothing about them.
      set({
        theme: localTheme() ?? get().theme,
        hydrated: true,
        hydrateFailed: true,
        loading: false,
        ...ownBanner("hydrate", displayError(error)),
      });
    }
  },
  setTheme: async (theme: AppTheme) => {
    const previousTheme = get().theme;
    // Which click this is. Everything after the await is skipped for a call
    // a later click has superseded -- otherwise an older write resolving
    // second re-applies its theme over the newer one, and the UI snaps back
    // to a click the reader has already changed their mind about.
    const seq = ++themeSeq;
    // Optimistic here, unlike the other two rows, because the theme is the
    // one thing that has to change the instant it is clicked. localStorage
    // moves with it: `hydrate`'s failure path reads it back.
    //
    // The banner is deliberately left alone until this write lands. It is the
    // only place a failed provider switch is reported -- that control has no
    // error line of its own -- and clearing it on a theme click makes that
    // failure vanish while nothing about it has been fixed.
    set({ theme });
    persistLocalTheme(theme);

    try {
      await writeRow("theme", theme);
      // After the write, not before: this one sets optimistically and reverts,
      // and a reverted value is not something an in-flight load should be
      // stopped from overwriting.
      //
      // Re-applied as well as recorded, like the other two writers. The
      // optimistic set above happens before `recordWrite`, so a load that
      // resolves in that window overwrites the theme with its older snapshot
      // and `writesSinceLoad` -- empty until this line -- has nothing to
      // layer back. Recording alone would leave the store and the rendered
      // theme on the old value while SQLite and localStorage held the new
      // one, and the next launch would flip.
      // Recorded whether or not a later click superseded this one: the two
      // guards answer different questions. `recordWrite` says "this value is
      // on disk", which a superseded write that committed still is, and an
      // in-flight load that reverted it would put the store, localStorage and
      // SQLite into three different states. The `set` below is what must not
      // repaint over a newer click. Writes are serialized, so the last one to
      // record is the last one committed.
      recordWrite({ theme });
      committedTheme = theme;
      if (seq === themeSeq) {
        // `error: null` because this click succeeded: an earlier one that
        // failed and was superseded still writes the banner, and leaving it
        // up says the theme did not save when the one the reader is looking
        // at did.
        set({ theme, ...releaseBanner("theme") });
      }
    } catch (error) {
      // Revert the optimistic set before rethrowing: a failed persist must
      // not leave the store (or the localStorage fallback `hydrate` reads
      // on its own failure path) claiming a value that was never saved, or
      // this session disagrees with what the next app start loads. Set the
      // shared banner message, then rethrow so a caller awaiting this call
      // sees the same failure via its own try/catch rather than having to
      // read this store's mutable `error` field, which a concurrent
      // unrelated action could overwrite or clear first. Every call site
      // must catch this — see Sidebar.tsx.
      // A superseded call must not revert past the click that replaced it:
      // the reader has since chosen something else, and that choice is
      // already on screen and in localStorage.
      if (seq === themeSeq) {
        const revertTo = committedTheme ?? previousTheme;
        set({ theme: revertTo, ...ownBanner("theme", displayError(error)) });
        persistLocalTheme(revertTo);
      } else {
        set(ownBanner("theme", displayError(error)));
      }
      throw error;
    }
  },
  setTtsProvider: async (provider: TtsProvider) => {
    // Applied only once the write lands, like `saveTtsSettings` and for a
    // sharper version of the same reason: `ttsProvider` is the first
    // component of `engineKey`, which playback reads at every sentence
    // boundary. Setting it first would let an auto-advance inside that window
    // build an engine for a provider that was never saved -- and when that
    // provider is Fish, synthesize over the network and bill the reader for a
    // selection that then failed to stick. It also cuts the mirror case: a
    // failed switch would strand the in-flight lookahead, whose liveness
    // guard trips on the optimistic value and stops for a change that never
    // happened.
    // Which click this is. Only the button being written is disabled, so
    // clicking one provider then the other is an invited interaction.
    const seq = ++providerSeq;

    try {
      await writeRow("tts_provider", provider);
    } catch (error) {
      // This control renders no error of its own, so the shared banner is
      // where its failure is reported. Rethrown as well: callers detect their
      // own failure by catching, never by reading that mutable field.
      // A superseded click must not revert past the one that replaced it, but
      // its failure is still worth reporting -- this control has no error
      // line of its own.
      if (seq === providerSeq && committedProvider !== undefined) {
        // Reconcile with disk: an earlier click may have committed while
        // superseded, leaving the store on a provider that is not the stored
        // one. See `committedProvider`.
        set({
          ttsProvider: committedProvider,
          ...ownBanner("tts-provider", displayError(error)),
        });
      } else {
        set(ownBanner("tts-provider", displayError(error)));
      }
      throw error;
    }

    // Recorded whether or not a later click superseded this one: it
    // committed, so an in-flight load must not revert it. Writes are
    // serialized, so the last to record is the last committed.
    recordWrite({ ttsProvider: provider });
    committedProvider = provider;
    if (seq !== providerSeq) {
      // Applying it would put a provider the reader has already clicked past
      // into the store for the length of the next write -- and `activeEngine`
      // reads the store live, so a sentence boundary in that window builds
      // that engine and, on Fish, issues a billed request for audio filed
      // under a key nothing will read again.
      return;
    }
    set({ ttsProvider: provider, ...releaseBanner("tts-provider") });
  },
  saveTtsSettings: async (ttsSettings: TtsSettingsPatch) => {
    // Only the rows this patch names are written. Writing all four on every
    // call let two saves on one screen clobber each other -- FishAudioSettings
    // renders inside SettingsPanel -- because a {fishVoiceId} patch also
    // rewrote supertonic_voice_style with the value it read at its own start,
    // undoing a style the other save had committed in between.
    const rows: SettingRow[] = [];

    if (ttsSettings.supertonicVoiceStyle !== undefined) {
      rows.push({
        name: "supertonic_voice_style",
        label: "Voice style",
        value: ttsSettings.supertonicVoiceStyle,
        apply: { supertonicVoiceStyle: ttsSettings.supertonicVoiceStyle },
      });
    }
    if (ttsSettings.supertonicLanguage !== undefined) {
      rows.push({
        name: "supertonic_language",
        label: "Language",
        value: ttsSettings.supertonicLanguage,
        apply: { supertonicLanguage: ttsSettings.supertonicLanguage },
      });
    }
    if (ttsSettings.translationTargetLang !== undefined) {
      rows.push({
        name: "translation_target_lang",
        label: "Read aloud language",
        value: ttsSettings.translationTargetLang,
        apply: {
          translationTargetLang: ttsSettings.translationTargetLang,
        },
      });
    }
    if (ttsSettings.fishVoiceId !== undefined) {
      rows.push({
        name: "fish_voice_id",
        label: "Fish Audio voice",
        value: ttsSettings.fishVoiceId,
        apply: { fishVoiceId: ttsSettings.fishVoiceId },
      });
    }

    if (rows.length === 0) {
      return;
    }

    // `allSettled`, not `all`: these writes are independent and all go out
    // regardless, so one rejecting says nothing about the others. `all`
    // rejects on the first failure while the rest still commit, and reverting
    // the lot would put the store back to values the DB no longer holds --
    // the same store/DB disagreement the revert exists to prevent, pointing
    // the other way.
    // `async` on the callback is load-bearing: `allSettled` only settles
    // promises handed to it, and a `setSetting` that throws synchronously
    // throws inside this `.map` -- escaping the settled-results path below
    // entirely, so the rejection never reaches the caller and no row is
    // applied or reported. The async wrapper turns it into a rejection like
    // any other.
    const results = await Promise.allSettled(
      // Through the same per-row queues as every other writer: the panel's
      // `disabled={saving}` is the only thing keeping two of these apart
      // today, and that does not survive the panel unmounting mid-write --
      // navigate away and back and Save again, and two unordered writes land
      // on one row.
      rows.map(async (row) => writeRow(row.name, row.value)),
    );
    // Applied after the fact, and only for rows that actually landed, so the
    // store can never claim a value the DB does not hold -- in either
    // direction, and with no in-flight window. That matters most for
    // `supertonicVoiceStyle`: playback keys its engine on it, so a value set
    // before its write resolved would have the reader hearing, and on Fish
    // paying for, a voice that a failure then took back. `setTheme` next
    // door still sets optimistically and reverts: no engine is built from the
    // theme, so nothing reads it mid-playback.
    const applied = merged(
      rows
        .filter((_, index) => results[index].status === "fulfilled")
        .map((row) => row.apply),
    );
    recordWrite(applied);
    set(applied);

    const failed = results.flatMap((result, index) =>
      result.status === "rejected"
        ? [{ row: rows[index], reason: result.reason }]
        : [],
    );
    if (failed.length === 0) {
      return;
    }

    // Naming rows earns its keep in exactly two cases. One: a partial
    // failure, where the rows that landed stay landed, so a bare "disk full"
    // reads as "nothing saved" while the language the reader picked is on
    // disk and in force from here on. Two: rows failing for separately
    // actionable causes, where reporting one hides the other.
    //
    // Otherwise -- everything asked for failed, for one cause, which is what
    // a locked database or a read-only disk actually does -- labels only
    // stutter that cause once per row, and a lone row's label just repeats
    // the control the reader is already looking at.
    const reasons = failed.map((failure) => displayError(failure.reason));
    const named = failed.length < rows.length || new Set(reasons).size > 1;

    if (!named) {
      // Nothing to combine, so the caller gets the backend error exactly as
      // it arrived -- structured `kind` and all, for anything that classifies
      // rejections (see `asAppError` in errors.ts).
      throw failed[0].reason;
    }
    // A synthesized message is the only value that describes every failed
    // row, so no single structured reason can survive here. Nothing reads
    // one: both callers only display what they catch. If something ever needs
    // to branch on `kind` after a partial save, that is the moment to carry
    // the reasons along -- not before.
    throw new Error(
      failed
        .map((failure, index) => `${failure.row.label}: ${reasons[index]}`)
        .join("; "),
    );
  },
  };
});

export async function loadSettings(): Promise<Partial<SettingsState>> {
  const settings = await api.getAllSettings();
  return definedSettings({
    defaultSpeed: asNumber(settings.default_speed),
    exportDirectory: asString(settings.export_directory),
    theme: asTheme(settings.theme),
    ttsProvider: asTtsProvider(settings.tts_provider),
    supertonicVoiceStyle: asSupertonicVoiceStyle(
      settings.supertonic_voice_style,
    ),
    supertonicLanguage: asSupertonicLanguage(settings.supertonic_language),
    translationTargetLang: asTranslationTargetLanguage(
      settings.translation_target_lang,
    ),
    fishVoiceId: asString(settings.fish_voice_id) ?? null,
  });
}

function definedSettings(settings: Partial<SettingsState>) {
  return Object.fromEntries(
    Object.entries(settings).filter(([, value]) => value !== undefined),
  ) as Partial<SettingsState>;
}

function asString(value: unknown): string | undefined {
  return typeof value === "string" ? value : undefined;
}

function asNumber(value: unknown): number | undefined {
  return typeof value === "number" ? value : undefined;
}

function asTranslationTargetLanguage(
  value: unknown,
): SupertonicLanguage | null | undefined {
  return value === null ? null : asSupertonicLanguage(value);
}

function asTheme(value: unknown): AppTheme | undefined {
  return value === "light" || value === "dark" || value === "system"
    ? value
    : undefined;
}

/**
 * The single place stored provider values are interpreted, including retired
 * ones. `system` was the Web Speech path, removed once every engine sat behind
 * SpeechEngine; `gemini` predates Supertonic; `kokoro` was removed once it
 * proved it could not produce audio in a bundled build. Retired values fall
 * back to the default rather than being written back. Nothing rewrites the
 * row either: `saveTtsSettings` writes only the rows its patch names and no
 * caller names `ttsProvider` (the provider control goes through
 * `setTtsProvider`), so a stale value survives on disk and is coerced on
 * every read -- here in the webview, and by `migrate_removed_tts_provider`
 * in `db/settings.rs` on the Rust side.
 *
 * `fish` is no longer retired: it is a real, selectable provider again (see
 * `SpeechEngineId` in `lib/speech/types.ts`).
 */
function asTtsProvider(value: unknown): TtsProvider | undefined {
  if (value === "gemini" || value === "kokoro") {
    return "supertonic";
  }
  return value === "supertonic" || value === "fish" ? value : undefined;
}

// The localStorage key is intentionally still "johnny-reader-theme" from the
// pre-rename app name. Do NOT rename it to match the LibreTexts Reader
// rebrand — every existing user's stored key would stop matching and their
// theme preference would silently reset to "system". Leave this key alone.
function localTheme(): AppTheme | undefined {
  if (typeof window === "undefined") {
    return undefined;
  }

  return asTheme(window.localStorage.getItem("johnny-reader-theme"));
}

function persistLocalTheme(theme: AppTheme) {
  if (typeof window !== "undefined") {
    window.localStorage.setItem("johnny-reader-theme", theme);
  }
}
