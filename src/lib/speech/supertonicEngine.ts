import {
  SUPERTONIC_VOICES,
  type SupertonicLanguage,
  type SupertonicVoiceStyle,
} from "../supertonic";
import { api, type SpeechAudio } from "../tauri";
import {
  throwIfAborted,
  type SettingsSource,
  type SpeechEngine,
} from "./types";

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
  /**
   * The reader's configured voice style. Deliberately the closed union and
   * not `string`: Rust's `playback_voice_style` substitutes "M1" for anything
   * it does not recognise rather than erroring, so an unknown style would be
   * silently ignored exactly the way this whole setting used to be.
   */
  voiceStyle: SupertonicVoiceStyle;
  /**
   * Where the settings that chose this engine came from.
   *
   * Anything but `loaded` means DEFAULT_SETTINGS stood in and the provider is
   * a guess -- Supertonic is what that guess lands on, so a reader who only
   * ever uses Fish can end up here. `ensureReady` refuses to fetch the model
   * then: ~383MB from huggingface.co on a guess is network and disk they
   * never agreed to, in an app that otherwise puts that download behind an
   * explicit button in Settings.
   */
  settingsSource: SettingsSource;
}): SpeechEngine {
  return {
    id: "supertonic",
    defaultVoice: options.voiceStyle,

    async synthesize(request, signal) {
      throwIfAborted(signal);
      const speech = await api.synthesizeSpeech({
        provider: "supertonic",
        text: request.text,
        speed: request.speed,
        // The reader's configured style. Playback used to send whatever the
        // player carried in a shared `voice` field, which was seeded "M1" and
        // that no component ever set, so every request said "M1" no matter
        // what the reader picked. Export, Preview and Test read the setting
        // themselves and so were unaffected -- it worked everywhere except
        // the one place a reader listens.
        voiceId: options.voiceStyle,
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

      if (options.settingsSource !== "loaded") {
        // Worded for which of the two it is: "unloaded" clears itself in a
        // moment and Settings shows nothing to retry, so telling the reader
        // to go there would be an instruction they cannot follow.
        throw new Error(
          options.settingsSource === "unloaded"
            ? "Your settings are still loading, so this may not be the voice engine you chose. Try again in a moment."
            : "Your settings could not be loaded, so this may not be the voice engine you chose. Retry loading them in Settings before downloading the Supertonic model.",
        );
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
