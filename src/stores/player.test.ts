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
