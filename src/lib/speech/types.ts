export type SpeechEngineId = "supertonic" | "fish";

export interface SpeechVoice {
  id: string;
  name: string;
  /** False when the voice still needs downloading before it can be used. */
  ready: boolean;
}

export interface SynthesisRequest {
  text: string;
  /**
   * A voice id this engine understands. The player carries one voice id across
   * engines, so an engine may be handed an id belonging to another one and is
   * expected to fall back rather than fail.
   */
  voice: string;
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
export interface SpeechEngine {
  readonly id: SpeechEngineId;
  /** Used when the current voice id belongs to a different engine. */
  readonly defaultVoice: string;

  synthesize(request: SynthesisRequest, signal?: AbortSignal): Promise<Blob>;

  /**
   * Make the engine usable: download what is missing, warm what is cold. Safe
   * to call repeatedly; cheap once ready.
   */
  ensureReady(onStatus?: (status: string) => void): Promise<void>;

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
