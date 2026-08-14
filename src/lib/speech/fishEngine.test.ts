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
    // The regression this guards: `player.ts` initialises `voice: "M1"` (a
    // Supertonic style) and only replaces it when an engine is swapped
    // mid-session, so on a fresh launch with Fish already selected the
    // request arrived carrying "M1". Fish 404s on it, which maps to a
    // `voice` error, and the reader was told to "Choose a Fish Audio voice
    // in Settings" -- while a voice was chosen. Seeded explicitly here so
    // this test fails against that behaviour rather than passing by default.
    const engine = await loadEngine("d8ee9d1a-6f3e-4b8a-9c1d-abcdef012345");

    await engine.synthesize({ text: "Hello.", voice: "M1", speed: 1 });

    expect(synthesizeSpeech).toHaveBeenCalledTimes(1);
    const request = synthesizeSpeech.mock.calls[0][0];
    expect(request.voiceId).toBe("d8ee9d1a-6f3e-4b8a-9c1d-abcdef012345");
    expect(request.voiceId).not.toBe("M1");
  });

  it("falls back to the requested voice when the engine has none configured", async () => {
    // `ensureReady` normally refuses this state, so the fallback exists only
    // so an explicit per-call voice keeps working if that path is ever used.
    const engine = await loadEngine(null);

    await engine.synthesize({ text: "Hello.", voice: "voice-abc", speed: 1 });

    const request = synthesizeSpeech.mock.calls[0][0];
    expect(request.voiceId).toBe("voice-abc");
  });
});
