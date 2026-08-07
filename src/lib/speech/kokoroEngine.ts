import type { ModelPrecision } from "../../stores/settings";
import { ensureKokoroReady, synthesizeKokoroSpeech } from "../kokoro";
import { api } from "../tauri";
import { throwIfAborted, type SpeechEngine } from "./types";

const KOKORO_DEFAULT_VOICE = "af_heart";

/** Kokoro runs in the webview. See ADR-0001 for why it is not in Rust. */
export function createKokoroEngine(options: {
  precision: ModelPrecision;
}): SpeechEngine {
  return {
    id: "kokoro",
    defaultVoice: KOKORO_DEFAULT_VOICE,

    async synthesize(request, signal) {
      throwIfAborted(signal);
      const blob = await synthesizeKokoroSpeech({
        text: request.text,
        speed: request.speed,
        voiceId: request.voice || KOKORO_DEFAULT_VOICE,
        precision: options.precision,
      });
      throwIfAborted(signal);
      return blob;
    },

    async ensureReady(onStatus) {
      await api.ensureModelDownloaded(options.precision);
      await ensureKokoroReady(options.precision, onStatus);
    },

    async listVoices() {
      const voices = await api.listVoices();
      return voices.map((voice) => ({
        id: voice.id,
        name: voice.displayName,
        ready: voice.isBundled || voice.isDownloaded,
      }));
    },
  };
}
