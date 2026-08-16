import { afterEach, describe, expect, it, vi } from "vitest";
import type { FakeEngine } from "../lib/speech";
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
  importedAt: "2026-01-01T00:00:00Z",
  lastOpenedAt: null,
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
 * Every test gets a fresh module graph: the player keeps its speech cache,
 * utterance tokens and memoized engine in module scope, and they would
 * otherwise leak between cases.
 */
async function loadPlayer(engines: FakeEngine[]) {
  vi.resetModules();

  let created = 0;
  const createSpeechEngine = vi.fn(() => engines[Math.min(created++, engines.length - 1)]);

  vi.doMock("../lib/speech", async () => ({
    ...(await vi.importActual<typeof import("../lib/speech")>("../lib/speech")),
    createSpeechEngine,
  }));

  vi.doMock("../lib/tauri", () => ({
    api: {
      getDocument: vi.fn(async () => DOCUMENT),
      listSections: vi.fn(async () => SECTIONS),
      listParagraphs: vi.fn(async () => PARAGRAPHS),
      listSectionImages: vi.fn(async () => []),
      savePlaybackState: vi.fn(async () => undefined),
      // switchToSupertonic goes through useSettingsStore.setTtsProvider,
      // which calls this; without it the switch action rejects with
      // "api.setSetting is not a function" before it ever reaches the
      // provider/playback assertions below.
      setSetting: vi.fn(async () => undefined),
    },
    isTauriRuntime: () => false,
  }));

  const { usePlayerStore } = await import("./player");
  return { usePlayerStore, createSpeechEngine };
}

afterEach(() => {
  vi.doUnmock("../lib/speech");
  vi.doUnmock("../lib/tauri");
});

async function createFake(options?: Parameters<typeof import("../lib/speech").createFakeEngine>[0]) {
  const { createFakeEngine } = await import("../lib/speech/fakeEngine");
  return createFakeEngine(options);
}

describe("playback through SpeechEngine", () => {
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
