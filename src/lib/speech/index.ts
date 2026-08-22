import type { SupertonicLanguage, SupertonicVoiceStyle } from "../supertonic";
import { createFishEngine } from "./fishEngine";
import { createSupertonicEngine } from "./supertonicEngine";
import type { SettingsSource, SpeechEngine, SpeechEngineId } from "./types";

export { createSupertonicEngine, speechAudioToBlob } from "./supertonicEngine";
export { createFishEngine } from "./fishEngine";
export { createFakeEngine, type FakeEngine } from "./fakeEngine";
export {
  SPEECH_ENGINE_LABELS,
  SpeechAbortedError,
  throwIfAborted,
  type EngineStatus,
  type SettingsSource,
  type SpeechEngine,
  type SpeechEngineId,
  type SpeechVoice,
  type SynthesisRequest,
} from "./types";

export interface SpeechEngineSettings {
  ttsProvider: SpeechEngineId;
  supertonicLanguage: SupertonicLanguage;
  supertonicVoiceStyle: SupertonicVoiceStyle;
  fishVoiceId: string | null;
  /** See `SettingsSource`. */
  settingsSource: SettingsSource;
}

/**
 * The one place the app decides which engine speaks.
 *
 * Everything downstream holds a `SpeechEngine` and never sees the provider
 * string again. Adding an engine means adding a case here and nowhere else.
 */
export function createSpeechEngine(settings: SpeechEngineSettings): SpeechEngine {
  switch (settings.ttsProvider) {
    case "supertonic":
      return createSupertonicEngine({
        language: settings.supertonicLanguage,
        voiceStyle: settings.supertonicVoiceStyle,
        settingsSource: settings.settingsSource,
      });
    case "fish":
      return createFishEngine({ voiceId: settings.fishVoiceId });
  }
}
