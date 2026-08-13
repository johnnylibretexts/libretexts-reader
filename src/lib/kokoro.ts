import { readFile } from "@tauri-apps/plugin-fs";
import type { ModelPrecision } from "../stores/settings";
import { api, isTauriRuntime } from "./tauri";

const KOKORO_MODEL_ID = "onnx-community/Kokoro-82M-v1.0-ONNX";
const KOKORO_MODEL_REMOTE_BASE =
  "https://huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX/resolve/main/onnx";
const KOKORO_LOAD_TIMEOUT_MS = 120_000;
const KOKORO_GENERATE_TIMEOUT_MS = 60_000;

type KokoroDtype = "fp32" | "q8";

interface KokoroAudio {
  toBlob: () => Blob;
}

interface KokoroEngine {
  generate: (
    text: string,
    options?: { voice?: string; speed?: number },
  ) => Promise<KokoroAudio>;
}

interface KokoroModule {
  KokoroTTS: {
    from_pretrained: (
      modelId: string,
      options?: {
        dtype?: KokoroDtype;
        device?: "wasm" | "webgpu" | "cpu" | null;
        progress_callback?: (progress: KokoroProgress) => void;
      },
    ) => Promise<KokoroEngine>;
  };
}

interface KokoroProgress {
  status?: string;
  file?: string;
  progress?: number;
  loaded?: number;
  total?: number;
}

let enginePromise: Promise<KokoroEngine> | null = null;
let loadedDtype: KokoroDtype | null = null;
let fetchInterceptorInstalled = false;
const nativeModelPaths = new Map<string, string>();

export async function synthesizeKokoroSpeech({
  text,
  speed,
  voiceId,
  precision,
  onStatus,
}: {
  text: string;
  speed: number;
  voiceId: string;
  precision: ModelPrecision;
  onStatus?: (status: string) => void;
}) {
  const engine = await loadKokoroEngine(precision, onStatus);
  onStatus?.("Generating Kokoro audio...");
  const audio = await withTimeout(
    engine.generate(text, {
      voice: voiceId || "af_heart",
      speed: clamp(speed, 0.5, 2),
    }),
    KOKORO_GENERATE_TIMEOUT_MS,
    "Kokoro speech generation timed out.",
  );
  return audio.toBlob();
}

/**
 * Load and cache the engine without synthesizing anything, so a caller can pay
 * the model-load cost up front and report progress while it happens.
 */
export async function ensureKokoroReady(
  precision: ModelPrecision,
  onStatus?: (status: string) => void,
) {
  await loadKokoroEngine(precision, onStatus);
}

async function loadKokoroEngine(
  precision: ModelPrecision,
  onStatus?: (status: string) => void,
) {
  const dtype = precision === "q8" ? "q8" : "fp32";
  if (!enginePromise || loadedDtype !== dtype) {
    loadedDtype = dtype;
    enginePromise = withTimeout(
      (async () => {
        onStatus?.("Preparing local Kokoro model...");
        await prepareNativeModelSource(precision, onStatus);
        onStatus?.("Loading Kokoro model...");
        const { KokoroTTS } = (await import("kokoro-js")) as KokoroModule;
        return KokoroTTS.from_pretrained(KOKORO_MODEL_ID, {
          dtype,
          device: "wasm",
          progress_callback: (progress) =>
            onStatus?.(formatKokoroProgress(progress)),
        });
      })(),
      KOKORO_LOAD_TIMEOUT_MS,
      "Kokoro model load timed out. Check your network and try the q8 model.",
    ).catch((error) => {
      enginePromise = null;
      loadedDtype = null;
      throw error;
    });
  }
  return enginePromise;
}

async function prepareNativeModelSource(
  precision: ModelPrecision,
  onStatus?: (status: string) => void,
) {
  if (!isTauriRuntime()) {
    return;
  }

  const remoteUrl = modelRemoteUrl(precision);
  const modelPath = await api.getModelPath(precision);
  nativeModelPaths.set(remoteUrl, modelPath);
  installFetchInterceptor();
  onStatus?.("Local Kokoro model ready.");
}

function installFetchInterceptor() {
  if (fetchInterceptorInstalled || typeof window === "undefined") {
    return;
  }

  fetchInterceptorInstalled = true;
  const nativeFetch = window.fetch.bind(window);
  window.fetch = async (input, init) => {
    const url =
      typeof input === "string"
        ? input
        : input instanceof URL
          ? input.toString()
          : input.url;
    const modelPath = nativeModelPaths.get(url);
    if (modelPath) {
      return binaryResponse(await readKokoroModelFile(modelPath));
    }
    return nativeFetch(input, init);
  };
}

async function readKokoroModelFile(modelPath: string) {
  try {
    return await readFile(modelPath);
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error);
    throw new Error(
      `Kokoro model file is missing or unreadable at ${modelPath}. Download the Kokoro model again. ${detail}`,
    );
  }
}

function binaryResponse(bytes: Uint8Array) {
  return new Response(bytes, {
    status: 200,
    headers: {
      "content-length": String(bytes.byteLength),
      "content-type": "application/octet-stream",
    },
  });
}

function modelRemoteUrl(precision: ModelPrecision) {
  const fileName = precision === "q8" ? "model_quantized.onnx" : "model.onnx";
  return `${KOKORO_MODEL_REMOTE_BASE}/${fileName}`;
}

function formatKokoroProgress(progress: KokoroProgress) {
  if (progress.status === "progress" && progress.file) {
    const percent =
      typeof progress.progress === "number"
        ? ` ${Math.round(progress.progress)}%`
        : "";
    return `Loading ${progress.file}${percent}`;
  }
  if (progress.status === "ready") {
    return "Kokoro model ready.";
  }
  if (progress.status === "done" && progress.file) {
    return `Loaded ${progress.file}`;
  }
  if (progress.file) {
    return `Loading ${progress.file}`;
  }
  return "Loading Kokoro model...";
}

function withTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
  message: string,
) {
  let timeoutId: number | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timeoutId = window.setTimeout(() => reject(new Error(message)), timeoutMs);
  });

  return Promise.race([promise, timeout]).finally(() => {
    if (timeoutId !== undefined) {
      window.clearTimeout(timeoutId);
    }
  });
}

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}
