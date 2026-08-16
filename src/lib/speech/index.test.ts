import { describe, expect, it } from "vitest";
import { createSpeechEngine } from "./index";

describe("createSpeechEngine", () => {
  it("returns the Supertonic engine when the provider says supertonic", () => {
    const engine = createSpeechEngine({
      ttsProvider: "supertonic",
      supertonicLanguage: "en",
      fishVoiceId: null,
    });
    expect(engine.id).toBe("supertonic");
  });

  it("returns the Fish engine when the provider says fish", () => {
    const engine = createSpeechEngine({
      ttsProvider: "fish",
      supertonicLanguage: "en",
      fishVoiceId: "voice-abc",
    });
    expect(engine.id).toBe("fish");
  });
});
