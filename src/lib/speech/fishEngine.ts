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
        // The reader's configured voice, and only that. Playback used to send
        // whatever the player carried in a shared `voice` field, seeded with
        // Supertonic's "M1" -- so on every launch with Fish already selected
        // the request said "M1", Fish answered 404, and the reader was told
        // to choose a voice they had already chosen. `ensureReady` refuses an
        // empty one before it can get here.
        voiceId: options.voiceId ?? "",
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
      if (!options.voiceId) {
        // Without this check, synthesize() sends voiceId: "" and the failure
        // surfaces as an opaque Rust-side error instead of this actionable one.
        throw new Error("Choose a Fish Audio voice in Settings to use this voice.");
      }
    },

    async listVoices() {
      return api.listFishVoices();
    },
  };
}
