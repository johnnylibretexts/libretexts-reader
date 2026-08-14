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
        // The configured voice wins over `request.voice`. The player carries
        // ONE voice id across engines (see SynthesisRequest.voice) and seeds
        // it with Supertonic's "M1", which it only replaces when an engine is
        // swapped mid-session -- never on a fresh launch. So on every launch
        // with Fish already selected, `request.voice` is "M1", Fish answers
        // 404, and the reader is told to choose a voice they had already
        // chosen. `request.voice` is still honoured when this engine has no
        // voice of its own to prefer, so an explicit per-call voice keeps
        // working if that path is ever used.
        voiceId: options.voiceId || request.voice || "",
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
