import { describe, expect, it } from "vitest";

import {
  SUPERTONIC_LANGUAGES,
  supertonicSampleText,
  type SupertonicLanguage,
} from "./supertonic";

describe("Supertonic language samples", () => {
  it("gives every language its own sample sentence", () => {
    // The Settings Test button used to speak one hardcoded English string
    // whatever language was selected, so auditioning Spanish demonstrated
    // English read with Spanish phonology and nothing else. A sample per
    // language is what makes that control tell the truth -- so a shared or
    // missing sentence is the bug, not a tidiness problem.
    const samples = SUPERTONIC_LANGUAGES.map((option) => option.sample);

    expect(samples.every((sample) => sample.trim().length > 0)).toBe(true);
    expect(new Set(samples).size).toBe(SUPERTONIC_LANGUAGES.length);
  });

  it("speaks the selected language, not English", () => {
    expect(supertonicSampleText("es")).toBe(
      "Esta es una prueba de voz de LibreTexts Reader.",
    );
    expect(supertonicSampleText("es")).not.toBe(supertonicSampleText("en"));
  });

  it("falls back to English for a language the list no longer has", () => {
    // Same posture as `normalize_language` in the Rust engine: a settings row
    // written by an older release must not leave Test with nothing to say.
    expect(supertonicSampleText("klingon" as SupertonicLanguage)).toBe(
      supertonicSampleText("en"),
    );
  });
});
