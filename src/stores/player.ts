import { create } from "zustand";
import { synthesizeKokoroSpeech } from "../lib/kokoro";
import { mathContentToSpeech } from "../lib/mathContent";
import { api } from "../lib/tauri";
import { displayError } from "../lib/errors";
import { useSettingsStore } from "./settings";
import type { SettingsState, TtsProvider } from "./settings";
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
  activeSentenceText: string;
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
}

let utteranceToken = 0;
let activeAudio: HTMLAudioElement | null = null;
let activeAudioUrl: string | null = null;
const SPEECH_INITIAL_BUFFER_SENTENCES = 3;
const SPEECH_LOOKAHEAD_SENTENCES = 10;
const SPEECH_PREFETCH_CONCURRENCY = 2;
const SPEECH_CACHE_LIMIT = 32;

type NeuralTtsProvider = Extract<TtsProvider, "kokoro" | "supertonic">;

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
  voice: "af_heart",
  speed: 1,
  isPlaying: false,
  isBuffering: false,
  bufferingMessage: "",
  loading: false,
  error: null,
  activeSentenceText: "",

  loadDocument: async (documentId: string) => {
    cancelSpeech();
    set({
      loading: true,
      error: null,
      isPlaying: false,
      isBuffering: false,
      bufferingMessage: "",
      activeSentenceText: "",
    });

    try {
      const document = await api.getDocument(documentId);
      const sections = await api.listSections(documentId);
      if (sections.length === 0) {
        set({
          document,
          sections: [],
          paragraphs: [],
          sectionImages: [],
          loading: false,
          error: "This document has no readable sections.",
        });
        return;
      }

      const [paragraphs, sectionImages] = await Promise.all([
        api.listParagraphs(sections[0].id),
        api.listSectionImages(sections[0].id),
      ]);
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
      });
      await persistPlaybackState(get());
    } catch (error) {
      set({
        loading: false,
        error: error instanceof Error ? error.message : String(error),
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
      activeSentenceText: "",
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
      activeSentenceText: "",
      error: "No sentence selected.",
    });
    return;
  }

  let token = options.token;
  if (token === undefined) {
    cancelSpeech();
    token = ++utteranceToken;
  }
  const settings = useSettingsStore.getState();
  const position = currentPosition(state);
  const requireInitialBuffer = options.requireInitialBuffer ?? true;

  if (settings.ttsProvider === "kokoro") {
    await speakWithKokoro(
      position,
      sentence,
      token,
      set,
      get,
      requireInitialBuffer,
    );
    return;
  }

  if (settings.ttsProvider === "supertonic") {
    await speakWithSupertonic(
      position,
      sentence,
      token,
      set,
      get,
      requireInitialBuffer,
    );
    return;
  }

  speakWithSystemVoice(sentence, token, set, get);
}

function speakWithSystemVoice(
  sentence: string,
  token: number,
  set: (partial: Partial<PlayerState>) => void,
  get: () => PlayerState,
) {
  if (
    !("speechSynthesis" in window) ||
    !("SpeechSynthesisUtterance" in window)
  ) {
    set({
      isPlaying: false,
      isBuffering: false,
      bufferingMessage: "",
      error: "Speech playback is unavailable in this webview.",
    });
    return;
  }

  const utterance = new SpeechSynthesisUtterance(mathContentToSpeech(sentence));
  utterance.rate = clamp(get().speed, 0.5, 2);
  utterance.voice = chooseSystemVoice(get().voice);
  utterance.onend = () => {
    if (token !== utteranceToken || !get().isPlaying) {
      return;
    }
    void advanceBySentence(set, get, 1, true, token);
  };
  utterance.onerror = (event) => {
    if (token !== utteranceToken) {
      return;
    }
    set({
      isPlaying: false,
      isBuffering: false,
      error: event.error ? `Playback error: ${event.error}` : "Playback error.",
    });
  };

  set({
    isPlaying: true,
    isBuffering: false,
    bufferingMessage: "",
    error: null,
    activeSentenceText: sentence,
  });
  window.speechSynthesis.speak(utterance);
  void persistPlaybackState(get());
}

async function speakWithKokoro(
  position: Position,
  sentence: string,
  token: number,
  set: (partial: Partial<PlayerState>) => void,
  get: () => PlayerState,
  requireInitialBuffer: boolean,
) {
  await speakWithBufferedSpeech(
    "kokoro",
    position,
    sentence,
    token,
    set,
    get,
    requireInitialBuffer,
  );
}

async function speakWithSupertonic(
  position: Position,
  sentence: string,
  token: number,
  set: (partial: Partial<PlayerState>) => void,
  get: () => PlayerState,
  requireInitialBuffer: boolean,
) {
  await speakWithBufferedSpeech(
    "supertonic",
    position,
    sentence,
    token,
    set,
    get,
    requireInitialBuffer,
  );
}

async function speakWithBufferedSpeech(
  provider: NeuralTtsProvider,
  position: Position,
  sentence: string,
  token: number,
  set: (partial: Partial<PlayerState>) => void,
  get: () => PlayerState,
  requireInitialBuffer: boolean,
) {
  const label = provider === "kokoro" ? "Kokoro" : "Supertonic";
  const lookaheadPositions = speechPositionsFromCurrent(
    get(),
    SPEECH_LOOKAHEAD_SENTENCES,
  );
  set({
    isPlaying: true,
    isBuffering: true,
    bufferingMessage: `Buffering ${label} audio`,
    error: null,
    activeSentenceText: sentence,
  });

  try {
    void fillSpeechBuffer(provider, lookaheadPositions, token, get);
    if (requireInitialBuffer) {
      const initialPositions = lookaheadPositions.slice(
        0,
        SPEECH_INITIAL_BUFFER_SENTENCES,
      );
      await fillSpeechBuffer(
        provider,
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
      provider,
      position,
      text: sentence,
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
      provider,
      speechPositionsFromCurrent(get(), SPEECH_LOOKAHEAD_SENTENCES),
      token,
      get,
    );
  } catch (error) {
    if (token !== utteranceToken) {
      return;
    }
    set({
      isPlaying: false,
      isBuffering: false,
      bufferingMessage: "",
      error: displayError(error),
    });
  }
}

async function cachedSpeechBlob({
  provider,
  position,
  text,
  state,
  settings,
  onStatus,
}: {
  provider: NeuralTtsProvider;
  position: Position;
  text: string;
  state: PlayerState;
  settings: SettingsState;
  onStatus?: (status: string) => void;
}) {
  const speechText = mathContentToSpeech(text);
  const key = speechCacheKey(provider, position, speechText, state, settings);
  const cached = speechCache.get(key);
  if (cached) {
    cached.lastUsed = Date.now();
    return cached.promise;
  }

  const promise = synthesizeSpeechBlob({
    provider,
    text: speechText,
    state,
    settings,
    onStatus,
  }).catch((error) => {
    speechCache.delete(key);
    throw error;
  });

  speechCache.set(key, { promise, lastUsed: Date.now() });
  trimSpeechCache();
  return promise;
}

async function synthesizeSpeechBlob({
  provider,
  text,
  state,
  settings,
  onStatus,
}: {
  provider: NeuralTtsProvider;
  text: string;
  state: PlayerState;
  settings: SettingsState;
  onStatus?: (status: string) => void;
}) {
  if (provider === "kokoro") {
    return synthesizeKokoroSpeech({
      text,
      speed: state.speed,
      voiceId: state.voice,
      precision: settings.modelPrecision,
      onStatus,
    });
  }

  const speech = await api.synthesizeSpeech({
    text,
    speed: state.speed,
    voiceId:
      provider === "supertonic" ? settings.supertonicVoiceStyle : state.voice,
  });
  return new Blob([new Uint8Array(speech.audio)], {
    type: speech.mimeType || "audio/mpeg",
  });
}

async function fillSpeechBuffer(
  provider: NeuralTtsProvider,
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
      const text = sentenceAtPosition(state, position);
      if (!text) {
        continue;
      }

      try {
        await cachedSpeechBlob({
          provider,
          position,
          text,
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
  provider: NeuralTtsProvider,
  position: Position,
  text: string,
  state: PlayerState,
  settings: SettingsState,
) {
  const section = state.sections[position.sectionIndex];
  const paragraph = state.paragraphs[position.paragraphIndex];
  return JSON.stringify({
    provider,
    documentId: state.document?.id ?? "",
    sectionId: section?.id ?? "",
    paragraphId: paragraph?.id ?? "",
    sentenceIndex: position.sentenceIndex,
    text,
    voice: state.voice,
    speed: state.speed,
    precision: settings.modelPrecision,
    supertonicVoiceStyle:
      provider === "supertonic" ? settings.supertonicVoiceStyle : "",
    supertonicLanguage:
      provider === "supertonic" ? settings.supertonicLanguage : "",
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
    });
  };

  set({
    isPlaying: true,
    isBuffering: false,
    bufferingMessage: "",
    error: null,
    activeSentenceText: currentSentence(get()) ?? "",
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
      error: error instanceof Error ? error.message : String(error),
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
  const wasPlaying = get().isPlaying || fromAutoAdvance;
  const next = direction > 0 ? nextPosition(get()) : previousPosition(get());
  if (!next) {
    cancelSpeech();
    set({
      isPlaying: false,
      isBuffering: false,
      bufferingMessage: "",
      activeSentenceText: "",
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
  if (!options.preservePlayback) {
    cancelSpeech();
    set({
      isBuffering: true,
      bufferingMessage: "Loading section",
      isPlaying: false,
      error: null,
    });
  }

  try {
    let paragraphs = get().paragraphs;
    let sectionImages = get().sectionImages;
    if (position.sectionIndex !== get().currentSectionIndex) {
      const section = get().sections[position.sectionIndex];
      if (!section) {
        return;
      }
      if (options.preservePlayback) {
        set({
          isBuffering: true,
          bufferingMessage: "Loading section",
          error: null,
        });
      }
      [paragraphs, sectionImages] = await Promise.all([
        api.listParagraphs(section.id),
        api.listSectionImages(section.id),
      ]);
    }

    const paragraphIndex = clamp(
      position.paragraphIndex,
      0,
      Math.max(0, paragraphs.length - 1),
    );
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
    set({
      isBuffering: false,
      bufferingMessage: "",
      error: error instanceof Error ? error.message : String(error),
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
  if (position.sectionIndex !== state.currentSectionIndex) {
    return null;
  }

  const paragraph = state.paragraphs[position.paragraphIndex];
  if (!paragraph) {
    return null;
  }

  return sentenceText(paragraph, position.sentenceIndex);
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

  const paragraph =
    previousParagraph.sectionIndex === state.currentSectionIndex
      ? state.paragraphs[previousParagraph.paragraphIndex]
      : null;
  return {
    ...previousParagraph,
    sentenceIndex: paragraph ? Math.max(0, sentenceCount(paragraph) - 1) : 0,
  };
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
    return {
      sectionIndex: state.currentSectionIndex - 1,
      paragraphIndex: 0,
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

function chooseSystemVoice(voiceId: string) {
  const voices = window.speechSynthesis.getVoices();
  if (voices.length === 0) {
    return null;
  }

  const wantsFemale =
    voiceId.includes("f_") ||
    voiceId.startsWith("af") ||
    voiceId.startsWith("bf");
  const wantsBritish = voiceId.startsWith("b");
  return (
    voices.find((voice) =>
      wantsBritish
        ? voice.lang.toLowerCase().includes("gb")
        : voice.lang.toLowerCase().includes("us"),
    ) ??
    voices.find((voice) =>
      wantsFemale
        ? /samantha|victoria|karen|female/i.test(voice.name)
        : /alex|daniel|male/i.test(voice.name),
    ) ??
    voices[0]
  );
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

function cancelSpeech() {
  utteranceToken += 1;
  clearGeneratedAudio();
  if ("speechSynthesis" in window) {
    window.speechSynthesis.cancel();
  }
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
