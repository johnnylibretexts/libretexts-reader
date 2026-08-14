import { SUPERTONIC_VOICES, type SupertonicLanguage } from "../supertonic";
import { api, type SpeechAudio } from "../tauri";
import { throwIfAborted, type SpeechEngine } from "./types";

const SUPERTONIC_DEFAULT_VOICE = "M1";

/**
 * Turn the raw bytes a Tauri command returns into something playable.
 *
 * Exported because chapter export and the settings preview call their own
 * commands and need the same conversion; before this existed, the same three
 * lines were written out at each call site.
 */
export function speechAudioToBlob(speech: SpeechAudio): Blob {
  return new Blob([new Uint8Array(speech.audio)], {
    type: speech.mimeType || "audio/wav",
  });
}

/** Supertonic runs in Rust, behind the ONNX runtime. */
export function createSupertonicEngine(options: {
  language: SupertonicLanguage;
}): SpeechEngine {
  return {
    id: "supertonic",
    defaultVoice: SUPERTONIC_DEFAULT_VOICE,

    async synthesize(request, signal) {
      throwIfAborted(signal);
      const speech = await api.synthesizeSpeech({
        provider: "supertonic",
        text: request.text,
        speed: request.speed,
        voiceId: request.voice || SUPERTONIC_DEFAULT_VOICE,
        language: options.language,
      });
      throwIfAborted(signal);
      return speechAudioToBlob(speech);
    },

    async ensureReady(onStatus) {
      const status = await api.getSupertonicModelStatus();
      if (status.downloaded) {
        return;
      }

      onStatus?.("Downloading the Supertonic model...");
      await api.ensureSupertonicModelDownloaded();
    },

    async listVoices() {
      return SUPERTONIC_VOICES.map((voice) => ({
        id: voice.id,
        name: voice.name,
        ready: true,
      }));
    },
  };
}
