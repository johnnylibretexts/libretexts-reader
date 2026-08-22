import { listen } from "@tauri-apps/api/event";
import { displayError } from "../errors";
import {
  SUPERTONIC_VOICES,
  type SupertonicLanguage,
  type SupertonicVoiceStyle,
} from "../supertonic";
import {
  api,
  isTauriRuntime,
  type SpeechAudio,
  type SupertonicModelProgress,
} from "../tauri";
import {
  SpeechAbortedError,
  throwIfAborted,
  type EngineStatus,
  type SettingsSource,
  type SpeechEngine,
} from "./types";

/**
 * What Rust fails a cancelled download with. Matched as a substring because it
 * reaches the webview wrapped in the command's error; it must not drift from
 * `MODEL_DOWNLOAD_CANCELLED` in `src-tauri/src/tts/supertonic/model.rs`.
 */
const MODEL_DOWNLOAD_CANCELLED = "Supertonic model download cancelled.";

/**
 * The one line shown alongside the bar. Deliberately says how long this lasts
 * and where the bytes come from -- it used to be the *only* thing the reader
 * was told for several minutes, and said neither.
 */
const DOWNLOADING_MESSAGE = "Downloading the on-device voice (one time)";

/**
 * Fetch the model, reporting every byte Rust announces.
 *
 * Split out of `ensureReady` because it owns a subscription with a lifetime:
 * the listener must not outlive the download, or a later Settings-side
 * download would keep reporting into a player that is not downloading
 * anything.
 */
async function downloadModel(onStatus?: (status: EngineStatus) => void) {
  const cancel = () => api.cancelSupertonicModelDownload();
  // Reported before the first byte arrives: on a slow connection the first
  // progress event is seconds away, and until then the reader would be looking
  // at the same blank spinner this exists to replace.
  onStatus?.({ message: DOWNLOADING_MESSAGE, cancel });

  const unlisten = await subscribeToModelProgress((progress) => {
    onStatus?.({
      message: DOWNLOADING_MESSAGE,
      download: {
        downloadedBytes: progress.downloaded,
        totalBytes: progress.total,
      },
      cancel,
    });
  });

  try {
    await api.ensureSupertonicModelDownloaded();
  } catch (error) {
    // A reader who pressed Cancel has not hit a problem. `SpeechAbortedError`
    // is what `speakWithBufferedSpeech` swallows silently; anything else it
    // shows, which here would be an error message for something they asked
    // for.
    if (displayError(error).includes(MODEL_DOWNLOAD_CANCELLED)) {
      throw new SpeechAbortedError();
    }
    throw error;
  } finally {
    unlisten?.();
  }
}

/** Null outside the desktop runtime, where there is no event bus to listen on. */
async function subscribeToModelProgress(
  onProgress: (progress: SupertonicModelProgress) => void,
) {
  if (!isTauriRuntime()) {
    return null;
  }
  return listen<SupertonicModelProgress>(
    "supertonic-model-download-progress",
    (event) => onProgress(event.payload),
  );
}

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

      await downloadModel(onStatus);
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
