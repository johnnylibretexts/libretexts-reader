import { describe, expect, it } from "vitest";
import { createSpeechEngine } from "./index";

describe("createSpeechEngine", () => {
  it("returns the Supertonic engine when the provider says supertonic", () => {
    const engine = createSpeechEngine({
      ttsProvider: "supertonic",
      supertonicLanguage: "en",
      supertonicVoiceStyle: "M1",
      fishVoiceId: null,
      settingsSource: "loaded",
    });
    expect(engine.id).toBe("supertonic");
  });

  it("hands the Supertonic engine the reader's configured voice style", () => {
    // The wiring this guards: the style is a settings row, and playback is
    // the only path that does not read it directly. If it stops reaching
    // `createSupertonicEngine`, every playback request silently reverts to
    // the seeded "M1" while Export, Preview and Test keep working.
    const engine = createSpeechEngine({
      ttsProvider: "supertonic",
      supertonicLanguage: "en",
      supertonicVoiceStyle: "F3",
      fishVoiceId: null,
      settingsSource: "loaded",
    });
    expect(engine.defaultVoice).toBe("F3");
  });

  it("returns the Fish engine when the provider says fish", () => {
    const engine = createSpeechEngine({
      ttsProvider: "fish",
      supertonicLanguage: "en",
      supertonicVoiceStyle: "M1",
      fishVoiceId: "voice-abc",
      settingsSource: "loaded",
    });
    expect(engine.id).toBe("fish");
  });
});
