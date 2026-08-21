import { afterEach, describe, expect, it, vi } from "vitest";
import type { SynthesizeSpeechRequest } from "../tauri";

const synthesizeSpeech = vi.fn(async (_request: SynthesizeSpeechRequest) => ({
  audio: [1, 2, 3],
  mimeType: "audio/mpeg",
}));

vi.mock("../tauri", () => ({
  api: {
    synthesizeSpeech: (request: SynthesizeSpeechRequest) =>
      synthesizeSpeech(request),
    getFishKeyStatus: vi.fn(async () => ({ present: true })),
    listFishVoices: vi.fn(async () => []),
  },
  isTauriRuntime: () => false,
}));

afterEach(() => {
  synthesizeSpeech.mockClear();
});

async function loadEngine(voiceId: string | null) {
  const { createFishEngine } = await import("./fishEngine");
  return createFishEngine({ voiceId });
}

describe("fish engine voice selection", () => {
  it("sends its configured voice, not a Supertonic voice style carried over from the player", async () => {
    // The regression this guards: playback carried one voice id across
    // engines, seeded with the Supertonic style "M1", so on a fresh launch
    // with Fish already selected the request arrived saying "M1". Fish 404s
    // on it, which maps to a `voice` error, and the reader was told to
    // "Choose a Fish Audio voice in Settings" -- while a voice was chosen.
    const engine = await loadEngine("d8ee9d1a-6f3e-4b8a-9c1d-abcdef012345");

    await engine.synthesize({ text: "Hello.", speed: 1 });

    expect(synthesizeSpeech).toHaveBeenCalledTimes(1);
    const request = synthesizeSpeech.mock.calls[0][0];
    expect(request.voiceId).toBe("d8ee9d1a-6f3e-4b8a-9c1d-abcdef012345");
    expect(request.voiceId).not.toBe("M1");
  });

  it("reports as its default voice the one it actually synthesizes with", async () => {
    // `SpeechEngine.defaultVoice` is what playback persists as the reader's
    // voice, so an engine whose `defaultVoice` disagrees with what it sends
    // records a voice nobody heard. Asserted against the same call rather
    // than a literal, so the two cannot drift apart.
    const engine = await loadEngine("voice-abc");

    await engine.synthesize({ text: "Hello.", speed: 1 });

    expect(synthesizeSpeech.mock.calls[0][0].voiceId).toBe(engine.defaultVoice);
  });
});
