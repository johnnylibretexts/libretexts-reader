import {
  throwIfAborted,
  type EngineStatus,
  type SpeechEngine,
  type SpeechEngineId,
  type SynthesisRequest,
} from "./types";

export interface FakeEngine extends SpeechEngine {
  /** Every synthesize call, in order. */
  readonly calls: SynthesisRequest[];
  readonly readyCalls: number;
  /** Hold the next synthesize open until the returned function is called. */
  blockNextSynthesis(): () => void;
  /**
   * Fail every synthesis until called with null. Sticky rather than one-shot
   * because the player prefetches ahead of the sentence it is speaking, so a
   * single-use failure would be absorbed by a prefetch that never surfaces it.
   */
  failSynthesis(error: Error | null): void;
  /**
   * What `ensureReady` reports before it resolves — what a real engine emits
   * while fetching something large. Reported before the gate below, so a
   * blocked readying can be inspected mid-download.
   */
  reportWhileReadying(...statuses: EngineStatus[]): void;
  /** Hold the next ensureReady open until the returned function is called. */
  blockNextReady(): () => void;
  /** Fail every ensureReady until called with null. */
  failReady(error: Error | null): void;
}

/**
 * A SpeechEngine for tests. Everything real engines do over WASM, ONNX or the
 * network, this does synchronously in memory — which is the point of having the
 * interface at all.
 */
export function createFakeEngine(
  options: { id?: SpeechEngineId; voices?: string[] } = {},
): FakeEngine {
  const id = options.id ?? "supertonic";
  const voiceIds = options.voices ?? ["fake-voice-1", "fake-voice-2"];
  const calls: SynthesisRequest[] = [];

  let readyCalls = 0;
  let release: (() => void) | null = null;
  let gate: Promise<void> | null = null;
  let nextError: Error | null = null;
  let readyStatuses: EngineStatus[] = [];
  let readyRelease: (() => void) | null = null;
  let readyGate: Promise<void> | null = null;
  let readyError: Error | null = null;

  return {
    id,
    defaultVoice: voiceIds[0],
    calls,

    get readyCalls() {
      return readyCalls;
    },

    blockNextSynthesis() {
      gate = new Promise<void>((resolve) => {
        release = resolve;
      });
      return () => {
        release?.();
        gate = null;
        release = null;
      };
    },

    failSynthesis(error: Error | null) {
      nextError = error;
    },

    reportWhileReadying(...statuses: EngineStatus[]) {
      readyStatuses = statuses;
    },

    failReady(error: Error | null) {
      readyError = error;
    },

    blockNextReady() {
      readyGate = new Promise<void>((resolve) => {
        readyRelease = resolve;
      });
      return () => {
        readyRelease?.();
        readyGate = null;
        readyRelease = null;
      };
    },

    async synthesize(request, signal) {
      throwIfAborted(signal);
      calls.push({ ...request });

      if (gate) {
        await gate;
      }
      if (nextError) {
        throw nextError;
      }

      throwIfAborted(signal);
      // Content differs per request so tests can tell two results apart.
      return new Blob([`audio:${request.speed}:${request.text}`], {
        type: "audio/wav",
      });
    },

    async ensureReady(onStatus) {
      readyCalls += 1;
      for (const status of readyStatuses) {
        onStatus?.(status);
      }
      if (readyGate) {
        await readyGate;
      }
      if (readyError) {
        throw readyError;
      }
    },

    async listVoices() {
      return voiceIds.map((voiceId) => ({ id: voiceId, name: voiceId, ready: true }));
    },
  };
}
