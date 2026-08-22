import { afterEach, describe, expect, it, vi } from "vitest";
import { SpeechAbortedError } from "./types";
import type { EngineStatus, SettingsSource } from "./types";
import type { SupertonicVoiceStyle } from "../supertonic";
import type {
  SupertonicModelProgress,
  SynthesizeSpeechRequest,
} from "../tauri";

const getSupertonicModelStatus = vi.fn(async () => ({ downloaded: true }));
const ensureSupertonicModelDownloaded = vi.fn(async () => undefined);
const cancelSupertonicModelDownload = vi.fn(async () => undefined);
const synthesizeSpeech = vi.fn(async (_request: SynthesizeSpeechRequest) => ({
  audio: [1, 2, 3],
  mimeType: "audio/wav",
}));
let tauriRuntime = true;

vi.mock("../tauri", () => ({
  api: {
    synthesizeSpeech: (request: SynthesizeSpeechRequest) =>
      synthesizeSpeech(request),
    getSupertonicModelStatus: () => getSupertonicModelStatus(),
    ensureSupertonicModelDownloaded: () => ensureSupertonicModelDownloaded(),
    cancelSupertonicModelDownload: () => cancelSupertonicModelDownload(),
  },
  isTauriRuntime: () => tauriRuntime,
}));

type ProgressHandler = (event: { payload: SupertonicModelProgress }) => void;
let progressHandlers: ProgressHandler[] = [];
const unlisten = vi.fn();

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (_name: string, handler: ProgressHandler) => {
    progressHandlers.push(handler);
    return unlisten;
  }),
}));

/** Stand in for Rust emitting `supertonic-model-download-progress`. */
function emitProgress(payload: SupertonicModelProgress) {
  for (const handler of progressHandlers) {
    handler({ payload });
  }
}

afterEach(() => {
  synthesizeSpeech.mockClear();
  getSupertonicModelStatus.mockClear();
  getSupertonicModelStatus.mockResolvedValue({ downloaded: true });
  ensureSupertonicModelDownloaded.mockClear();
  ensureSupertonicModelDownloaded.mockImplementation(async () => undefined);
  cancelSupertonicModelDownload.mockClear();
  unlisten.mockClear();
  progressHandlers = [];
  tauriRuntime = true;
});

async function loadEngine(
  voiceStyle: SupertonicVoiceStyle,
  settingsSource: SettingsSource = "loaded",
) {
  const { createSupertonicEngine } = await import("./supertonicEngine");
  return createSupertonicEngine({ language: "en", voiceStyle, settingsSource });
}

describe("supertonic engine voice selection", () => {
  it("sends the reader's configured voice style, not the one the player carries", async () => {
    // The regression this guards: playback sent the literal "M1" -- the value
    // a shared player field was seeded with and that no component ever set --
    // whatever the reader had chosen. Export, Preview and Test all read the
    // setting directly and so appeared to work, leaving it ignored in the one
    // place it is actually used.
    const engine = await loadEngine("F3");

    await engine.synthesize({ text: "Hello.", speed: 1 });

    expect(synthesizeSpeech).toHaveBeenCalledTimes(1);
    const request = synthesizeSpeech.mock.calls[0][0];
    expect(request.voiceId).toBe("F3");
    expect(request.voiceId).not.toBe("M1");
  });

  it("reports as its default voice the one it actually synthesizes with", async () => {
    // `SpeechEngine.defaultVoice` is what playback persists as the reader's
    // voice, so an engine whose `defaultVoice` disagrees with what it sends
    // records a voice nobody heard. Asserted against the same call rather
    // than a literal, so the two cannot drift apart.
    const engine = await loadEngine("F3");

    await engine.synthesize({ text: "Hello.", speed: 1 });

    expect(engine.defaultVoice).toBe("F3");
    expect(synthesizeSpeech.mock.calls[0][0].voiceId).toBe(engine.defaultVoice);
  });

  it("will not start the model download when the reader's provider is a guess", async () => {
    // A failed settings load falls back to Supertonic, so playback can end up
    // here for a reader who uses Fish exclusively. Downloading ~383MB from
    // huggingface.co on that guess is not a cosmetic wrong-voice problem: it
    // is network and disk the reader never agreed to, in an app that
    // otherwise puts that download behind an explicit button.
    getSupertonicModelStatus.mockResolvedValue({ downloaded: false });
    const engine = await loadEngine("M1", "failed");

    await expect(engine.ensureReady()).rejects.toThrow(
      /could not be loaded/i,
    );
    expect(ensureSupertonicModelDownloaded).not.toHaveBeenCalled();
  });

  it("says settings are still loading, not that they failed, before hydration", async () => {
    // The two refusals need different words: nothing is broken while the load
    // is merely in flight, and Settings shows no banner and no "Try again" to
    // act on -- so pointing the reader there is an instruction they cannot
    // follow.
    getSupertonicModelStatus.mockResolvedValue({ downloaded: false });
    const engine = await loadEngine("M1", "unloaded");

    await expect(engine.ensureReady()).rejects.toThrow(/still loading/i);
    expect(ensureSupertonicModelDownloaded).not.toHaveBeenCalled();
  });

  it("still downloads the model when the provider is the reader's own", async () => {
    getSupertonicModelStatus.mockResolvedValue({ downloaded: false });
    const engine = await loadEngine("M1");

    await engine.ensureReady();

    expect(ensureSupertonicModelDownloaded).toHaveBeenCalled();
  });
});

describe("supertonic model download progress", () => {
  /** Run `ensureReady` against a missing model, collecting what it reports. */
  async function downloadReporting(
    duringDownload: () => void,
  ): Promise<EngineStatus[]> {
    getSupertonicModelStatus.mockResolvedValue({ downloaded: false });
    ensureSupertonicModelDownloaded.mockImplementation(async () => {
      duringDownload();
      return undefined;
    });
    const engine = await loadEngine("M1");
    const statuses: EngineStatus[] = [];

    await engine.ensureReady((status) => statuses.push(status));

    return statuses;
  }

  it("reports the bytes as they arrive, not one static line", async () => {
    // The whole reason #52 read as a hang: the only thing the player was ever
    // told was "Downloading the Supertonic model...", for the several minutes
    // it takes to pull 383MB, with every control disabled behind it.
    const statuses = await downloadReporting(() => {
      emitProgress({ file: "model.onnx", downloaded: 100, total: 400 });
      emitProgress({ file: "model.onnx", downloaded: 300, total: 400 });
    });

    const progress = statuses
      .map((status) => status.download)
      .filter((download) => download !== undefined);
    expect(progress).toEqual([
      { downloadedBytes: 100, totalBytes: 400 },
      { downloadedBytes: 300, totalBytes: 400 },
    ]);
  });

  it("offers a cancel that actually stops the download", async () => {
    const statuses = await downloadReporting(() => {
      emitProgress({ file: "model.onnx", downloaded: 100, total: 400 });
    });

    const cancel = statuses[statuses.length - 1]?.cancel;
    expect(cancel).toBeTypeOf("function");
    await cancel?.();
    expect(cancelSupertonicModelDownload).toHaveBeenCalledTimes(1);
  });

  it("stops listening once the download is over", async () => {
    // The listener outliving the download would keep a later Settings-side
    // download writing into a player that is not downloading anything.
    await downloadReporting(() => {
      emitProgress({ file: "model.onnx", downloaded: 100, total: 400 });
    });

    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("treats a cancelled download as an abort, not a failure to show", async () => {
    // `speakWithBufferedSpeech` swallows `SpeechAbortedError` silently and
    // shows anything else as an error. A reader who pressed Cancel has not
    // hit a problem and must not be told they have.
    getSupertonicModelStatus.mockResolvedValue({ downloaded: false });
    ensureSupertonicModelDownloaded.mockImplementation(async () => {
      throw new Error("model error: Supertonic model download cancelled.");
    });
    const engine = await loadEngine("M1");

    await expect(engine.ensureReady()).rejects.toBeInstanceOf(
      SpeechAbortedError,
    );
  });

  it("still reports a real download failure", async () => {
    getSupertonicModelStatus.mockResolvedValue({ downloaded: false });
    ensureSupertonicModelDownloaded.mockImplementation(async () => {
      throw new Error("model error: download SHA-256 mismatch");
    });
    const engine = await loadEngine("M1");

    await expect(engine.ensureReady()).rejects.toThrow(/SHA-256/);
  });
});
