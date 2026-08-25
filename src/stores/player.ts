import { create } from "zustand";
import { api } from "../lib/tauri";
import { asAppError, displayError } from "../lib/errors";
import {
  createSpeechEngine,
  SPEECH_ENGINE_LABELS,
  SpeechAbortedError,
  type SettingsSource,
  type SpeechEngine,
} from "../lib/speech";
import { useSettingsStore } from "./settings";
import type { SettingsStore } from "./settings";
import { useTranslationStore } from "./translation";
import type * as Domain from "../types/domain";

interface Position {
  sectionIndex: number;
  paragraphIndex: number;
  sentenceIndex: number;
}

interface PlayerState {
  document: Domain.Document | null;
  sections: Domain.Section[];
  currentSectionIndex: number;
  paragraphs: Domain.Paragraph[];
  sectionImages: Domain.SectionImage[];
  currentParagraphIndex: number;
  currentSentenceIndex: number;
  speed: number;
  isPlaying: boolean;
  isBuffering: boolean;
  bufferingMessage: string;
  /**
   * The one-time voice-model fetch, while it is running. Null the rest of the
   * time, including while ordinary audio buffers.
   *
   * Separate from `isBuffering` because the two waits are nothing alike:
   * buffering a sentence is seconds, this is ~383MB over minutes. Rendered
   * from the same flag, the download got the same indeterminate spinner and
   * the same wall of disabled controls, which is what made it read as a hang.
   */
  modelDownload: { downloadedBytes: number; totalBytes: number } | null;
  loading: boolean;
  error: string | null;
  /**
   * True only while `error` describes a Fish Audio synthesis failure: it
   * gates the MiniPlayer's "Switch to Supertonic" action. Never set true for
   * any other engine or failure — offering an automatic-looking escape hatch
   * from a Supertonic error would blur the one distinction this exists to
   * preserve (see switchToSupertonic).
   */
  canSwitchToSupertonic: boolean;
  /**
   * Set when the reader's place could not be written, or could not be read
   * back on open. Null the rest of the time.
   *
   * Deliberately not `error`: nothing about listening has failed, so the red
   * banner that gates "Switch to Supertonic" would be both louder and less
   * accurate than the truth, which is that the bookkeeping stopped. Losing an
   * hour of listening in silence is the outcome this exists to prevent.
   */
  positionError: string | null;
  loadDocument: (documentId: string) => Promise<void>;
  play: () => Promise<void>;
  pause: () => void;
  reset: () => void;
  seekToSentence: (
    paragraphIndex: number,
    sentenceIndex: number,
  ) => Promise<void>;
  setSection: (sectionIndex: number) => Promise<void>;
  setDocumentSourceLanguage: (sourceLanguage: string) => Promise<void>;
  skipForward: () => Promise<void>;
  skipBack: () => Promise<void>;
  skipParagraphForward: () => Promise<void>;
  skipParagraphBack: () => Promise<void>;
  setSpeed: (speed: number) => void;
  /**
   * Stop the voice-model download in flight and release the player.
   *
   * Identical to `pause` in effect -- named separately because that is what
   * the button next to the progress bar does, and a control labelled Cancel
   * calling `pause` reads as a different action than it is.
   */
  cancelModelDownload: () => void;
  /**
   * The one place playback is allowed to change `ttsProvider`. Reachable only
   * from the reader clicking "Switch to Supertonic" after a Fish failure
   * (see `canSwitchToSupertonic`) — never called automatically. Switches the
   * settings store to Supertonic, then resumes speaking the current sentence
   * with the newly-selected engine.
   */
  switchToSupertonic: () => Promise<void>;
}

let utteranceToken = 0;
// Monotonic token so an earlier navigation (document load or section move) that
// resolves late cannot overwrite the state of a newer one.
let navToken = 0;
let activeAudio: HTMLAudioElement | null = null;
let activeAudioUrl: string | null = null;
const SPEECH_INITIAL_BUFFER_SENTENCES = 3;
const SPEECH_LOOKAHEAD_SENTENCES = 10;
// A billing engine reads far less far ahead. Ten sentences per Play is free
// for Supertonic and roughly ten charges for Fish -- paid up front, before the
// reader has heard one of them, and thrown away by the first seek past the
// buffer. Three still covers the gap between sentences without buying most of
// a page the reader may never reach.
export const SPEECH_BILLED_LOOKAHEAD_SENTENCES = 3;
const SPEECH_PREFETCH_CONCURRENCY = 2;
const SPEECH_CACHE_LIMIT = 32;
// Sentinel paragraph/sentence index meaning "the last one in the section".
// moveToPosition clamps it to the real last index once the section's paragraphs
// load, so backward navigation into another section lands at its end.
const LAST_IN_SECTION = Number.MAX_SAFE_INTEGER;

/**
 * The engine in use, rebuilt only when the settings that define it change,
 * paired with the settings snapshot it was built from. Holding it here rather
 * than passing it around keeps every caller below from having to know which
 * engine is active — that decision happens once, in `activeEngine`, and
 * nowhere else in this file.
 *
 * The snapshot travels with the engine because the two must never be read
 * from different moments: `speechCacheKey` describes the audio an engine
 * produced, so keying with settings newer than the engine files that engine's
 * output under a key promising a voice it did not speak in.
 */
interface ActiveEngine {
  engine: SpeechEngine;
  settings: SettingsStore;
}

let cachedEngine: { key: string; active: ActiveEngine } | null = null;

/** Aborts in-flight synthesis for the current utterance. See SpeechEngine. */
let speechAbort: AbortController | null = null;

/**
 * The cancel the active engine handed over while it downloads something large.
 *
 * Module scope rather than state, next to `speechAbort` and for the same
 * reason: it is a handle for stopping work, not something anything renders.
 */
let modelDownloadCancel: (() => Promise<void>) | null = null;

interface SpeechCacheEntry {
  promise: Promise<Blob>;
  lastUsed: number;
}

interface SpeakOptions {
  token?: number;
  requireInitialBuffer?: boolean;
}

const speechCache = new Map<string, SpeechCacheEntry>();

export const usePlayerStore = create<PlayerState>((set, get) => ({
  document: null,
  sections: [],
  currentSectionIndex: 0,
  paragraphs: [],
  sectionImages: [],
  currentParagraphIndex: 0,
  currentSentenceIndex: 0,
  speed: 1,
  isPlaying: false,
  isBuffering: false,
  bufferingMessage: "",
  modelDownload: null,
  loading: false,
  error: null,
  canSwitchToSupertonic: false,
  positionError: null,

  loadDocument: async (documentId: string) => {
    const requestId = ++navToken;
    cancelSpeech();
    set({
      loading: true,
      error: null,
      canSwitchToSupertonic: false,
      isPlaying: false,
      isBuffering: false,
      bufferingMessage: "",
      modelDownload: null,
    });

    try {
      const document = await api.getDocument(documentId);
      const [sections, restored] = await Promise.all([
        api.listSections(documentId),
        readPlaybackState(documentId),
      ]);
      const saved = restored.state;
      if (requestId !== navToken) {
        return;
      }
      if (sections.length === 0) {
        set({
          document,
          sections: [],
          paragraphs: [],
          sectionImages: [],
          loading: false,
          error: "This document has no readable sections.",
          canSwitchToSupertonic: false,
        });
        return;
      }

      // Which section to open is the resume cursor's first decision, and it
      // has to be made before the paragraphs are fetched -- this used to load
      // `sections[0]` unconditionally, so a cursor in chapter seven would have
      // indexed chapter one's paragraphs.
      const sectionIndex = Math.max(
        0,
        sections.findIndex((section) => section.id === saved?.sectionId),
      );
      const [paragraphs, sectionImages] = await Promise.all([
        api.listParagraphs(
          sections[sectionIndex].id,
          useSettingsStore.getState().translationTargetLang,
        ),
        api.listSectionImages(sections[sectionIndex].id),
      ]);
      if (requestId !== navToken) {
        return;
      }
      const resumed = resumePosition(saved, sections[sectionIndex], paragraphs);
      set({
        document,
        sections,
        paragraphs,
        sectionImages,
        currentSectionIndex: sectionIndex,
        currentParagraphIndex: resumed.paragraphIndex,
        currentSentenceIndex: resumed.sentenceIndex,
        // Speed is per document -- a dense textbook and a novel do not want
        // the same one -- so the reader's global default is only the opening
        // value for a book that has never been played.
        speed: saved?.speed ?? useSettingsStore.getState().defaultSpeed,
        loading: false,
        error: null,
        canSwitchToSupertonic: false,
      });
      await persistPlaybackState(set, get());
      // After the persist, not before: a successful write clears the notice,
      // and a book that opened at page one because its cursor could not be
      // read is still worth saying out loud.
      if (restored.failure) {
        set({ positionError: restored.failure });
      }
    } catch (error) {
      if (requestId !== navToken) {
        return;
      }
      set({
        loading: false,
        error: displayError(error),
        canSwitchToSupertonic: false,
      });
    }
  },

  play: async () => {
    if (
      needsCurrentSectionTranslation(get()) &&
      !(await prepareCurrentSectionTranslation(set, get))
    ) {
      return;
    }
    await speakCurrentSentence(set, get);
  },

  pause: () => {
    // A model download is part of what Pause has to stop: it is the only
    // reason playback has not started, and leaving it running would keep
    // pulling ~383MB after the reader said stop.
    stopModelDownload();
    cancelSpeech();
    set({
      isPlaying: false,
      isBuffering: false,
      bufferingMessage: "",
      modelDownload: null,
    });
  },

  reset: () => {
    cancelSpeech();
    // Invalidate any in-flight load/navigation so a late response cannot
    // repopulate the state we are clearing here.
    navToken += 1;
    set({
      document: null,
      sections: [],
      paragraphs: [],
      sectionImages: [],
      currentSectionIndex: 0,
      currentParagraphIndex: 0,
      currentSentenceIndex: 0,
      isPlaying: false,
      isBuffering: false,
      bufferingMessage: "",
      modelDownload: null,
      loading: false,
      error: null,
      canSwitchToSupertonic: false,
      positionError: null,
    });
  },

  seekToSentence: async (paragraphIndex: number, sentenceIndex: number) => {
    const wasPlaying = get().isPlaying;
    await moveToPosition(set, get, {
      sectionIndex: get().currentSectionIndex,
      paragraphIndex,
      sentenceIndex,
    });
    if (wasPlaying) {
      await speakCurrentSentence(set, get);
    }
  },

  setSection: async (sectionIndex: number) => {
    const wasPlaying = get().isPlaying;
    await moveToPosition(set, get, {
      sectionIndex,
      paragraphIndex: 0,
      sentenceIndex: 0,
    });
    if (wasPlaying) {
      await get().play();
    }
  },

  setDocumentSourceLanguage: async (sourceLanguage: string) => {
    const document = get().document;
    if (!document || document.sourceLanguage === sourceLanguage) {
      return;
    }

    get().pause();
    if (useTranslationStore.getState().sectionState.status === "running") {
      await useTranslationStore.getState().cancel();
    }
    try {
      await api.setDocumentSourceLanguage(document.id, sourceLanguage);
      if (get().document?.id !== document.id) {
        return;
      }
      const normalized = sourceLanguage.trim().toLowerCase().replace(/_/g, "-");
      const section = get().sections[get().currentSectionIndex];
      const paragraphs = section
        ? await api.listParagraphs(
            section.id,
            useSettingsStore.getState().translationTargetLang,
          )
        : [];
      if (get().document?.id !== document.id) {
        return;
      }
      set({
        document: { ...document, sourceLanguage: normalized },
        paragraphs,
        error: null,
        canSwitchToSupertonic: false,
      });
      useTranslationStore.setState({
        sectionState: {
          status: "idle",
          done: 0,
          total: 0,
          fallbackCount: 0,
          sentenceCount: 0,
          error: null,
        },
      });
    } catch (error) {
      set({ error: displayError(error), canSwitchToSupertonic: false });
    }
  },

  skipForward: async () => {
    await advanceBySentence(set, get, 1);
  },

  skipBack: async () => {
    await advanceBySentence(set, get, -1);
  },

  skipParagraphForward: async () => {
    await advanceByParagraph(set, get, 1);
  },

  skipParagraphBack: async () => {
    await advanceByParagraph(set, get, -1);
  },

  setSpeed: (speed: number) => {
    set({ speed });
  },

  cancelModelDownload: () => {
    get().pause();
  },

  switchToSupertonic: async () => {
    // This is the only place `ttsProvider` may change as a consequence of a
    // Fish failure, and it only runs from here — a click on the MiniPlayer's
    // "Switch to Supertonic" button. Nothing above calls this on its own.
    try {
      await useSettingsStore.getState().setTtsProvider("supertonic");
    } catch (error) {
      // setTtsProvider rejects on a failed persist (after recording its own
      // banner message) — surface that here too rather
      // than let it become an unhandled rejection, and stop before resuming
      // playback with a provider switch that may not have stuck.
      // `canSwitchToSupertonic` stays true: this is the button the reader
      // just pressed, and removing it on a transient "database is locked"
      // leaves them stopped on the provider that failed with no way back in
      // the player. Retrying is safe -- `setTtsProvider` applies nothing
      // until its write lands, and those writes are serialized.
      set({ error: displayError(error) });
      return;
    }

    if (useSettingsStore.getState().ttsProvider !== "supertonic") {
      // `setTtsProvider` resolves without applying when a later click
      // supersedes it, so resolving is not the same as "Supertonic is now the
      // provider". Falling through would take this button away and replay the
      // sentence through the engine that just failed. Leave it for the reader
      // to press again.
      return;
    }

    set({ canSwitchToSupertonic: false });
    await speakCurrentSentence(set, get);
  },
}));

async function speakCurrentSentence(
  set: (partial: Partial<PlayerState>) => void,
  get: () => PlayerState,
  options: SpeakOptions = {},
) {
  const state = get();
  const sentence = currentSentence(state);
  if (!sentence) {
    set({
      isPlaying: false,
      error: "No sentence selected.",
      canSwitchToSupertonic: false,
    });
    return;
  }

  let token = options.token;
  if (token === undefined) {
    cancelSpeech();
    token = ++utteranceToken;
    speechAbort = new AbortController();
  } else if (token !== utteranceToken) {
    // A stale auto-advance/continuation request: playback was paused or a newer
    // utterance started while this one was awaiting. Do not resume playback.
    return;
  }
  const position = currentPosition(state);
  const requireInitialBuffer = options.requireInitialBuffer ?? true;

  const { engine, settings } = activeEngine(state.document);
  await speakWithBufferedSpeech(
    engine,
    settings,
    position,
    sentence,
    token,
    set,
    get,
    requireInitialBuffer,
  );
}

/**
 * Translate the current chapter before any audio leaves the speech engine.
 *
 * The backend is the authority on cache/model staleness. A completed call can
 * be instant when the chapter is current; on a real run its progress is owned
 * by the translation store and rendered in Reader. Reloading afterwards is
 * what swaps `sentenceSpeech` from per-index original fallbacks to the newly
 * persisted translated forms while leaving display text and cursor indices
 * untouched.
 */
async function prepareCurrentSectionTranslation(
  set: (partial: Partial<PlayerState>) => void,
  get: () => PlayerState,
) {
  const state = get();
  const section = state.sections[state.currentSectionIndex];
  const targetLanguage = useSettingsStore.getState().translationTargetLang;
  if (
    !section ||
    !targetLanguage ||
    targetLanguage === state.document?.sourceLanguage
  ) {
    return true;
  }

  await useTranslationStore.getState().translateSection(section.id);
  if (useTranslationStore.getState().sectionState.status !== "complete") {
    set({
      isPlaying: false,
      isBuffering: false,
      bufferingMessage: "",
      modelDownload: null,
    });
    return false;
  }

  try {
    const paragraphs = await api.listParagraphs(section.id, targetLanguage);
    if (get().sections[get().currentSectionIndex]?.id !== section.id) {
      return false;
    }
    set({ paragraphs });
    return true;
  } catch (error) {
    set({
      isPlaying: false,
      error: displayError(error),
      canSwitchToSupertonic: false,
    });
    return false;
  }
}

function needsCurrentSectionTranslation(state: PlayerState) {
  const targetLanguage = useSettingsStore.getState().translationTargetLang;
  return Boolean(
    state.sections[state.currentSectionIndex] &&
      targetLanguage &&
      targetLanguage !== state.document?.sourceLanguage,
  );
}

/**
 * Everything about `settings` that changes which engine speaks, or how.
 *
 * Per provider, not a union of all of them: a key folding in every provider's
 * settings unconditionally means saving a Supertonic voice style rebuilds a
 * *Fish* engine and re-keys its whole prefetch buffer, re-buying sentences the
 * reader has already paid Fish for. The Supertonic settings card renders
 * whatever provider is active, so that is a click away, not a corner case.
 *
 * Every setting an engine captures at construction time (not just per-call)
 * must be in its branch, or changing it silently has no effect until the
 * reader switches providers and back. Supertonic captures `language` and
 * `supertonicVoiceStyle` (see supertonicEngine.ts); Fish captures
 * `fishVoiceId` (see fishEngine.ts).
 */
/**
 * Whether these rows are the reader's, or DEFAULT_SETTINGS standing in.
 *
 * `hydrateFailed` alone cannot say: it is false both when the load succeeded
 * and when it has not finished, and only the first is the reader's. A slow
 * `get_all_settings` and an early Play would otherwise build Supertonic from
 * defaults with permission to fetch its model -- the same defaults a *failed*
 * load refuses.
 */
function settingsSource(settings: SettingsStore): SettingsSource {
  if (!settings.hydrated) {
    return "unloaded";
  }
  return settings.hydrateFailed ? "failed" : "loaded";
}

/**
 * Everything about `settings` that changes how a sentence *sounds*.
 *
 * Per provider, not a union of all of them: a key folding in every provider's
 * settings unconditionally means saving a Supertonic voice style rebuilds a
 * *Fish* engine and re-keys its whole prefetch buffer, re-buying sentences the
 * reader has already paid Fish for.
 *
 * This is what `speechCacheKey` is built from, so it must contain nothing
 * that leaves the audio identical -- see `engineKey`.
 */
function voiceKey(settings: SettingsStore): string {
  switch (settings.ttsProvider) {
    case "supertonic":
      return [
        "supertonic",
        settings.supertonicLanguage,
        settings.supertonicVoiceStyle,
      ].join(":");
    case "fish":
      return ["fish", settings.fishVoiceId].join(":");
  }
}

/**
 * When the engine has to be rebuilt: the voice, plus where the settings came
 * from. The engine captures the source -- one built from a guess refuses to
 * fetch Supertonic's model -- and anything captured has to be here, or it
 * goes stale in both directions: an engine cached before a failure keeps
 * permission it should have lost, and one cached during it keeps refusing
 * after a retry that loaded rows identical to the defaults, with no way out
 * but a restart.
 *
 * Deliberately wider than `voiceKey`, and deliberately not what the speech
 * cache is keyed on. The source changes what the engine may *do*, never what
 * it produces, so folding it into the cache key would orphan a whole prefetch
 * buffer the moment a retry succeeded -- re-synthesizing sentences that were
 * already correct.
 */
function engineKey(settings: SettingsStore): string {
  return `${voiceKey(settings)}:${settingsSource(settings)}`;
}

/**
 * Resolve the engine for the current settings, rebuilding it only when the
 * settings that define it change, and pair it with the snapshot it was built
 * from.
 *
 * Pure apart from the memo: the player used to mirror the engine's default
 * voice into its own `voice` field from here, which meant that field was
 * right only at the moments this happened to run. Whoever needs the voice
 * asks the engine for it instead -- it is the only thing that ever knew.
 *
 * Deliberately not gated on `hydrated`, unlike the two settings panels. Those
 * mount on their own routes and can beat `hydrate()`; this runs from the
 * Reader, which `AppShell` only mounts once the reader navigates there from
 * the default library route -- long after the one `get_all_settings` invoke
 * App fires on mount. Anything that made the Reader reachable at launch (a
 * restored session, a deep link) would need this to wait for hydration, or a
 * cold start would speak the first sentence through DEFAULT_SETTINGS: on
 * Supertonic in "M1", whatever the reader had actually chosen.
 *
 * A hydrate *failure* lands in that same state and `hydrated` does not say
 * so, which is what `hydrateFailed` is for. Playback keeps going on the
 * defaults rather than refusing -- silence helps nobody -- and Settings
 * carries the banner and the retry that gets the real rows back.
 */
function activeEngine(document: Domain.Document | null = null): ActiveEngine {
  const storedSettings = useSettingsStore.getState();
  // The global target drives both translation and pronunciation. With
  // Original language selected, pronunciation instead follows this book's
  // editable source language rather than the last translated target saved in
  // `supertonic_language`.
  const spokenLanguage =
    storedSettings.translationTargetLang ??
    document?.sourceLanguage ??
    storedSettings.supertonicLanguage;
  const settings: SettingsStore = {
    ...storedSettings,
    supertonicLanguage:
      spokenLanguage as SettingsStore["supertonicLanguage"],
  };
  const key = engineKey(settings);

  if (cachedEngine?.key !== key) {
    cachedEngine = {
      key,
      active: {
        engine: createSpeechEngine({
          ...settings,
          settingsSource: settingsSource(settings),
        }),
        settings,
      },
    };
  }

  return cachedEngine.active;
}

/**
 * How many sentences to buy ahead of the one being spoken.
 *
 * Asked of the engine rather than read from settings, so the answer always
 * describes the engine actually speaking -- see the rule in CLAUDE.md that a
 * provider decision is made once, in `createSpeechEngine`, and never re-derived.
 */
function lookaheadFor(engine: SpeechEngine) {
  return engine.bills
    ? SPEECH_BILLED_LOOKAHEAD_SENTENCES
    : SPEECH_LOOKAHEAD_SENTENCES;
}

async function speakWithBufferedSpeech(
  engine: SpeechEngine,
  /** The snapshot `engine` was built from — never re-read the store here. */
  settings: SettingsStore,
  position: Position,
  sentence: string,
  token: number,
  set: (partial: Partial<PlayerState>) => void,
  get: () => PlayerState,
  requireInitialBuffer: boolean,
) {
  const label = SPEECH_ENGINE_LABELS[engine.id];
  const lookaheadPositions = speechPositionsFromCurrent(
    get(),
    lookaheadFor(engine),
  );
  set({
    isPlaying: true,
    isBuffering: true,
    bufferingMessage: `Buffering ${label} audio`,
    error: null,
    canSwitchToSupertonic: false,
  });

  try {
    // Readiness first, and from here rather than from whichever sentence
    // happens to miss the cache first. `fillSpeechBuffer` passes no status
    // callback and reaches the engine before the sentence being spoken does,
    // so a model download announced from in there was announced to nobody --
    // and by the time this sentence asked for its audio, the prefetch had
    // already filled the cache, so it never asked the engine at all. That is
    // why the ~383MB fetch in #52 showed no progress of any kind.
    await engine.ensureReady((status) => {
      if (token === utteranceToken && get().isPlaying) {
        // Cleared when the engine stops offering one: a stale cancel would
        // offer to stop something that is no longer running.
        modelDownloadCancel = status.cancel ?? null;
        set({
          bufferingMessage: status.message,
          modelDownload: status.download ?? null,
        });
      }
    });
    if (token !== utteranceToken || !get().isPlaying) {
      return;
    }
    modelDownloadCancel = null;
    set({
      bufferingMessage: `Buffering ${label} audio`,
      modelDownload: null,
    });

    void fillSpeechBuffer(engine, settings, lookaheadPositions, token, get);
    if (requireInitialBuffer) {
      const initialPositions = lookaheadPositions.slice(
        0,
        SPEECH_INITIAL_BUFFER_SENTENCES,
      );
      await fillSpeechBuffer(
        engine,
        settings,
        initialPositions,
        token,
        get,
        (ready) => {
          if (token === utteranceToken && get().isPlaying) {
            set({
              bufferingMessage: `Buffering ${label} audio ${ready}/${initialPositions.length}`,
            });
          }
        },
      );
    }

    // Deliberately not guarded the way `fillSpeechBuffer` is. If a settings
    // save landed while the buffer above was filling, that guard has already
    // stopped the lookahead -- but this sentence still goes through the
    // superseded engine, in the voice the reader just replaced, and is filed
    // under its old key. Bailing instead would stop playback dead with
    // nothing queued to take over, which is worse than one sentence of lag;
    // the new voice takes over at the next `speakCurrentSentence`. The blob is
    // read by the very playback below, so nothing is bought unheard -- only a
    // replay of this one sentence would miss the cache.
    const blob = await cachedSpeechBlob({
      engine,
      position,
      speechText: sentenceSpeechAtPosition(get(), position) ?? sentence,
      state: get(),
      settings,
    });
    if (token !== utteranceToken || !get().isPlaying) {
      return;
    }

    await playGeneratedAudio(blob, token, set, get);
    void fillSpeechBuffer(
      engine,
      settings,
      speechPositionsFromCurrent(get(), lookaheadFor(engine)),
      token,
      get,
    );
  } catch (error) {
    // A cancelled utterance is not a failure worth showing anyone.
    if (token !== utteranceToken || error instanceof SpeechAbortedError) {
      return;
    }
    // The download this belonged to is over, one way or another. A cancel
    // handle left behind would have a later Pause telling Rust to abort a
    // download nobody started.
    modelDownloadCancel = null;

    // Never fall back to another engine here, even implicitly: stop and
    // surface why, and — for Fish — offer the switch as something the
    // reader must click, not something that happens on their behalf. See
    // `canSwitchToSupertonic` and `switchToSupertonic`.
    const isFishFailure = engine.id === "fish";
    set({
      isPlaying: false,
      isBuffering: false,
      bufferingMessage: "",
      modelDownload: null,
      error: isFishFailure ? fishFailureMessage(error) : displayError(error),
      canSwitchToSupertonic: isFishFailure,
    });
  }
}

/**
 * Map a Fish Audio synthesis failure to a message the reader can act on.
 *
 * Kinds come from the Rust backend (`AppErrorKind`); an error without a
 * recognised kind — including the plain `Error`s `fishEngine.ensureReady`
 * throws locally for a missing key/voice — falls through to its own message,
 * which is already written for a reader (see `fishEngine.ts`).
 */
function fishFailureMessage(error: unknown): string {
  const appError = asAppError(error);
  switch (appError?.kind) {
    case "auth":
      return "Fish Audio rejected your API key.";
    case "payment_required":
      return "Your Fish Audio account is out of credit.";
    case "rate_limited":
      return "Fish Audio is rate limiting requests.";
    case "voice":
      return "Choose a Fish Audio voice in Settings.";
    default:
      return displayError(error);
  }
}

async function cachedSpeechBlob({
  engine,
  position,
  speechText,
  state,
  settings,
}: {
  engine: SpeechEngine;
  position: Position;
  /** Already in spoken form — see Paragraph.sentenceSpeech. */
  speechText: string;
  state: PlayerState;
  settings: SettingsStore;
}) {
  const key = speechCacheKey(position, speechText, state, settings);
  const cached = speechCache.get(key);
  if (cached) {
    cached.lastUsed = Date.now();
    return cached.promise;
  }

  const promise = (async () => {
    // Cheap once ready, and by here it always is: `speakWithBufferedSpeech`
    // awaits `ensureReady` before any of this runs. Kept as the belt on the
    // prefetch path, which can outlive the utterance that started it.
    await engine.ensureReady();
    return engine.synthesize(
      { text: speechText, speed: state.speed },
      speechAbort?.signal,
    );
  })().catch((error) => {
    speechCache.delete(key);
    throw error;
  });

  speechCache.set(key, { promise, lastUsed: Date.now() });
  trimSpeechCache();
  return promise;
}

async function fillSpeechBuffer(
  engine: SpeechEngine,
  /** See `speakWithBufferedSpeech` — this must be `engine`'s own snapshot. */
  settings: SettingsStore,
  positions: Position[],
  token: number,
  get: () => PlayerState,
  onReady?: (ready: number) => void,
) {
  if (positions.length === 0) {
    return;
  }

  // What this engine was built for. Compared against the live store below to
  // decide whether to keep going -- never to build a key. Keys come from
  // `settings` and only from `settings`; that is what keeps an engine's audio
  // from being filed under another engine's name.
  const builtFor = engineKey(settings);
  let ready = 0;
  let cursor = 0;

  async function worker() {
    while (cursor < positions.length) {
      if (token !== utteranceToken || !get().isPlaying) {
        return;
      }
      // The settings moved on, so this engine's output is already unreachable
      // -- every sentence it files lands under `builtFor`, which nothing will
      // read again, while the engine that replaced it re-synthesizes the same
      // positions. Fish bills for both. The token guard above cannot catch
      // this: auto-advance reuses the token, so nothing bumps it.
      if (engineKey(useSettingsStore.getState()) !== builtFor) {
        return;
      }

      const position = positions[cursor];
      cursor += 1;
      const state = get();
      const speechText = sentenceSpeechAtPosition(state, position);
      if (!speechText) {
        continue;
      }

      try {
        await cachedSpeechBlob({
          engine,
          position,
          speechText,
          state,
          settings,
        });
        ready += 1;
        onReady?.(ready);
      } catch {
        // Skip this sentence, do not abandon the rest. Returning here ended
        // the whole read-ahead on the first failure, so every sentence past a
        // single bad one went unbuffered and playback fell back to
        // synthesizing each as it reached it.
        //
        // Nothing is reported from here on purpose. This is a cache warmer:
        // when playback actually reaches the sentence, `speakWithBufferedSpeech`
        // synthesizes it again on the playing path, and *its* catch is what
        // stops playback and shows the reason. Surfacing a prefetch failure
        // would stop playback that recovers on its own.
        //
        // `cursor` was already advanced above, so this moves on rather than
        // retrying the sentence that just failed.
        continue;
      }
    }
  }

  await Promise.all(
    Array.from(
      { length: Math.min(SPEECH_PREFETCH_CONCURRENCY, positions.length) },
      () => worker(),
    ),
  );
}

function speechCacheKey(
  position: Position,
  text: string,
  state: PlayerState,
  settings: SettingsStore,
) {
  const section = state.sections[position.sectionIndex];
  const paragraph = state.paragraphs[position.paragraphIndex];
  // The engine's own settings, via `voiceKey` -- the half of `engineKey` that
  // decides how a sentence sounds -- so the key and the engine can never
  // disagree about what makes audio stale, and one provider's settings never
  // invalidate another's buffered audio.
  //
  // Deliberately NOT `state.voice`. Both engines take their voice from
  // settings, so `engineKey` already carries it; adding the field too meant
  // `activeEngine` rewriting it on a provider swap orphaned every entry the
  // previous provider had already been billed for, and switching back bought
  // them again.
  return JSON.stringify({
    engine: voiceKey(settings),
    documentId: state.document?.id ?? "",
    sectionId: section?.id ?? "",
    paragraphId: paragraph?.id ?? "",
    sentenceIndex: position.sentenceIndex,
    text,
    speed: state.speed,
  });
}

function trimSpeechCache() {
  while (speechCache.size > SPEECH_CACHE_LIMIT) {
    const oldest = [...speechCache.entries()].sort(
      (first, second) => first[1].lastUsed - second[1].lastUsed,
    )[0]?.[0];
    if (!oldest) {
      return;
    }
    speechCache.delete(oldest);
  }
}

async function playGeneratedAudio(
  blob: Blob,
  token: number,
  set: (partial: Partial<PlayerState>) => void,
  get: () => PlayerState,
) {
  clearGeneratedAudio();
  activeAudioUrl = URL.createObjectURL(blob);
  activeAudio = new Audio(activeAudioUrl);
  activeAudio.onended = () => {
    if (token !== utteranceToken || !get().isPlaying) {
      clearGeneratedAudio();
      return;
    }
    clearGeneratedAudio();
    void advanceBySentence(set, get, 1, true, token);
  };
  activeAudio.onerror = () => {
    if (token !== utteranceToken) {
      clearGeneratedAudio();
      return;
    }
    clearGeneratedAudio();
    set({
      isPlaying: false,
      isBuffering: false,
      bufferingMessage: "",
      modelDownload: null,
      error: "Audio playback failed.",
      canSwitchToSupertonic: false,
    });
  };

  set({
    isPlaying: true,
    isBuffering: false,
    bufferingMessage: "",
    modelDownload: null,
    error: null,
    canSwitchToSupertonic: false,
  });

  try {
    await activeAudio.play();
    await persistPlaybackState(set, get());
  } catch (error) {
    if (token !== utteranceToken) {
      return;
    }
    clearGeneratedAudio();
    set({
      isPlaying: false,
      isBuffering: false,
      bufferingMessage: "",
      modelDownload: null,
      error: displayError(error),
      canSwitchToSupertonic: false,
    });
  }
}

async function advanceBySentence(
  set: (partial: Partial<PlayerState>) => void,
  get: () => PlayerState,
  direction: 1 | -1,
  fromAutoAdvance = false,
  token?: number,
) {
  // Reject a stale auto-advance before it mutates the reader position: playback
  // was paused or a newer utterance started while this continuation awaited.
  if (fromAutoAdvance && token !== undefined && token !== utteranceToken) {
    return;
  }
  const wasPlaying = get().isPlaying || fromAutoAdvance;
  const previousSectionIndex = get().currentSectionIndex;
  const next = direction > 0 ? nextPosition(get()) : previousPosition(get());
  if (!next) {
    cancelSpeech();
    set({
      isPlaying: false,
      isBuffering: false,
      bufferingMessage: "",
      modelDownload: null,
    });
    return;
  }

  await moveToPosition(set, get, next, { preservePlayback: fromAutoAdvance });
  if (wasPlaying) {
    if (
      get().currentSectionIndex !== previousSectionIndex &&
      !(await prepareCurrentSectionTranslation(set, get))
    ) {
      return;
    }
    await speakCurrentSentence(set, get, {
      token: fromAutoAdvance ? token : undefined,
      requireInitialBuffer: !fromAutoAdvance,
    });
  }
}

async function advanceByParagraph(
  set: (partial: Partial<PlayerState>) => void,
  get: () => PlayerState,
  direction: 1 | -1,
) {
  const state = get();
  const wasPlaying = state.isPlaying;
  const previousSectionIndex = state.currentSectionIndex;
  const position =
    direction > 0
      ? nextParagraphPosition(state)
      : previousParagraphPosition(state);
  if (!position) {
    return;
  }

  await moveToPosition(set, get, position);
  if (wasPlaying) {
    if (
      get().currentSectionIndex !== previousSectionIndex &&
      !(await prepareCurrentSectionTranslation(set, get))
    ) {
      return;
    }
    await speakCurrentSentence(set, get);
  }
}

async function moveToPosition(
  set: (partial: Partial<PlayerState>) => void,
  get: () => PlayerState,
  position: Position,
  options: { preservePlayback?: boolean } = {},
) {
  // Claim the latest navigation slot so a slower section load started here
  // cannot overwrite reader state after a newer navigation or reset wins.
  const requestId = ++navToken;
  if (!options.preservePlayback) {
    cancelSpeech();
    set({
      isBuffering: true,
      bufferingMessage: "Loading section",
      isPlaying: false,
      error: null,
      canSwitchToSupertonic: false,
    });
  }

  try {
    let paragraphs = get().paragraphs;
    let sectionImages = get().sectionImages;
    if (position.sectionIndex !== get().currentSectionIndex) {
      const section = get().sections[position.sectionIndex];
      if (!section) {
        // Reset the buffering UI instead of leaving it stuck on "Loading section".
        set({ isBuffering: false, bufferingMessage: "", modelDownload: null });
        return;
      }
      if (options.preservePlayback) {
        set({
          isBuffering: true,
          bufferingMessage: "Loading section",
          error: null,
          canSwitchToSupertonic: false,
        });
      }
      [paragraphs, sectionImages] = await Promise.all([
        api.listParagraphs(
          section.id,
          useSettingsStore.getState().translationTargetLang,
        ),
        api.listSectionImages(section.id),
      ]);
      if (requestId !== navToken) {
        return;
      }
    }

    const paragraphIndex =
      position.paragraphIndex === LAST_IN_SECTION
        ? lastReadableParagraphIndex(paragraphs)
        : clamp(position.paragraphIndex, 0, Math.max(0, paragraphs.length - 1));
    const sentenceIndex = clamp(
      position.sentenceIndex,
      0,
      Math.max(0, sentenceCount(paragraphs[paragraphIndex]) - 1),
    );

    set({
      paragraphs,
      sectionImages,
      currentSectionIndex: position.sectionIndex,
      currentParagraphIndex: paragraphIndex,
      currentSentenceIndex: sentenceIndex,
      isBuffering: false,
      bufferingMessage: "",
      modelDownload: null,
    });
    await persistPlaybackState(set, get());
  } catch (error) {
    // Ignore failures from a navigation that a newer one (or reset) superseded.
    if (requestId !== navToken) {
      return;
    }
    set({
      isBuffering: false,
      bufferingMessage: "",
      modelDownload: null,
      error: displayError(error),
      canSwitchToSupertonic: false,
    });
  }
}

function currentSentence(state: PlayerState) {
  return sentenceAtPosition(state, currentPosition(state));
}

function currentPosition(state: PlayerState): Position {
  return {
    sectionIndex: state.currentSectionIndex,
    paragraphIndex: state.currentParagraphIndex,
    sentenceIndex: state.currentSentenceIndex,
  };
}

function sentenceAtPosition(state: PlayerState, position: Position) {
  const paragraph = paragraphAtPosition(state, position);
  return paragraph ? sentenceText(paragraph, position.sentenceIndex) : null;
}

/**
 * The spoken form of a sentence, as computed by the backend.
 *
 * Falls back to the display text when a paragraph predates this field — the
 * reader still hears the sentence, just with notation read literally.
 */
function sentenceSpeechAtPosition(state: PlayerState, position: Position) {
  const paragraph = paragraphAtPosition(state, position);
  if (!paragraph) {
    return null;
  }

  return (
    paragraph.sentenceSpeech?.[position.sentenceIndex] ??
    sentenceText(paragraph, position.sentenceIndex)
  );
}

function paragraphAtPosition(state: PlayerState, position: Position) {
  if (position.sectionIndex !== state.currentSectionIndex) {
    return null;
  }

  return state.paragraphs[position.paragraphIndex] ?? null;
}

export function sentenceText(
  paragraph: Domain.Paragraph,
  sentenceIndex: number,
) {
  const offsets = paragraph.sentenceOffsets[sentenceIndex];
  if (!offsets) {
    return paragraph.text.trim();
  }

  return paragraph.text
    .slice(
      utf8ByteOffsetToStringIndex(paragraph.text, offsets[0]),
      utf8ByteOffsetToStringIndex(paragraph.text, offsets[1]),
    )
    .trim();
}

function utf8ByteOffsetToStringIndex(text: string, byteOffset: number) {
  if (byteOffset <= 0) {
    return 0;
  }

  let bytes = 0;
  for (let index = 0; index < text.length; ) {
    if (bytes >= byteOffset) {
      return index;
    }

    const codePoint = text.codePointAt(index) ?? 0;
    bytes += utf8CodePointByteLength(codePoint);
    index += codePoint > 0xffff ? 2 : 1;
  }

  return text.length;
}

function utf8CodePointByteLength(codePoint: number) {
  if (codePoint <= 0x7f) {
    return 1;
  }
  if (codePoint <= 0x7ff) {
    return 2;
  }
  if (codePoint <= 0xffff) {
    return 3;
  }
  return 4;
}

function speechPositionsFromCurrent(state: PlayerState, count: number) {
  const positions: Position[] = [currentPosition(state)];
  let position: Position | null = currentPosition(state);

  while (positions.length < count) {
    position = nextPositionInLoadedSection(state, position);
    if (!position) {
      break;
    }
    positions.push(position);
  }

  return positions;
}

function nextPositionInLoadedSection(
  state: PlayerState,
  position: Position,
): Position | null {
  if (position.sectionIndex !== state.currentSectionIndex) {
    return null;
  }

  const paragraph = state.paragraphs[position.paragraphIndex];
  if (!paragraph) {
    return null;
  }

  if (position.sentenceIndex + 1 < sentenceCount(paragraph)) {
    return {
      sectionIndex: position.sectionIndex,
      paragraphIndex: position.paragraphIndex,
      sentenceIndex: position.sentenceIndex + 1,
    };
  }

  if (position.paragraphIndex + 1 < state.paragraphs.length) {
    return {
      sectionIndex: position.sectionIndex,
      paragraphIndex: position.paragraphIndex + 1,
      sentenceIndex: 0,
    };
  }

  return null;
}

function nextPosition(state: PlayerState): Position | null {
  const paragraph = state.paragraphs[state.currentParagraphIndex];
  if (!paragraph) {
    return null;
  }

  if (state.currentSentenceIndex + 1 < sentenceCount(paragraph)) {
    return {
      sectionIndex: state.currentSectionIndex,
      paragraphIndex: state.currentParagraphIndex,
      sentenceIndex: state.currentSentenceIndex + 1,
    };
  }

  return nextParagraphPosition(state);
}

function previousPosition(state: PlayerState): Position | null {
  if (state.currentSentenceIndex > 0) {
    return {
      sectionIndex: state.currentSectionIndex,
      paragraphIndex: state.currentParagraphIndex,
      sentenceIndex: state.currentSentenceIndex - 1,
    };
  }

  const previousParagraph = previousParagraphPosition(state);
  if (!previousParagraph) {
    return null;
  }

  // Same section: we know the paragraph, so resolve its last sentence now.
  // Different section: defer to moveToPosition's clamp, which resolves
  // LAST_IN_SECTION to the last sentence of the (now loaded) last paragraph.
  if (previousParagraph.sectionIndex === state.currentSectionIndex) {
    const paragraph = state.paragraphs[previousParagraph.paragraphIndex];
    return {
      ...previousParagraph,
      sentenceIndex: paragraph ? Math.max(0, sentenceCount(paragraph) - 1) : 0,
    };
  }

  return { ...previousParagraph, sentenceIndex: LAST_IN_SECTION };
}

function nextParagraphPosition(state: PlayerState): Position | null {
  if (state.currentParagraphIndex + 1 < state.paragraphs.length) {
    return {
      sectionIndex: state.currentSectionIndex,
      paragraphIndex: state.currentParagraphIndex + 1,
      sentenceIndex: 0,
    };
  }

  if (state.currentSectionIndex + 1 < state.sections.length) {
    return {
      sectionIndex: state.currentSectionIndex + 1,
      paragraphIndex: 0,
      sentenceIndex: 0,
    };
  }

  return null;
}

function previousParagraphPosition(state: PlayerState): Position | null {
  if (state.currentParagraphIndex > 0) {
    return {
      sectionIndex: state.currentSectionIndex,
      paragraphIndex: state.currentParagraphIndex - 1,
      sentenceIndex: 0,
    };
  }

  if (state.currentSectionIndex > 0) {
    // Going back a paragraph from the first paragraph lands on the LAST
    // paragraph of the previous section (clamped once that section loads).
    return {
      sectionIndex: state.currentSectionIndex - 1,
      paragraphIndex: LAST_IN_SECTION,
      sentenceIndex: 0,
    };
  }

  return null;
}

function sentenceCount(paragraph: Domain.Paragraph | undefined) {
  if (!paragraph) {
    return 0;
  }

  return Math.max(1, paragraph.sentenceOffsets.length);
}

/**
 * Where in a section's paragraphs a saved cursor points.
 *
 * Identity, not ordinal: a re-import renumbers nothing but replaces every id,
 * so a paragraph that is no longer there is a cursor that no longer means
 * anything. That case opens the section at its first readable paragraph rather
 * than at a position the reader never chose.
 */
function resumePosition(
  saved: Domain.PlaybackState | null,
  section: Domain.Section,
  paragraphs: Domain.Paragraph[],
) {
  const paragraphIndex =
    saved && saved.sectionId === section.id
      ? paragraphs.findIndex((paragraph) => paragraph.id === saved.paragraphId)
      : -1;

  if (paragraphIndex < 0) {
    return {
      paragraphIndex: firstReadableParagraphIndex(paragraphs),
      sentenceIndex: 0,
    };
  }

  return {
    paragraphIndex,
    sentenceIndex: clamp(
      saved?.sentenceIndex ?? 0,
      0,
      Math.max(0, sentenceCount(paragraphs[paragraphIndex]) - 1),
    ),
  };
}

function firstReadableParagraphIndex(paragraphs: Domain.Paragraph[]) {
  return Math.max(
    0,
    paragraphs.findIndex((paragraph) => paragraph.text.trim().length > 0),
  );
}

function lastReadableParagraphIndex(paragraphs: Domain.Paragraph[]) {
  for (let index = paragraphs.length - 1; index >= 0; index -= 1) {
    if (paragraphs[index].text.trim().length > 0) {
      return index;
    }
  }
  return Math.max(0, paragraphs.length - 1);
}

async function persistPlaybackState(
  set: (partial: Partial<PlayerState>) => void,
  state: PlayerState,
) {
  const document = state.document;
  const section = state.sections[state.currentSectionIndex];
  const paragraph = state.paragraphs[state.currentParagraphIndex];
  if (!document || !section || !paragraph) {
    return;
  }

  try {
    await api.savePlaybackState({
      documentId: document.id,
      sectionId: section.id,
      paragraphId: paragraph.id,
      sentenceIndex: state.currentSentenceIndex,
      sentenceOffsetMs: 0,
      // The voice the settings select as of this write, asked for at write
      // time rather than mirrored into player state: this runs on load and on
      // every seek, not only while speaking, so any cached copy is right only
      // at the moments something happens to refresh it. Not necessarily the
      // voice this sentence was synthesized in -- a style saved while it was
      // buffering lands here first. Empty for Fish with no voice configured,
      // which is what `createFishEngine` reports and is truer than naming a
      // voice from the other provider. Building an engine is plain object
      // construction; nothing downloads or synthesizes until `ensureReady`.
      voiceId: activeEngine(document).engine.defaultVoice,
      speed: state.speed,
      updatedAt: new Date().toISOString(),
    });
    set({ positionError: null });
  } catch (error) {
    set({
      positionError: `Your place in this book is not being saved. (${displayError(error)})`,
    });
  }
}

/**
 * The saved cursor, and whatever went wrong reading it.
 *
 * A failed read is not a failed open. Left to reach `loadDocument`'s catch it
 * refused to open the document at all, which turns "we lost your place" into
 * "you cannot read this book".
 */
async function readPlaybackState(documentId: string) {
  try {
    return { state: await api.getPlaybackState(documentId), failure: null };
  } catch (error) {
    return {
      state: null,
      failure: `Could not restore where you left off, so this book opened at the beginning. (${displayError(error)})`,
    };
  }
}

/**
 * Stop caring about the current utterance, and tell the engine so.
 *
 * The token is what makes late results harmless; the abort is what lets an
 * engine skip work it has not started. Neither can stop synthesis already in
 * flight — see SpeechEngine.
 */
/**
 * Ask the engine to stop whatever large thing it is fetching.
 *
 * Fire-and-forget: the request only has to be delivered. The download itself
 * fails a moment later, inside the `ensureReady` the play chain is still
 * awaiting, as an abort -- which `speakWithBufferedSpeech` drops in silence.
 */
function stopModelDownload() {
  const cancel = modelDownloadCancel;
  modelDownloadCancel = null;
  void cancel?.();
}

function cancelSpeech() {
  utteranceToken += 1;
  speechAbort?.abort();
  // The aborted controller deliberately stays. Clearing it to null made
  // `speechAbort?.signal` undefined for anything issued afterwards, so a
  // request that started after Pause carried no signal at all and ran to
  // completion -- billed in full, for audio nobody was waiting for. `play`
  // installs a fresh controller when playback actually resumes.
  clearGeneratedAudio();
}

function clearGeneratedAudio() {
  if (activeAudio) {
    const audio = activeAudio;
    activeAudio = null;
    audio.onended = null;
    audio.onerror = null;
    audio.pause();
    audio.removeAttribute("src");
    audio.load();
  }
  if (activeAudioUrl) {
    URL.revokeObjectURL(activeAudioUrl);
    activeAudioUrl = null;
  }
}

function clamp(value: number, min: number, max: number) {
  if (max < min) {
    return min;
  }
  return Math.min(max, Math.max(min, value));
}
