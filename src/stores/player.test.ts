import { afterEach, describe, expect, it, vi } from "vitest";
import type { FakeEngine, SpeechEngineSettings } from "../lib/speech";
import type * as Domain from "../types/domain";

const DOCUMENT: Domain.Document = {
  id: "doc-1",
  title: "A Book",
  sourceType: "openstax",
  sourceMetadata: null,
  coverImagePath: null,
  license: null,
  attribution: null,
  wordCount: 8,
  sourceLanguage: "en",
  importedAt: "2026-01-01T00:00:00Z",
  lastOpenedAt: null,
  progress: 0,
};

const SECTIONS: Domain.Section[] = [
  { id: "sec-1", documentId: "doc-1", ordinal: 0, title: "Chapter One", wordCount: 8 },
];

// "First sentence." is [0,15); "Second sentence." is [16,32).
const PARAGRAPHS: Domain.Paragraph[] = [
  {
    id: "para-1",
    sectionId: "sec-1",
    ordinal: 0,
    text: "First sentence. Second sentence.",
    sentenceOffsets: [
      [0, 15],
      [16, 32],
    ],
    // Deliberately unlike the display text: a test can then tell whether the
    // player spoke the backend's speech form or fell back to slicing.
    sentenceSpeech: ["First sentence spoken.", "Second sentence spoken."],
  },
  {
    id: "para-2",
    sectionId: "sec-1",
    ordinal: 1,
    text: "Third sentence.",
    sentenceOffsets: [[0, 15]],
    sentenceSpeech: ["Third sentence spoken."],
  },
];

/**
 * Six sentences in one paragraph. The prefetch runs two workers, so a single
 * failed sentence is absorbed by the other -- only a contiguous run of
 * failures can strand the sentences past it, and six is the shortest list that
 * leaves an unmistakable one at the end.
 */
const LONG_PARAGRAPHS: Domain.Paragraph[] = [
  {
    id: "para-long",
    sectionId: "sec-1",
    ordinal: 0,
    text: "One. Two. Three. Four. Five. Six.",
    sentenceOffsets: [
      [0, 4],
      [5, 9],
      [10, 16],
      [17, 22],
      [23, 28],
      [29, 33],
    ],
    sentenceSpeech: [
      "Sentence one spoken.",
      "Sentence two spoken.",
      "Sentence three spoken.",
      "Sentence four spoken.",
      "Sentence five spoken.",
      "Sentence six spoken.",
    ],
  },
];

/**
 * Every test gets a fresh module graph: the player keeps its speech cache,
 * utterance tokens and memoized engine in module scope, and they would
 * otherwise leak between cases.
 */
interface PlayerOptions {
  /** What `get_playback_state` returns. Null is a book never opened. */
  playbackState?: Domain.PlaybackState | null;
  sections?: Domain.Section[];
  /** Paragraphs per section id, for the multi-section resume cases. */
  paragraphsBySection?: Record<string, Domain.Paragraph[]>;
  /** Makes every persist reject, the way a locked database does. */
  persistFails?: boolean;
  /** Makes the resume-cursor read reject. */
  readFails?: boolean;
  document?: Domain.Document;
  translatedParagraphs?: Domain.Paragraph[];
  translationResult?: Domain.TranslateSectionResult;
}

async function loadPlayer(
  engines: FakeEngine[],
  paragraphs: Domain.Paragraph[] = PARAGRAPHS,
  options: PlayerOptions = {},
) {
  vi.resetModules();

  const sections = options.sections ?? SECTIONS;
  const savePlaybackState = vi.fn(async (_state: { voiceId: string }) => {
    if (options.persistFails) {
      throw new Error("database is locked");
    }
    return undefined;
  });
  const getPlaybackState = vi.fn(async (_documentId: string) => {
    if (options.readFails) {
      throw new Error("database is locked");
    }
    return options.playbackState ?? null;
  });
  const translateSection = vi.fn(async (_sectionId: string) =>
    options.translationResult ?? {
      status: "complete" as const,
      sourceLang: "en",
      targetLang: "es",
      fallbackCount: 0,
      sentenceCount: paragraphs.flatMap((paragraph) => paragraph.sentenceSpeech)
        .length,
    },
  );
  const setDocumentSourceLanguage = vi.fn(
    async (_documentId: string, _sourceLanguage: string) => undefined,
  );
  const listParagraphs = vi.fn(
    async (sectionId: string, targetLang?: string | null) =>
      targetLang && options.translatedParagraphs
        ? options.translatedParagraphs
        : options.paragraphsBySection?.[sectionId] ?? paragraphs,
  );
  let created = 0;
  const createSpeechEngine = vi.fn(
    (_settings: SpeechEngineSettings) =>
      engines[Math.min(created++, engines.length - 1)],
  );

  vi.doMock("../lib/speech", async () => ({
    ...(await vi.importActual<typeof import("../lib/speech")>("../lib/speech")),
    createSpeechEngine,
  }));

  vi.doMock("../lib/tauri", () => ({
    api: {
      getDocument: vi.fn(async () => options.document ?? DOCUMENT),
      listSections: vi.fn(async () => sections),
      listParagraphs,
      listSectionImages: vi.fn(async () => []),
      savePlaybackState,
      getPlaybackState,
      translateSection,
      cancelSectionTranslation: vi.fn(async () => undefined),
      setDocumentSourceLanguage,
      // switchToSupertonic goes through useSettingsStore.setTtsProvider,
      // which calls this; without it the switch action rejects with
      // "api.setSetting is not a function" before it ever reaches the
      // provider/playback assertions below.
      setSetting: vi.fn(async () => undefined),
    },
    isTauriRuntime: () => false,
  }));

  const { usePlayerStore } = await import("./player");
  return {
    usePlayerStore,
    createSpeechEngine,
    savePlaybackState,
    getPlaybackState,
    listParagraphs,
    translateSection,
    setDocumentSourceLanguage,
  };
}

afterEach(() => {
  vi.doUnmock("../lib/speech");
  vi.doUnmock("../lib/tauri");
});

async function createFake(options?: Parameters<typeof import("../lib/speech").createFakeEngine>[0]) {
  const { createFakeEngine } = await import("../lib/speech/fakeEngine");
  return createFakeEngine(options);
}

describe("read-ahead buffering", () => {
  it("keeps buffering past a sentence it cannot synthesize", async () => {
    // One bad sentence used to end the whole read-ahead: the worker returned
    // on its first failure, so every sentence past it went unbuffered and
    // playback fell back to synthesizing each one as it reached it. Failing a
    // contiguous run kills both workers, which is the only way to strand the
    // tail with a concurrency of two.
    //
    // Deliberately not a test that playback stops: it does not, and it must
    // not. Sentence one succeeds, so `speakWithBufferedSpeech` plays it and
    // anything genuinely fatal is still that function's catch to report.
    const engine = await createFake();
    engine.failSynthesisFor(
      (text) => /two|three|four|five/.test(text),
      new Error("synthesis failed"),
    );
    const { usePlayerStore } = await loadPlayer([engine], LONG_PARAGRAPHS);

    await usePlayerStore.getState().loadDocument("doc-1");
    await usePlayerStore.getState().play();

    await vi.waitFor(() =>
      expect(engine.calls.map((call) => call.text)).toContain(
        "Sentence six spoken.",
      ),
    );
    expect(usePlayerStore.getState().isPlaying).toBe(true);
  });
});

describe("playback through SpeechEngine", () => {
  it("translates the chapter before speaking and reloads its speech forms", async () => {
    const engine = await createFake();
    const translated = PARAGRAPHS.map((paragraph, index) => ({
      ...paragraph,
      sentenceSpeech:
        index === 0
          ? ["Primera frase.", "Segunda frase."]
          : ["Tercera frase."],
    }));
    const { usePlayerStore, translateSection, listParagraphs } = await loadPlayer(
      [engine],
      PARAGRAPHS,
      { translatedParagraphs: translated },
    );
    const { useSettingsStore } = await import("./settings");
    useSettingsStore.setState({ translationTargetLang: "es" });

    await usePlayerStore.getState().loadDocument("doc-1");
    await usePlayerStore.getState().play();

    expect(translateSection).toHaveBeenCalledWith("sec-1");
    expect(listParagraphs).toHaveBeenLastCalledWith("sec-1", "es");
    expect(engine.calls[0].text).toBe("Primera frase.");
  });

  it("derives Original-language pronunciation from the current book", async () => {
    const engine = await createFake();
    const { usePlayerStore, createSpeechEngine } = await loadPlayer(
      [engine],
      PARAGRAPHS,
      { document: { ...DOCUMENT, sourceLanguage: "fr" } },
    );
    const { useSettingsStore } = await import("./settings");
    useSettingsStore.setState({
      translationTargetLang: null,
      supertonicLanguage: "es",
    });

    await usePlayerStore.getState().loadDocument("doc-1");
    await usePlayerStore.getState().play();

    expect(createSpeechEngine).toHaveBeenCalledWith(
      expect.objectContaining({ supertonicLanguage: "fr" }),
    );
  });

  it("reloads original speech after translated narration is turned off", async () => {
    const engine = await createFake();
    const translated = PARAGRAPHS.map((paragraph) => ({
      ...paragraph,
      sentenceSpeech: paragraph.sentenceSpeech.map(() => "Narración traducida."),
    }));
    const { usePlayerStore, listParagraphs } = await loadPlayer(
      [engine],
      PARAGRAPHS,
      { translatedParagraphs: translated },
    );
    const { useSettingsStore } = await import("./settings");
    useSettingsStore.setState({ translationTargetLang: "es" });

    await usePlayerStore.getState().loadDocument("doc-1");
    await usePlayerStore.getState().play();
    usePlayerStore.getState().pause();

    useSettingsStore.setState({ translationTargetLang: null });
    await usePlayerStore.getState().play();

    expect(listParagraphs).toHaveBeenLastCalledWith("sec-1", null);
    expect(
      engine.calls.some((call) => call.text === "First sentence spoken."),
    ).toBe(true);
  });

  it("synthesizes the current sentence through whichever engine is active", async () => {
    const engine = await createFake();
    const { usePlayerStore } = await loadPlayer([engine]);

    await usePlayerStore.getState().loadDocument("doc-1");
    await usePlayerStore.getState().play();

    expect(engine.calls[0].text).toBe("First sentence spoken.");
    expect(usePlayerStore.getState().isPlaying).toBe(true);
  });

  it("makes the engine ready before asking it to speak", async () => {
    const engine = await createFake();
    const { usePlayerStore } = await loadPlayer([engine]);

    await usePlayerStore.getState().loadDocument("doc-1");
    await usePlayerStore.getState().play();

    expect(engine.readyCalls).toBeGreaterThan(0);
  });

  it("prefetches sentences ahead of the one being spoken", async () => {
    const engine = await createFake();
    const { usePlayerStore } = await loadPlayer([engine]);

    await usePlayerStore.getState().loadDocument("doc-1");
    await usePlayerStore.getState().play();

    const spoken = engine.calls.map((call) => call.text);
    expect(spoken).toContain("Second sentence spoken.");
    expect(spoken).toContain("Third sentence spoken.");
  });

  it("serves a repeated sentence from the cache instead of the engine", async () => {
    const engine = await createFake();
    const { usePlayerStore } = await loadPlayer([engine]);

    await usePlayerStore.getState().loadDocument("doc-1");
    await usePlayerStore.getState().play();
    const afterFirstPlay = engine.calls.length;

    await usePlayerStore.getState().seekToSentence(0, 0);
    await usePlayerStore.getState().play();

    expect(engine.calls.length).toBe(afterFirstPlay);
  });

  it("re-synthesizes when speed changes, because the cache key includes it", async () => {
    const engine = await createFake();
    const { usePlayerStore } = await loadPlayer([engine]);

    await usePlayerStore.getState().loadDocument("doc-1");
    await usePlayerStore.getState().play();
    const atOriginalSpeed = engine.calls.length;

    usePlayerStore.getState().setSpeed(1.5);
    await usePlayerStore.getState().play();

    expect(engine.calls.length).toBeGreaterThan(atOriginalSpeed);
    expect(engine.calls[engine.calls.length - 1].speed).toBe(1.5);
  });

  it("re-synthesizes when the Fish voice changes, because the cache key includes it", async () => {
    // Deliberately the same shape as "serves a repeated sentence from the
    // cache" above, seeking back to the *same* sentence so an extra call can
    // only be a cache miss and not a different sentence being spoken. A
    // fishVoiceId change keeps engine.id === "fish", so nothing else in the
    // key moves: without fishVoiceId in it, the buffered sentences replay in
    // the old voice while new ones use the new one, and the chapter narrates
    // in two voices.
    const engine = await createFake({ id: "fish" });
    const { usePlayerStore } = await loadPlayer([engine]);
    const { useSettingsStore } = await import("./settings");

    useSettingsStore.setState({ ttsProvider: "fish", fishVoiceId: "voice-1" });
    await usePlayerStore.getState().loadDocument("doc-1");
    await usePlayerStore.getState().play();
    const withFirstVoice = engine.calls.length;

    useSettingsStore.setState({ fishVoiceId: "voice-2" });
    await usePlayerStore.getState().seekToSentence(0, 0);
    await usePlayerStore.getState().play();

    expect(engine.calls.length).toBeGreaterThan(withFirstVoice);
  });
});

describe("book source language", () => {
  it("persists a correction and updates the open document", async () => {
    const engine = await createFake();
    const { usePlayerStore, setDocumentSourceLanguage } = await loadPlayer([
      engine,
    ]);
    await usePlayerStore.getState().loadDocument("doc-1");

    await usePlayerStore.getState().setDocumentSourceLanguage("FR");

    expect(setDocumentSourceLanguage).toHaveBeenCalledWith("doc-1", "FR");
    expect(usePlayerStore.getState().document?.sourceLanguage).toBe("fr");
  });
});

describe("buffering message", () => {
  it("names the engine actually speaking, not always Supertonic", async () => {
    // Regression guard: `speakWithBufferedSpeech` used to hardcode
    // `const label = "Supertonic"`, so a Fish Audio user was told "Buffering
    // Supertonic audio" while Fish was the engine actually running. The
    // label must come from `engine.id`, which this fake engine sets to
    // "fish" independent of whatever `useSettingsStore` reports.
    const engine = await createFake({ id: "fish" });
    const { usePlayerStore } = await loadPlayer([engine]);

    await usePlayerStore.getState().loadDocument("doc-1");

    // Checked synchronously, before awaiting: the buffering message is set
    // before the function's first await, so it is already in the store the
    // instant `play()` is called, without needing the call to finish.
    const playPromise = usePlayerStore.getState().play();
    expect(usePlayerStore.getState().bufferingMessage).toBe(
      "Buffering Fish Audio audio",
    );
    await playPromise;
  });
});

describe("engine selection", () => {
  it("builds the engine once and reuses it across sentences", async () => {
    const engine = await createFake();
    const { usePlayerStore, createSpeechEngine } = await loadPlayer([engine]);

    await usePlayerStore.getState().loadDocument("doc-1");
    await usePlayerStore.getState().play();
    await usePlayerStore.getState().skipForward();

    expect(createSpeechEngine).toHaveBeenCalledTimes(1);
  });

  it("rebuilds the engine when the read-aloud target changes", async () => {
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

    useSettingsStore.setState({ translationTargetLang: "ko" });
    await usePlayerStore.getState().play();

    // The one read-aloud choice is both translation target and pronunciation,
    // so changing it must not keep the engine built for the previous target.
    expect(createSpeechEngine).toHaveBeenCalledTimes(2);
    expect(korean.calls.length).toBeGreaterThan(0);
  });

  it("rebuilds the engine when the Supertonic voice style changes", async () => {
    // The style is captured at engine construction (see supertonicEngine.ts),
    // so it has to be in the engine cache key for the same reason the
    // language does: without it, changing Voice style has no effect until the
    // reader switches providers and back, which is indistinguishable from the
    // setting being ignored outright -- the bug this replaced.
    const male = await createFake({ voices: ["M1"] });
    const female = await createFake({ voices: ["F3"] });
    const { usePlayerStore, createSpeechEngine } = await loadPlayer([
      male,
      female,
    ]);
    const { useSettingsStore } = await import("./settings");

    await usePlayerStore.getState().loadDocument("doc-1");
    await usePlayerStore.getState().play();
    expect(createSpeechEngine).toHaveBeenCalledTimes(1);

    useSettingsStore.setState({ supertonicVoiceStyle: "F3" });
    await usePlayerStore.getState().play();

    expect(createSpeechEngine).toHaveBeenCalledTimes(2);
    expect(createSpeechEngine.mock.calls[1][0]).toMatchObject({
      supertonicVoiceStyle: "F3",
    });
    expect(female.calls.length).toBeGreaterThan(0);
  });

  it("does not throw away paid-for Fish audio when a Supertonic setting changes", async () => {
    // Fish bills per synthesis. The Supertonic settings card renders whatever
    // the active provider is, so a Fish listener can save a Voice style at
    // any time -- and an engine/cache key that folds in every provider's
    // settings unconditionally would rebuild the Fish engine and re-key every
    // one of the SPEECH_LOOKAHEAD_SENTENCES already prefetched, re-buying
    // sentences the reader has already paid for. Keys are per engine: what
    // Fish does cannot depend on a Supertonic row.
    const engine = await createFake({ id: "fish" });
    const { usePlayerStore, createSpeechEngine } = await loadPlayer([engine]);
    const { useSettingsStore } = await import("./settings");

    useSettingsStore.setState({ ttsProvider: "fish", fishVoiceId: "voice-1" });
    await usePlayerStore.getState().loadDocument("doc-1");
    await usePlayerStore.getState().play();
    const paidFor = engine.calls.length;

    useSettingsStore.setState({ supertonicVoiceStyle: "F3" });
    await usePlayerStore.getState().seekToSentence(0, 0);
    await usePlayerStore.getState().play();

    expect(createSpeechEngine).toHaveBeenCalledTimes(1);
    expect(engine.calls.length).toBe(paidFor);
  });

  it("records the configured voice after a settings change with no reload and no play", async () => {
    // Seeding on load is not enough: the Reader skips `loadDocument` for the
    // document already open, so a reader who visits Settings, saves a style
    // and comes back to seek without pressing Play was still recording the
    // voice from before the change. The persisted voice comes from the engine
    // itself now, which is the only thing that ever knew it.
    const male = await createFake({ voices: ["M1"] });
    const female = await createFake({ voices: ["F3"] });
    const { usePlayerStore, savePlaybackState } = await loadPlayer([
      male,
      female,
    ]);
    const { useSettingsStore } = await import("./settings");

    await usePlayerStore.getState().loadDocument("doc-1");
    useSettingsStore.setState({ supertonicVoiceStyle: "F3" });
    await usePlayerStore.getState().seekToSentence(0, 1);

    const calls = savePlaybackState.mock.calls;
    expect(calls[calls.length - 1][0].voiceId).toBe("F3");
    // Seeking while paused must not start synthesis on either engine.
    expect(male.calls).toEqual([]);
    expect(female.calls).toEqual([]);
  });

  it("records the configured voice for a book opened but never played", async () => {
    // `persistPlaybackState` also runs from `loadDocument` and
    // `moveToPosition`, which happen before anything is spoken -- so a reader
    // who opens a book and seeks without pressing Play used to write the
    // hardcoded `"M1"` seed. Resolving the engine on load reseeds `voice`
    // through the one path that owns it, rather than teaching a second place
    // how to derive a provider's default voice.
    const engine = await createFake({ voices: ["F3"] });
    const { usePlayerStore, savePlaybackState } = await loadPlayer([engine]);

    await usePlayerStore.getState().loadDocument("doc-1");

    expect(savePlaybackState).toHaveBeenCalled();
    const calls = savePlaybackState.mock.calls;
    expect(calls[calls.length - 1][0].voiceId).toBe("F3");
    // Nothing was spoken, and resolving an engine must not start synthesis.
    expect(engine.calls).toEqual([]);
  });

  it("records the voice actually being spoken, not the seeded one", async () => {
    // `activeEngine` only reseeded `state.voice` when the engine *id* changed,
    // so a session that never switches providers kept the literal "M1" the
    // store is initialised with -- and wrote it to `playback_state.voice_id`
    // while something else entirely was being spoken. Reseeding was gated
    // that way because `state.voice` was in `speechCacheKey` and rewriting it
    // threw away bought audio; it is not in that key any more.
    const engine = await createFake({ voices: ["F3"] });
    const { usePlayerStore, savePlaybackState } = await loadPlayer([engine]);

    await usePlayerStore.getState().loadDocument("doc-1");
    await usePlayerStore.getState().play();

    expect(savePlaybackState).toHaveBeenCalled();
    const calls = savePlaybackState.mock.calls;
    expect(calls[calls.length - 1][0].voiceId).toBe("F3");
  });

  it("still serves audio already bought from a provider after switching away and back", async () => {
    // `speechCacheKey` carried `state.voice`, which `activeEngine` rewrites on
    // every provider swap -- so a Fish listener who hits a rate limit, takes
    // the MiniPlayer's "Switch to Supertonic" escape hatch, then goes back to
    // Fish finds every already-billed sentence keyed under a voice id that no
    // longer matches, and buys the whole lookahead a second time. The engine's
    // voice is already in `engineKey`; carrying it twice only made entries
    // unreachable.
    const fishFirst = await createFake({ id: "fish" });
    const supertonic = await createFake({ id: "supertonic" });
    const fishAgain = await createFake({ id: "fish" });
    const { usePlayerStore } = await loadPlayer([
      fishFirst,
      supertonic,
      fishAgain,
    ]);
    const { useSettingsStore } = await import("./settings");

    useSettingsStore.setState({ ttsProvider: "fish", fishVoiceId: "voice-1" });
    await usePlayerStore.getState().loadDocument("doc-1");
    await usePlayerStore.getState().play();

    useSettingsStore.setState({ ttsProvider: "supertonic" });
    await usePlayerStore.getState().play();

    useSettingsStore.setState({ ttsProvider: "fish", fishVoiceId: "voice-1" });
    await usePlayerStore.getState().seekToSentence(0, 0);
    await usePlayerStore.getState().play();

    // Same provider, same voice, same sentences: nothing to re-buy.
    expect(fishAgain.calls).toEqual([]);
  });

  it("picks up a speed change in the prefetch already running", async () => {
    // `speechCacheKey` folds in `state.speed` as well as the engine's
    // settings, and the liveness guard only watches the latter. It does not
    // need to watch speed: the worker re-reads player state on every
    // iteration, so the sentence it takes next is both synthesized and keyed
    // at the new speed. Audio and key move together, so nothing is filed
    // where it cannot be read.
    const engine = await createFake();
    const { usePlayerStore } = await loadPlayer([engine]);

    await usePlayerStore.getState().loadDocument("doc-1");
    const release = engine.blockNextSynthesis();
    const playing = usePlayerStore.getState().play();
    usePlayerStore.getState().setSpeed(1.5);
    release();
    await playing;

    // Named per sentence: the current sentence is synthesized on its own path
    // after the buffer resolves, so a bare "some call used 1.5" passes even
    // when the prefetch is stuck at the old speed. The third sentence is
    // reachable only through the prefetch, and only after the change.
    const third = engine.calls.find(
      (call) => call.text === "Third sentence spoken.",
    );
    expect(third?.speed).toBe(1.5);
  });

  it("stops prefetching through an engine the settings have already replaced", async () => {
    // Pinning the snapshot to the engine keeps the *keys* honest, but the
    // fire-and-forget prefetch has no reason left to stop: auto-advance
    // reuses the utterance token, so `token !== utteranceToken` never trips.
    // Every sentence it goes on to synthesize is filed under the old engine's
    // key, which nothing will ever read again, and the new engine then
    // re-synthesizes the same positions. On Fish that is billed twice.
    // Reading the store here is a liveness check, never a key -- the two are
    // different questions.
    const first = await createFake({ id: "fish" });
    const second = await createFake({ id: "fish" });
    const { usePlayerStore } = await loadPlayer([first, second]);
    const { useSettingsStore } = await import("./settings");

    useSettingsStore.setState({ ttsProvider: "fish", fishVoiceId: "voice-1" });
    await usePlayerStore.getState().loadDocument("doc-1");

    const release = first.blockNextSynthesis();
    const playing = usePlayerStore.getState().play();
    // Saved from Settings while the MiniPlayer keeps playing.
    useSettingsStore.setState({ fishVoiceId: "voice-2" });
    release();
    await playing;

    // The lookahead sentence the old engine had not started yet must never be
    // bought from it: only the new voice will ever be played.
    expect(first.calls.map((call) => call.text)).not.toContain(
      "Third sentence spoken.",
    );
  });

  it("will not treat settings that have not loaded yet as the reader's", async () => {
    // `hydrateFailed` is false in two states -- loaded, and not loaded yet --
    // and only the first is the reader's. A slow `get_all_settings` plus an
    // early Play would otherwise build Supertonic from DEFAULT_SETTINGS with
    // permission to fetch its ~383MB model, for a reader whose provider is
    // Fish. Had the load *failed* rather than merely been slow, the identical
    // defaults would have been refused.
    const early = await createFake();
    const loaded = await createFake();
    const { usePlayerStore, createSpeechEngine } = await loadPlayer([
      early,
      loaded,
    ]);
    const { useSettingsStore } = await import("./settings");

    useSettingsStore.setState({ hydrated: false, hydrateFailed: false });
    await usePlayerStore.getState().loadDocument("doc-1");
    await usePlayerStore.getState().play();
    expect(createSpeechEngine.mock.calls[0][0]).toMatchObject({
      settingsSource: "unloaded",
    });

    useSettingsStore.setState({ hydrated: true });
    await usePlayerStore.getState().play();

    const last = createSpeechEngine.mock.calls.length - 1;
    expect(createSpeechEngine.mock.calls[last][0]).toMatchObject({
      settingsSource: "loaded",
    });
  });

  it("rebuilds the engine when a settings retry turns a guessed provider into a real one", async () => {
    // `activeEngine` captures whether the settings are real, to stop
    // Supertonic fetching its model for a provider the fallback only guessed
    // at. Anything captured has to be in the key, or the flag goes stale:
    // here the retry succeeds onto rows identical to the defaults, so nothing
    // else in the key moves and playback would keep refusing for the rest of
    // the session with no way out but a restart.
    const guessed = await createFake();
    const real = await createFake();
    const { usePlayerStore, createSpeechEngine } = await loadPlayer([
      guessed,
      real,
    ]);
    const { useSettingsStore } = await import("./settings");

    useSettingsStore.setState({ hydrated: true, hydrateFailed: true });
    await usePlayerStore.getState().loadDocument("doc-1");
    await usePlayerStore.getState().play();
    expect(createSpeechEngine.mock.calls[0][0]).toMatchObject({
      settingsSource: "failed",
    });

    useSettingsStore.setState({ hydrateFailed: false });
    await usePlayerStore.getState().play();

    const last = createSpeechEngine.mock.calls.length - 1;
    expect(createSpeechEngine.mock.calls[last][0]).toMatchObject({
      settingsSource: "loaded",
    });
    // Rebuilt, but the buffer survives: the retry loaded rows identical to
    // the defaults, so nothing about how a sentence sounds moved. Keying the
    // speech cache on the source too would orphan every prefetched sentence
    // the moment a retry succeeded and re-synthesize audio already correct.
    expect(real.calls).toEqual([]);
  });

  it("keys buffered audio by the settings that built the engine, not by whatever the store says later", async () => {
    // `speakWithBufferedSpeech` resolves its engine once, up front, but used
    // to re-read the settings store after awaiting -- so a style saved while
    // the initial buffer was filling had the *old* engine writing audio under
    // the *new* style's cache key. The next sentence then hit those entries
    // and the reader kept hearing the previous voice, indefinitely: the cache
    // is size-trimmed, never cleared on an engine rebuild.
    const male = await createFake({ voices: ["M1"] });
    const female = await createFake({ voices: ["F3"] });
    const { usePlayerStore } = await loadPlayer([male, female]);
    const { useSettingsStore } = await import("./settings");

    await usePlayerStore.getState().loadDocument("doc-1");

    const release = male.blockNextSynthesis();
    const playing = usePlayerStore.getState().play();
    // Saved mid-buffer: this is the window the skew lives in.
    useSettingsStore.setState({ supertonicVoiceStyle: "F3" });
    release();
    await playing;

    await usePlayerStore.getState().seekToSentence(0, 0);
    await usePlayerStore.getState().play();

    // Named per sentence, not just counted: the post-playback prefetch is
    // fire-and-forget and re-covers the *current* sentence, so a count alone
    // passes on some later sentence the new engine happened to reach while
    // the one being listened to was served from a poisoned entry.
    expect(female.calls.map((call) => call.text)).toContain(
      "First sentence spoken.",
    );
  });
});

describe("switching away from Fish", () => {
  it("keeps the escape hatch when another click supersedes its provider write", async () => {
    // `setTtsProvider` resolves without applying when a later click
    // supersedes it, so resolving is not the same as "the provider is now
    // Supertonic". Treating it as one removes the button and immediately
    // replays through the Fish engine that just failed.
    const engine = await createFake({ id: "fish" });
    const { usePlayerStore } = await loadPlayer([engine]);
    const { useSettingsStore } = await import("./settings");

    useSettingsStore.setState({ ttsProvider: "fish", fishVoiceId: "voice-1" });
    await usePlayerStore.getState().loadDocument("doc-1");
    usePlayerStore.setState({ canSwitchToSupertonic: true });

    const switching = usePlayerStore.getState().switchToSupertonic();
    // A Settings click landing on top of it.
    const overriding = useSettingsStore.getState().setTtsProvider("fish");
    await Promise.all([switching, overriding]);

    expect(useSettingsStore.getState().ttsProvider).toBe("fish");
    expect(usePlayerStore.getState().canSwitchToSupertonic).toBe(true);
  });
});

describe("cancellation", () => {
  it("discards a synthesis that lands after pause, without surfacing an error", async () => {
    const engine = await createFake();
    const { usePlayerStore } = await loadPlayer([engine]);

    await usePlayerStore.getState().loadDocument("doc-1");

    const release = engine.blockNextSynthesis();
    const playing = usePlayerStore.getState().play();
    usePlayerStore.getState().pause();
    release();
    await playing;

    const state = usePlayerStore.getState();
    expect(state.isPlaying).toBe(false);
    expect(state.error).toBeNull();
  });

  it("reports a real synthesis failure", async () => {
    const engine = await createFake();
    const { usePlayerStore } = await loadPlayer([engine]);

    await usePlayerStore.getState().loadDocument("doc-1");
    engine.failSynthesis(new Error("model is missing"));
    await usePlayerStore.getState().play();

    expect(usePlayerStore.getState().error).toBe("model is missing");
    expect(usePlayerStore.getState().isPlaying).toBe(false);
  });
});

describe("Fish Audio failure handling", () => {
  it("stops playback and maps a Fish error kind to its message, without switching providers", async () => {
    const engine = await createFake({ id: "fish" });
    const { usePlayerStore } = await loadPlayer([engine]);
    const { useSettingsStore } = await import("./settings");

    // Fish must actually be the active provider for the failure to be
    // attributed to it — and this seeds ttsProvider away from the store's
    // "supertonic" default, so the closing assertion (it is still "fish")
    // cannot pass merely because nothing touched the field.
    useSettingsStore.setState({ ttsProvider: "fish", fishVoiceId: "voice-1" });
    await usePlayerStore.getState().loadDocument("doc-1");

    // Seed both fields to values a correct run must overwrite: isPlaying to
    // true (the opposite of the "leaves playback paused" assertion below)
    // and error to unrelated leftover text (so matching the exact mapped
    // message proves the mapping ran, not just that *some* string landed).
    usePlayerStore.setState({ isPlaying: true, error: "stale error" });

    engine.failSynthesis(
      { kind: "auth", message: "invalid key", retryable: false } as unknown as Error,
    );
    await usePlayerStore.getState().play();

    const state = usePlayerStore.getState();
    expect(state.isPlaying).toBe(false);
    expect(state.error).toBe("Fish Audio rejected your API key.");
    expect(state.canSwitchToSupertonic).toBe(true);
    // The switch is the reader's action, never automatic: a failure alone
    // must never touch ttsProvider.
    expect(useSettingsStore.getState().ttsProvider).toBe("fish");
  });

  it.each([
    ["payment_required", "Your Fish Audio account is out of credit."],
    ["rate_limited", "Fish Audio is rate limiting requests."],
    ["voice", "Choose a Fish Audio voice in Settings."],
  ])("maps the %s error kind to its message", async (kind, expectedMessage) => {
    const engine = await createFake({ id: "fish" });
    const { usePlayerStore } = await loadPlayer([engine]);
    const { useSettingsStore } = await import("./settings");

    useSettingsStore.setState({ ttsProvider: "fish", fishVoiceId: "voice-1" });
    await usePlayerStore.getState().loadDocument("doc-1");

    engine.failSynthesis(
      { kind, message: "raw backend message", retryable: false } as unknown as Error,
    );
    await usePlayerStore.getState().play();

    expect(usePlayerStore.getState().error).toBe(expectedMessage);
  });

  it("falls back to the error's own message for an unrecognised kind", async () => {
    const engine = await createFake({ id: "fish" });
    const { usePlayerStore } = await loadPlayer([engine]);
    const { useSettingsStore } = await import("./settings");

    useSettingsStore.setState({ ttsProvider: "fish", fishVoiceId: "voice-1" });
    await usePlayerStore.getState().loadDocument("doc-1");

    engine.failSynthesis(new Error("the Fish backend timed out"));
    await usePlayerStore.getState().play();

    expect(usePlayerStore.getState().error).toBe("the Fish backend timed out");
  });

  it("does not offer the switch when a non-Fish engine fails", async () => {
    const engine = await createFake({ id: "supertonic" });
    const { usePlayerStore } = await loadPlayer([engine]);

    await usePlayerStore.getState().loadDocument("doc-1");
    // Seed true first so a no-op implementation (never flipping it) cannot
    // pass this assertion by coincidence.
    usePlayerStore.setState({ canSwitchToSupertonic: true });

    engine.failSynthesis(new Error("supertonic model is missing"));
    await usePlayerStore.getState().play();

    expect(usePlayerStore.getState().canSwitchToSupertonic).toBe(false);
  });

  it("switches to Supertonic and resumes playback only when the reader clicks it", async () => {
    const fish = await createFake({ id: "fish" });
    const supertonic = await createFake({ id: "supertonic" });
    const { usePlayerStore } = await loadPlayer([fish, supertonic]);
    const { useSettingsStore } = await import("./settings");

    useSettingsStore.setState({ ttsProvider: "fish", fishVoiceId: "voice-1" });
    await usePlayerStore.getState().loadDocument("doc-1");

    fish.failSynthesis({ kind: "auth", message: "x", retryable: false } as unknown as Error);
    await usePlayerStore.getState().play();
    expect(usePlayerStore.getState().canSwitchToSupertonic).toBe(true);

    await usePlayerStore.getState().switchToSupertonic();

    expect(useSettingsStore.getState().ttsProvider).toBe("supertonic");
    expect(usePlayerStore.getState().canSwitchToSupertonic).toBe(false);
    expect(usePlayerStore.getState().isPlaying).toBe(true);
    expect(supertonic.calls.length).toBeGreaterThan(0);
  });
});

describe("speech text", () => {
  it("speaks the backend's speech form, not the displayed text", async () => {
    const engine = await createFake();
    const { usePlayerStore } = await loadPlayer([engine]);

    await usePlayerStore.getState().loadDocument("doc-1");
    await usePlayerStore.getState().play();

    // Conversion happens in Rust now. If this ever reads "First sentence."
    // the player has gone back to converting — or to slicing display text.
    expect(engine.calls[0].text).toBe("First sentence spoken.");
  });

  it("falls back to display text when a paragraph carries no speech form", async () => {
    const engine = await createFake();
    const { usePlayerStore } = await loadPlayer([engine]);

    await usePlayerStore.getState().loadDocument("doc-1");
    usePlayerStore.setState({
      paragraphs: [
        {
          id: "para-legacy",
          sectionId: "sec-1",
          ordinal: 0,
          text: "Legacy sentence.",
          sentenceOffsets: [[0, 16]],
          sentenceSpeech: [],
        },
      ],
      currentParagraphIndex: 0,
      currentSentenceIndex: 0,
    });
    await usePlayerStore.getState().play();

    expect(engine.calls[engine.calls.length - 1].text).toBe("Legacy sentence.");
  });
});

describe("the one-time model download", () => {
  /** Let the play chain run as far as the blocked `ensureReady`. */
  async function settle() {
    await new Promise((resolve) => setTimeout(resolve, 0));
  }

  /**
   * A player stopped inside a model download, with the engine's cancel spy.
   *
   * `play()` is deliberately left unawaited: the download is the state under
   * test, and awaiting it would only ever observe the state after it ended.
   */
  async function playIntoADownload() {
    const engine = await createFake();
    const cancel = vi.fn(async () => undefined);
    engine.reportWhileReadying(
      { message: "Downloading the on-device voice (one time)", cancel },
      {
        message: "Downloading the on-device voice (one time)",
        download: { downloadedBytes: 100, totalBytes: 400 },
        cancel,
      },
    );
    const { usePlayerStore } = await loadPlayer([engine]);
    await usePlayerStore.getState().loadDocument("doc-1");

    const release = engine.blockNextReady();
    const playing = usePlayerStore.getState().play();
    await settle();

    return { usePlayerStore, cancel, release, playing };
  }

  it("carries the download's real byte counts, not just a spinner", async () => {
    // #52: the player was told one static string for the several minutes a
    // 383MB fetch takes, so there was nothing to render but an indeterminate
    // spinner -- which is what made a working download read as a hung app.
    const { usePlayerStore, release, playing } = await playIntoADownload();

    expect(usePlayerStore.getState().modelDownload).toEqual({
      downloadedBytes: 100,
      totalBytes: 400,
    });
    expect(usePlayerStore.getState().bufferingMessage).toBe(
      "Downloading the on-device voice (one time)",
    );

    release();
    await playing;
  });

  it("clears the download once the model is on disk", async () => {
    const { usePlayerStore, release, playing } = await playIntoADownload();

    release();
    await playing;

    expect(usePlayerStore.getState().modelDownload).toBeNull();
  });

  it("stops the download when the reader cancels it", async () => {
    const { usePlayerStore, cancel, release, playing } =
      await playIntoADownload();

    usePlayerStore.getState().cancelModelDownload();

    expect(cancel).toHaveBeenCalledTimes(1);
    const state = usePlayerStore.getState();
    expect(state.modelDownload).toBeNull();
    expect(state.isBuffering).toBe(false);
    expect(state.isPlaying).toBe(false);
    // Cancelling is not failing: an error here would tell the reader
    // something went wrong with the thing they just asked to stop.
    expect(state.error).toBeNull();

    release();
    await playing;
  });

  it("stops the download when the reader presses Pause", async () => {
    // Pause is the control a reader reaches for when the app looks stuck, and
    // the download is the only reason playback has not started. Leaving it
    // running would keep pulling 383MB after they said stop.
    const { usePlayerStore, cancel, release, playing } =
      await playIntoADownload();

    usePlayerStore.getState().pause();

    expect(cancel).toHaveBeenCalledTimes(1);
    expect(usePlayerStore.getState().modelDownload).toBeNull();

    release();
    await playing;
  });

  it("does not offer to stop a download that has already failed", async () => {
    // The cancel handle outliving its download means a later Pause tells Rust
    // to abort a download nobody started.
    const engine = await createFake();
    const cancel = vi.fn(async () => undefined);
    engine.reportWhileReadying({
      message: "Downloading the on-device voice (one time)",
      download: { downloadedBytes: 100, totalBytes: 400 },
      cancel,
    });
    engine.failReady(new Error("download stalled: no data received"));
    const { usePlayerStore } = await loadPlayer([engine]);
    await usePlayerStore.getState().loadDocument("doc-1");

    await usePlayerStore.getState().play();
    expect(usePlayerStore.getState().error).toMatch(/stalled/);

    usePlayerStore.getState().pause();

    expect(cancel).not.toHaveBeenCalled();
  });

  it("does not report a download for an engine that never started one", async () => {
    const engine = await createFake();
    const { usePlayerStore } = await loadPlayer([engine]);

    await usePlayerStore.getState().loadDocument("doc-1");
    await usePlayerStore.getState().play();

    expect(usePlayerStore.getState().modelDownload).toBeNull();
  });
});

describe("spending on an engine that bills", () => {
  it("reads only three sentences ahead, not ten", async () => {
    // Supertonic is free and local, so a deep read-ahead costs nothing but
    // CPU. Fish bills every sentence, and the reader pays for the whole
    // burst whether or not they listen to it -- so the same Play must buy
    // far less. LONG_PARAGRAPHS holds six sentences, which is enough to tell
    // a capped read-ahead from an uncapped one.
    const engine = await createFake({ id: "fish" });
    const { usePlayerStore } = await loadPlayer([engine], LONG_PARAGRAPHS);

    await usePlayerStore.getState().loadDocument("doc-1");
    await usePlayerStore.getState().play();
    await vi.waitFor(() => expect(engine.calls.length).toBeGreaterThan(0));

    expect(engine.calls).toHaveLength(3);
  });

  it("still reads far ahead for an engine that costs nothing", async () => {
    const engine = await createFake({ id: "supertonic" });
    const { usePlayerStore } = await loadPlayer([engine], LONG_PARAGRAPHS);

    await usePlayerStore.getState().loadDocument("doc-1");
    await usePlayerStore.getState().play();
    await vi.waitFor(() => expect(engine.calls).toHaveLength(6));

    expect(engine.calls).toHaveLength(6);
  });

  it("never synthesizes the same sentence twice across a pause", async () => {
    // A request that reached the engine has already been billed. Aborting it
    // afterwards used to throw, which deleted the cache entry, so resuming
    // bought the very same sentence a second time. The reader pays twice and
    // hears it once.
    const engine = await createFake({ id: "fish" });
    const { usePlayerStore } = await loadPlayer([engine], LONG_PARAGRAPHS);

    await usePlayerStore.getState().loadDocument("doc-1");

    const release = engine.blockNextSynthesis();
    const playing = usePlayerStore.getState().play();
    await vi.waitFor(() => expect(engine.calls.length).toBeGreaterThanOrEqual(2));
    usePlayerStore.getState().pause();
    release();
    await playing;

    await usePlayerStore.getState().play();
    await vi.waitFor(() => expect(engine.calls.length).toBeGreaterThanOrEqual(3));

    const spoken = engine.calls.map((call) => call.text);
    expect(new Set(spoken).size).toBe(spoken.length);
  });

  it("sends nothing more to the engine once the reader has paused", async () => {
    // `cancelSpeech` used to drop the AbortController entirely, so a request
    // issued afterwards read `speechAbort?.signal` as undefined and ran with
    // no signal at all -- billed in full, for audio nobody asked for.
    const engine = await createFake({ id: "fish" });
    const { usePlayerStore } = await loadPlayer([engine], LONG_PARAGRAPHS);

    await usePlayerStore.getState().loadDocument("doc-1");

    const release = engine.blockNextSynthesis();
    const playing = usePlayerStore.getState().play();
    await vi.waitFor(() => expect(engine.calls.length).toBeGreaterThanOrEqual(2));
    const atPause = engine.calls.length;

    usePlayerStore.getState().pause();
    release();
    await playing;

    expect(engine.calls).toHaveLength(atPause);
  });
});

describe("resuming where the reader stopped", () => {
  function cursor(
    overrides: Partial<Domain.PlaybackState> = {},
  ): Domain.PlaybackState {
    return {
      documentId: "doc-1",
      sectionId: "sec-1",
      paragraphId: "para-1",
      sentenceIndex: 0,
      sentenceOffsetMs: 0,
      voiceId: "M1",
      speed: 1,
      updatedAt: "2026-08-23T12:00:00Z",
      ...overrides,
    };
  }

  it("opens a document at the saved paragraph and sentence", async () => {
    const engine = await createFake();
    const { usePlayerStore } = await loadPlayer([engine], PARAGRAPHS, {
      playbackState: cursor({ paragraphId: "para-2", sentenceIndex: 0 }),
    });

    await usePlayerStore.getState().loadDocument("doc-1");

    expect(usePlayerStore.getState().currentParagraphIndex).toBe(1);
    expect(usePlayerStore.getState().currentSentenceIndex).toBe(0);
  });

  it("opens a document at the saved sentence within a paragraph", async () => {
    const engine = await createFake();
    const { usePlayerStore } = await loadPlayer([engine], PARAGRAPHS, {
      playbackState: cursor({ paragraphId: "para-1", sentenceIndex: 1 }),
    });

    await usePlayerStore.getState().loadDocument("doc-1");

    expect(usePlayerStore.getState().currentParagraphIndex).toBe(0);
    expect(usePlayerStore.getState().currentSentenceIndex).toBe(1);
  });

  it("loads the saved section's paragraphs, not the first section's", async () => {
    // The whole load path was hardcoded to `sections[0]`. A cursor in chapter
    // seven that only moved the indices would have pointed them at chapter
    // one's paragraphs -- wrong text, wrong length, and a seek straight past
    // the end of the array.
    const engine = await createFake();
    const SECOND_SECTION: Domain.Section[] = [
      SECTIONS[0],
      { id: "sec-2", documentId: "doc-1", ordinal: 1, title: "Chapter Two", wordCount: 3 },
    ];
    const SECTION_TWO_PARAGRAPHS: Domain.Paragraph[] = [
      {
        id: "para-3",
        sectionId: "sec-2",
        ordinal: 0,
        text: "Chapter two opens.",
        sentenceOffsets: [[0, 18]],
        sentenceSpeech: ["Chapter two opens."],
      },
    ];
    const { usePlayerStore } = await loadPlayer([engine], PARAGRAPHS, {
      sections: SECOND_SECTION,
      paragraphsBySection: {
        "sec-1": PARAGRAPHS,
        "sec-2": SECTION_TWO_PARAGRAPHS,
      },
      playbackState: cursor({ sectionId: "sec-2", paragraphId: "para-3" }),
    });

    await usePlayerStore.getState().loadDocument("doc-1");

    expect(usePlayerStore.getState().currentSectionIndex).toBe(1);
    expect(usePlayerStore.getState().paragraphs).toEqual(
      SECTION_TWO_PARAGRAPHS,
    );
    expect(usePlayerStore.getState().currentParagraphIndex).toBe(0);
  });

  it("opens at the beginning when the saved cursor names rows that are gone", async () => {
    // A re-import replaces every section and paragraph id. The cursor's own
    // row is cascaded away with them, but a stale one that survives -- or one
    // read before the delete lands -- must not strand the reader on an index
    // that does not exist.
    const engine = await createFake();
    const { usePlayerStore } = await loadPlayer([engine], PARAGRAPHS, {
      playbackState: cursor({
        sectionId: "sec-gone",
        paragraphId: "para-gone",
        sentenceIndex: 4,
      }),
    });

    await usePlayerStore.getState().loadDocument("doc-1");

    expect(usePlayerStore.getState().currentSectionIndex).toBe(0);
    expect(usePlayerStore.getState().currentParagraphIndex).toBe(0);
    expect(usePlayerStore.getState().currentSentenceIndex).toBe(0);
    expect(usePlayerStore.getState().error).toBeNull();
  });

  it("restores the speed the book was last played at", async () => {
    const engine = await createFake();
    const { usePlayerStore } = await loadPlayer([engine], PARAGRAPHS, {
      playbackState: cursor({ speed: 1.4 }),
    });

    await usePlayerStore.getState().loadDocument("doc-1");

    expect(usePlayerStore.getState().speed).toBe(1.4);
  });

  it("opens a book never played at the configured default speed", async () => {
    // Speed is stored per document, so the fallback is the only thing a fresh
    // book has to go on -- and it is the setting the reader chose, not the
    // 1.0 the store is seeded with.
    const engine = await createFake();
    const { usePlayerStore } = await loadPlayer([engine], PARAGRAPHS, {
      playbackState: null,
    });
    const { useSettingsStore } = await import("./settings");
    useSettingsStore.setState({ defaultSpeed: 0.9 });

    await usePlayerStore.getState().loadDocument("doc-1");

    expect(usePlayerStore.getState().speed).toBe(0.9);
  });

  it("opens a never-read book at its first readable paragraph", async () => {
    const engine = await createFake();
    const { usePlayerStore } = await loadPlayer([engine], PARAGRAPHS, {
      playbackState: null,
    });

    await usePlayerStore.getState().loadDocument("doc-1");

    expect(usePlayerStore.getState().currentParagraphIndex).toBe(0);
    expect(usePlayerStore.getState().currentSentenceIndex).toBe(0);
  });
});

describe("when the reader's place cannot be saved", () => {
  it("says so rather than swallowing the failure", async () => {
    // The catch here was empty, with a comment calling the backend task
    // unfinished. A reader whose database had gone read-only listened for an
    // hour and lost the lot, with nothing on screen having suggested it.
    const engine = await createFake();
    const { usePlayerStore } = await loadPlayer([engine], PARAGRAPHS, {
      persistFails: true,
    });

    await usePlayerStore.getState().loadDocument("doc-1");

    expect(usePlayerStore.getState().positionError).toBeTruthy();
  });

  it("keeps the book playable, because only the bookkeeping failed", async () => {
    const engine = await createFake();
    const { usePlayerStore } = await loadPlayer([engine], PARAGRAPHS, {
      persistFails: true,
    });

    await usePlayerStore.getState().loadDocument("doc-1");

    expect(usePlayerStore.getState().error).toBeNull();
    expect(usePlayerStore.getState().document).not.toBeNull();
  });

  it("takes the notice back down once a save succeeds", async () => {
    const engine = await createFake();
    // Mutated below: the harness reads this object at call time, so the same
    // player can be made to fail a save and then succeed at one.
    const options = { persistFails: true };
    const { usePlayerStore } = await loadPlayer([engine], PARAGRAPHS, options);

    await usePlayerStore.getState().loadDocument("doc-1");
    expect(usePlayerStore.getState().positionError).toBeTruthy();

    options.persistFails = false;
    await usePlayerStore.getState().seekToSentence(0, 1);

    expect(usePlayerStore.getState().positionError).toBeNull();
  });

  it("opens the book at the beginning when the saved place cannot be read", async () => {
    // A failed read is not a failed open: the book still works, it just starts
    // over. Letting this reach the outer catch would have refused to open the
    // document at all.
    const engine = await createFake();
    const { usePlayerStore } = await loadPlayer([engine], PARAGRAPHS, {
      readFails: true,
    });

    await usePlayerStore.getState().loadDocument("doc-1");

    expect(usePlayerStore.getState().document).not.toBeNull();
    expect(usePlayerStore.getState().currentParagraphIndex).toBe(0);
    expect(usePlayerStore.getState().error).toBeNull();
    expect(usePlayerStore.getState().positionError).toBeTruthy();
  });
});
