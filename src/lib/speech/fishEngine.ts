import { api } from "../tauri";
import { throwIfAborted, type SpeechEngine } from "./types";
import { speechAudioToBlob } from "./supertonicEngine";

/**
 * Fish Audio runs entirely in Rust, so this adapter only invokes. The API key
 * is never handed to the webview and so never appears here.
 */
export function createFishEngine(options: { voiceId: string | null }): SpeechEngine {
  return {
    id: "fish",
    // Fish has no built-in default voice; an unset one is an error the user
    // can act on, raised by Rust rather than guessed at here.
    defaultVoice: options.voiceId ?? "",

    async synthesize(request, signal) {
      throwIfAborted(signal);
      const speech = await api.synthesizeSpeech({
        provider: "fish",
        text: request.text,
        speed: request.speed,
        voiceId: request.voice || options.voiceId || "",
        language: null,
      });
      throwIfAborted(signal);
      return speechAudioToBlob(speech);
    },

    async ensureReady() {
      const status = await api.getFishKeyStatus();
      if (!status.present) {
        throw new Error("Add a Fish Audio API key in Settings to use this voice.");
      }
    },

    async listVoices() {
      return api.listFishVoices();
    },
  };
}
