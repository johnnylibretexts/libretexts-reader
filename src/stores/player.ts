import { create } from "zustand";
import { api } from "../lib/tauri";
import { asAppError, displayError } from "../lib/errors";
import {
  createSpeechEngine,
  SPEECH_ENGINE_LABELS,
  SpeechAbortedError,
  type SpeechEngine,
} from "../lib/speech";
import { useSettingsStore } from "./settings";
import type { SettingsState } from "./settings";
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
  voice: string;
  speed: number;
  isPlaying: boolean;
  isBuffering: boolean;
  bufferingMessage: string;
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
  loadDocument: (documentId: string) => Promise<void>;
  play: () => Promise<void>;
  pause: () => void;
  reset: () => void;
  seekToSentence: (
    paragraphIndex: number,
    sentenceIndex: number,
  ) => Promise<void>;
  setSection: (sectionIndex: number) => Promise<void>;
  skipForward: () => Promise<void>;
  skipBack: () => Promise<void>;
  skipParagraphForward: () => Promise<void>;
  skipParagraphBack: () => Promise<void>;
  setVoice: (voiceId: string) => void;
  setSpeed: (speed: number) => void;
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
const SPEECH_PREFETCH_CONCURRENCY = 2;
const SPEECH_CACHE_LIMIT = 32;
// Sentinel paragraph/sentence index meaning "the last one in the section".
// moveToPosition clamps it to the real last index once the section's paragraphs
// load, so backward navigation into another section lands at its end.
const LAST_IN_SECTION = Number.MAX_SAFE_INTEGER;

/**
 * The engine in use, rebuilt only when the settings that define it change.
 * Holding it here rather than passing it around keeps every caller below from
 * having to know which engine is active — that decision happens once, in
 * `activeEngine`, and nowhere else in this file.
 */
let cachedEngine: { key: string; engine: SpeechEngine } | null = null;

/** Aborts in-flight synthesis for the current utterance. See SpeechEngine. */
let speechAbort: AbortController | null = null;

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
  voice: "M1",
  speed: 1,
  isPlaying: false,
  isBuffering: false,
  bufferingMessage: "",
  loading: false,
  error: null,
  canSwitchToSupertonic: false,

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
    });

    try {
      const document = await api.getDocument(documentId);
      const sections = await api.listSections(documentId);
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

      const [paragraphs, sectionImages] = await Promise.all([
        api.listParagraphs(sections[0].id),
        api.listSectionImages(sections[0].id),
      ]);
      if (requestId !== navToken) {
        return;
      }
      set({
        document,
        sections,
        paragraphs,
        sectionImages,
        currentSectionIndex: 0,
        currentParagraphIndex: firstReadableParagraphIndex(paragraphs),
        currentSentenceIndex: 0,
        loading: false,
        error: null,
        canSwitchToSupertonic: false,
      });
      await persistPlaybackState(get());
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
    await speakCurrentSentence(set, get);
  },

  pause: () => {
    cancelSpeech();
    set({ isPlaying: false, isBuffering: false, bufferingMessage: "" });
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
      loading: false,
      error: null,
      canSwitchToSupertonic: false,
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
      await speakCurrentSentence(set, get);
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

  setVoice: (voiceId: string) => {
    set({ voice: voiceId });
  },

  setSpeed: (speed: number) => {
    set({ speed });
  },

  switchToSupertonic: async () => {
    // This is the only place `ttsProvider` may change as a consequence of a
    // Fish failure, and it only runs from here — a click on the MiniPlayer's
    // "Switch to Supertonic" button. Nothing above calls this on its own.
    try {
      await useSettingsStore.getState().setTtsProvider("supertonic");
    } catch (error) {
      // setTtsProvider/saveTtsSettings reject on a failed persist (after
      // recording their own banner message) — surface that here too rather
      // than let it become an unhandled rejection, and stop before resuming
      // playback with a provider switch that may not have stuck.
      set({ error: displayError(error), canSwitchToSupertonic: false });
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

  await speakWithBufferedSpeech(
    activeEngine(set),
    position,
    sentence,
    token,
    set,
    get,
    requireInitialBuffer,
  );
}

/**
 * Resolve the engine for the current settings, rebuilding it only when those
 * settings change.
 *
 * A voice id means something only to the engine that offered it, so switching
 * engines resets the reader's voice to the new engine's default rather than
 * handing it an id it cannot interpret.
 */
function activeEngine(set: (partial: Partial<PlayerState>) => void) {
  const settings = useSettingsStore.getState();
  // Every setting an engine captures at construction time (not just per-call)
  // must be in this key, or changing it silently has no effect until the
  // reader switches providers and back. Supertonic only captures `language`
  // (its voice is passed per-synthesize call, see supertonicEngine.ts); Fish
  // captures `fishVoiceId` (see fishEngine.ts).
  const key = [
    settings.ttsProvider,
    settings.supertonicLanguage,
    settings.fishVoiceId,
  ].join(":");

  if (cachedEngine?.key !== key) {
    const previous = cachedEngine?.engine;
    const engine = createSpeechEngine(settings);
    cachedEngine = { key, engine };
    if (previous && previous.id !== engine.id) {
      set({ voice: engine.defaultVoice });
    }
  }

  return cachedEngine.engine;
}

async function speakWithBufferedSpeech(
  engine: SpeechEngine,
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
    SPEECH_LOOKAHEAD_SENTENCES,
  );
  set({
    isPlaying: true,
    isBuffering: true,
    bufferingMessage: `Buffering ${label} audio`,
    error: null,
    canSwitchToSupertonic: false,
  });

  try {
    void fillSpeechBuffer(engine, lookaheadPositions, token, get);
    if (requireInitialBuffer) {
      const initialPositions = lookaheadPositions.slice(
        0,
        SPEECH_INITIAL_BUFFER_SENTENCES,
      );
      await fillSpeechBuffer(
        engine,
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

    const blob = await cachedSpeechBlob({
      engine,
      position,
      speechText: sentenceSpeechAtPosition(get(), position) ?? sentence,
      state: get(),
      settings: useSettingsStore.getState(),
      onStatus: (bufferingMessage) => {
        if (token === utteranceToken && get().isPlaying) {
          set({ bufferingMessage });
        }
      },
    });
    if (token !== utteranceToken || !get().isPlaying) {
      return;
    }

    await playGeneratedAudio(blob, token, set, get);
    void fillSpeechBuffer(
      engine,
      speechPositionsFromCurrent(get(), SPEECH_LOOKAHEAD_SENTENCES),
      token,
      get,
    );
  } catch (error) {
    // A cancelled utterance is not a failure worth showing anyone.
    if (token !== utteranceToken || error instanceof SpeechAbortedError) {
      return;
    }
    // Never fall back to another engine here, even implicitly: stop and
    // surface why, and — for Fish — offer the switch as something the
    // reader must click, not something that happens on their behalf. See
    // `canSwitchToSupertonic` and `switchToSupertonic`.
    const isFishFailure = engine.id === "fish";
    set({
      isPlaying: false,
      isBuffering: false,
      bufferingMessage: "",
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
  onStatus,
}: {
  engine: SpeechEngine;
  position: Position;
  /** Already in spoken form — see Paragraph.sentenceSpeech. */
  speechText: string;
  state: PlayerState;
  settings: SettingsState;
  onStatus?: (status: string) => void;
}) {
  const key = speechCacheKey(engine, position, speechText, state, settings);
  const cached = speechCache.get(key);
  if (cached) {
    cached.lastUsed = Date.now();
    return cached.promise;
  }

  const promise = (async () => {
    // Cold engines report progress while they load; synthesis itself does not.
    await engine.ensureReady(onStatus);
    return engine.synthesize(
      { text: speechText, voice: state.voice, speed: state.speed },
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
  positions: Position[],
  token: number,
  get: () => PlayerState,
  onReady?: (ready: number) => void,
) {
  if (positions.length === 0) {
    return;
  }

  const settings = useSettingsStore.getState();
  let ready = 0;
  let cursor = 0;

  async function worker() {
    while (cursor < positions.length) {
      if (token !== utteranceToken || !get().isPlaying) {
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
        return;
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
  engine: SpeechEngine,
  position: Position,
  text: string,
  state: PlayerState,
  settings: SettingsState,
) {
  const section = state.sections[position.sectionIndex];
  const paragraph = state.paragraphs[position.paragraphIndex];
  // Engine-specific settings are folded in unconditionally rather than per
  // engine: a superset key costs an occasional extra synthesis after a settings
  // change, where a too-narrow one would serve audio in the wrong voice.
  return JSON.stringify({
    engine: engine.id,
    documentId: state.document?.id ?? "",
    sectionId: section?.id ?? "",
    paragraphId: paragraph?.id ?? "",
    sentenceIndex: position.sentenceIndex,
    text,
    voice: state.voice,
    speed: state.speed,
    supertonicLanguage: settings.supertonicLanguage,
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
      error: "Audio playback failed.",
      canSwitchToSupertonic: false,
    });
  };

  set({
    isPlaying: true,
    isBuffering: false,
    bufferingMessage: "",
    error: null,
    canSwitchToSupertonic: false,
  });

  try {
    await activeAudio.play();
    await persistPlaybackState(get());
  } catch (error) {
    if (token !== utteranceToken) {
      return;
    }
    clearGeneratedAudio();
    set({
      isPlaying: false,
      isBuffering: false,
      bufferingMessage: "",
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
  const next = direction > 0 ? nextPosition(get()) : previousPosition(get());
  if (!next) {
    cancelSpeech();
    set({
      isPlaying: false,
      isBuffering: false,
      bufferingMessage: "",
    });
    return;
  }

  await moveToPosition(set, get, next, { preservePlayback: fromAutoAdvance });
  if (wasPlaying) {
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
  const position =
    direction > 0
      ? nextParagraphPosition(state)
      : previousParagraphPosition(state);
  if (!position) {
    return;
  }

  await moveToPosition(set, get, position);
  if (wasPlaying) {
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
        set({ isBuffering: false, bufferingMessage: "" });
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
        api.listParagraphs(section.id),
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
    });
    await persistPlaybackState(get());
  } catch (error) {
    // Ignore failures from a navigation that a newer one (or reset) superseded.
    if (requestId !== navToken) {
      return;
    }
    set({
      isBuffering: false,
      bufferingMessage: "",
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

async function persistPlaybackState(state: PlayerState) {
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
      voiceId: state.voice,
      speed: state.speed,
      updatedAt: new Date().toISOString(),
    });
  } catch {
    // Playback persistence is completed in a later backend task; playback should still work.
  }
}

/**
 * Stop caring about the current utterance, and tell the engine so.
 *
 * The token is what makes late results harmless; the abort is what lets an
 * engine skip work it has not started. Neither can stop synthesis already in
 * flight — see SpeechEngine.
 */
function cancelSpeech() {
  utteranceToken += 1;
  speechAbort?.abort();
  speechAbort = null;
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
