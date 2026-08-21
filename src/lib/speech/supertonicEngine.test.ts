import { afterEach, describe, expect, it, vi } from "vitest";
import type { SettingsSource } from "./types";
import type { SupertonicVoiceStyle } from "../supertonic";
import type { SynthesizeSpeechRequest } from "../tauri";

const getSupertonicModelStatus = vi.fn(async () => ({ downloaded: true }));
const ensureSupertonicModelDownloaded = vi.fn(async () => undefined);
const synthesizeSpeech = vi.fn(async (_request: SynthesizeSpeechRequest) => ({
  audio: [1, 2, 3],
  mimeType: "audio/wav",
}));

vi.mock("../tauri", () => ({
  api: {
    synthesizeSpeech: (request: SynthesizeSpeechRequest) =>
      synthesizeSpeech(request),
    getSupertonicModelStatus: () => getSupertonicModelStatus(),
    ensureSupertonicModelDownloaded: () => ensureSupertonicModelDownloaded(),
  },
  isTauriRuntime: () => false,
}));

afterEach(() => {
  synthesizeSpeech.mockClear();
  getSupertonicModelStatus.mockClear();
  getSupertonicModelStatus.mockResolvedValue({ downloaded: true });
  ensureSupertonicModelDownloaded.mockClear();
});

async function loadEngine(
  voiceStyle: SupertonicVoiceStyle,
  settingsSource: SettingsSource = "loaded",
) {
  const { createSupertonicEngine } = await import("./supertonicEngine");
  return createSupertonicEngine({ language: "en", voiceStyle, settingsSource });
}

describe("supertonic engine voice selection", () => {
  it("sends the reader's configured voice style, not the one the player carries", async () => {
    // The regression this guards: playback sent the literal "M1" -- the value
    // a shared player field was seeded with and that no component ever set --
    // whatever the reader had chosen. Export, Preview and Test all read the
    // setting directly and so appeared to work, leaving it ignored in the one
    // place it is actually used.
    const engine = await loadEngine("F3");

    await engine.synthesize({ text: "Hello.", speed: 1 });

    expect(synthesizeSpeech).toHaveBeenCalledTimes(1);
    const request = synthesizeSpeech.mock.calls[0][0];
    expect(request.voiceId).toBe("F3");
    expect(request.voiceId).not.toBe("M1");
  });

  it("reports as its default voice the one it actually synthesizes with", async () => {
    // `SpeechEngine.defaultVoice` is what playback persists as the reader's
    // voice, so an engine whose `defaultVoice` disagrees with what it sends
    // records a voice nobody heard. Asserted against the same call rather
    // than a literal, so the two cannot drift apart.
    const engine = await loadEngine("F3");

    await engine.synthesize({ text: "Hello.", speed: 1 });

    expect(engine.defaultVoice).toBe("F3");
    expect(synthesizeSpeech.mock.calls[0][0].voiceId).toBe(engine.defaultVoice);
  });

  it("will not start the model download when the reader's provider is a guess", async () => {
    // A failed settings load falls back to Supertonic, so playback can end up
    // here for a reader who uses Fish exclusively. Downloading ~383MB from
    // huggingface.co on that guess is not a cosmetic wrong-voice problem: it
    // is network and disk the reader never agreed to, in an app that
    // otherwise puts that download behind an explicit button.
    getSupertonicModelStatus.mockResolvedValue({ downloaded: false });
    const engine = await loadEngine("M1", "failed");

    await expect(engine.ensureReady()).rejects.toThrow(
      /could not be loaded/i,
    );
    expect(ensureSupertonicModelDownloaded).not.toHaveBeenCalled();
  });

  it("says settings are still loading, not that they failed, before hydration", async () => {
    // The two refusals need different words: nothing is broken while the load
    // is merely in flight, and Settings shows no banner and no "Try again" to
    // act on -- so pointing the reader there is an instruction they cannot
    // follow.
    getSupertonicModelStatus.mockResolvedValue({ downloaded: false });
    const engine = await loadEngine("M1", "unloaded");

    await expect(engine.ensureReady()).rejects.toThrow(/still loading/i);
    expect(ensureSupertonicModelDownloaded).not.toHaveBeenCalled();
  });

  it("still downloads the model when the provider is the reader's own", async () => {
    getSupertonicModelStatus.mockResolvedValue({ downloaded: false });
    const engine = await loadEngine("M1");

    await engine.ensureReady();

    expect(ensureSupertonicModelDownloaded).toHaveBeenCalled();
  });
});
