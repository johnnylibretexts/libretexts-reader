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
 * On cancellation: `signal` is honoured where it is cheap — work not yet
 * started is skipped, and a result that arrives after abort is discarded.
 * Synthesis already in flight cannot be aborted: the Rust command has no
 * cancellation channel, so the signal shortens nothing that has begun.
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
