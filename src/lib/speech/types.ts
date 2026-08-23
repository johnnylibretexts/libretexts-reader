export type SpeechEngineId = "supertonic" | "fish";

/**
 * Where the settings an engine was built from came from.
 *
 * `loaded` is the reader's own rows. The other two are DEFAULT_SETTINGS
 * standing in -- still in flight, or a load that failed -- which means the
 * provider is a guess, and an engine must not answer a guess by fetching a
 * ~383MB model. Three states rather than a boolean because the two failures
 * need different things said to the reader: one resolves on its own, the
 * other needs the retry in Settings.
 */
export type SettingsSource = "loaded" | "unloaded" | "failed";

/**
 * Human-readable names for user-facing strings (buffering status, export
 * gates, settings). The single place this mapping lives, so a label can
 * never drift out of sync with which engine is actually running -- see the
 * bug this replaced, where `player.ts` hardcoded "Supertonic" into the
 * buffering message regardless of `ttsProvider`.
 */
export const SPEECH_ENGINE_LABELS: Record<SpeechEngineId, string> = {
  supertonic: "Supertonic",
  fish: "Fish Audio",
};

/**
 * Whether an engine charges the reader for each sentence it speaks.
 *
 * Keyed by id rather than living only on the engine, because the screen that
 * *chooses* a provider has to ask before any engine exists. `SpeechEngine.bills`
 * reads from this same record, so a picker and a running engine can never
 * disagree about whether money is involved.
 *
 * This changes behaviour, not just copy: a billing engine reads far less far
 * ahead, because every prefetched sentence is spent whether or not the reader
 * ever hears it.
 */
export const SPEECH_ENGINE_BILLS: Record<SpeechEngineId, boolean> = {
  supertonic: false,
  fish: true,
};

/**
 * The container each engine's chapter export arrives in.
 *
 * Not cosmetic: it names the file the reader receives. Supertonic encodes
 * locally through macOS AudioToolbox since the LGPL LAME dependency was
 * dropped, and macOS ships no MP3 *encoder*, so its exports are AAC in an M4A
 * container. Fish returns MP3 from its API and is left alone -- re-encoding
 * lossy audio to make the extensions match would cost quality for nothing.
 *
 * Mirrors `export_extension` in `src-tauri/src/tts/provider.rs`; the two must
 * agree or a button promises a file the backend does not write. See ADR-0004.
 */
export const SPEECH_ENGINE_EXPORT_FORMAT: Record<SpeechEngineId, string> = {
  supertonic: "M4A",
  fish: "MP3",
};

export interface SpeechVoice {
  id: string;
  name: string;
  /** False when the voice still needs downloading before it can be used. */
  ready: boolean;
}

/**
 * Deliberately carries no voice. Every engine takes its voice from settings
 * and exposes it as `defaultVoice`, so a `voice` here would be a field the
 * two existing engines ignore -- and a trap for a third one written against
 * it, whose voice playback would then silently drop.
 */
export interface SynthesisRequest {
  text: string;
  speed: number;
}

/**
 * Everything the app needs from a way of turning text into speech.
 *
 * Callers never name an engine. They hold a `SpeechEngine` and use it; which
 * concrete engine it is has already been decided, once, by `createSpeechEngine`.
 *
 * On cancellation: `signal` prevents work from *starting*. Synthesis already
 * in flight cannot be aborted — the Rust command has no cancellation channel,
 * so the signal shortens nothing that has begun.
 *
 * A result that arrives after abort is therefore deliberately NOT discarded.
 * For a billing engine the request has already been charged, and throwing the
 * blob away only means buying the same sentence again on the next Play. The
 * player decides what to *play* from the utterance token; keeping the audio
 * costs a cache slot and saves real money.
 */
/**
 * What an engine reports while making itself ready.
 *
 * Richer than the plain string it used to be because "getting ready" covers
 * two very different waits: warming something already on disk, over in a
 * second, and Supertonic's one-time ~383MB fetch from huggingface.co, which
 * takes minutes. Told apart only by a message, the player rendered both as the
 * same indeterminate spinner behind the same disabled controls -- which is
 * what made the second read as a hung app.
 */
export interface EngineStatus {
  /** One line, already written for a reader. */
  message: string;
  /**
   * Set only while something large is being fetched, and only by an engine
   * that knows both numbers. Its presence is what lets the player show a real
   * bar instead of a spinner, so do not fill it in with guesses.
   */
  download?: { downloadedBytes: number; totalBytes: number };
  /**
   * Present only while a step the reader is allowed to abandon is running.
   * Resolving it does not mean the step has stopped -- it means the request to
   * stop was delivered; the step itself fails shortly after, as an abort.
   */
  cancel?: () => Promise<void>;
}

export interface SpeechEngine {
  readonly id: SpeechEngineId;
  /** See `SPEECH_ENGINE_BILLS`. */
  readonly bills: boolean;
  /**
   * The voice this engine actually speaks in, as configured.
   *
   * Not a fallback, despite the name. It is the only account anything outside
   * the engine has of what it is speaking in: `persistPlaybackState` in
   * `stores/player.ts` writes it as the reader's voice, and `player.ts` seeds
   * nothing else. So it must name the voice `synthesize` really uses -- an
   * engine that returns a placeholder here records that placeholder as what
   * the reader heard, with nothing to catch it.
   *
   * The empty string is the one honest exception: an engine with no voice
   * configured yet, which `ensureReady` then refuses (see `fishEngine`).
   */
  readonly defaultVoice: string;

  synthesize(request: SynthesisRequest, signal?: AbortSignal): Promise<Blob>;

  /**
   * Make the engine usable: download what is missing, warm what is cold. Safe
   * to call repeatedly; cheap once ready.
   */
  ensureReady(onStatus?: (status: EngineStatus) => void): Promise<void>;

  listVoices(): Promise<SpeechVoice[]>;
}

export class SpeechAbortedError extends Error {
  constructor() {
    super("Speech synthesis was cancelled.");
    this.name = "SpeechAbortedError";
  }
}

export function throwIfAborted(signal?: AbortSignal) {
  if (signal?.aborted) {
    throw new SpeechAbortedError();
  }
}
