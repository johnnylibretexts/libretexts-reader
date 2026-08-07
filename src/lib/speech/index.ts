import type { SupertonicLanguage } from "../supertonic";
import type { ModelPrecision } from "../../stores/settings";
import { createKokoroEngine } from "./kokoroEngine";
import { createSupertonicEngine } from "./supertonicEngine";
import type { SpeechEngine, SpeechEngineId } from "./types";

export { createKokoroEngine } from "./kokoroEngine";
export { createSupertonicEngine, speechAudioToBlob } from "./supertonicEngine";
export { createFakeEngine, type FakeEngine } from "./fakeEngine";
export {
  SpeechAbortedError,
  throwIfAborted,
  type SpeechEngine,
  type SpeechEngineId,
  type SpeechVoice,
  type SynthesisRequest,
} from "./types";

export interface SpeechEngineSettings {
  ttsProvider: SpeechEngineId;
  modelPrecision: ModelPrecision;
  supertonicLanguage: SupertonicLanguage;
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
      return createSupertonicEngine({ language: settings.supertonicLanguage });
    case "kokoro":
      return createKokoroEngine({ precision: settings.modelPrecision });
  }
}
