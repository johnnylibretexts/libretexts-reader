import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Check, Download, Loader2, Play, Save } from "lucide-react";
import {
  api,
  type FishKeyStatus,
  type SupertonicModelProgress,
  type SupertonicModelStatus,
  isTauriRuntime,
} from "../../lib/tauri";
import { displayError } from "../../lib/errors";
import { createSpeechEngine } from "../../lib/speech";
import {
  SUPERTONIC_LANGUAGES,
  SUPERTONIC_VOICES,
  type SupertonicLanguage,
  type SupertonicVoiceStyle,
} from "../../lib/supertonic";
import { useSettingsStore, type TtsProvider } from "../../stores/settings";
import { FishAudioSettings } from "./FishAudioSettings";

const TTS_PROVIDERS: { id: TtsProvider; label: string; hint: string }[] = [
  { id: "supertonic", label: "Supertonic", hint: "Local, on-device" },
  { id: "fish", label: "Fish Audio", hint: "Cloud, voice cloning" },
];

const SAMPLE_TEXT = "LibreTexts Reader voice test.";
const TEST_PLAYBACK_TIMEOUT_MS = 30_000;

export function SettingsPanel() {
  const hydrated = useSettingsStore((state) => state.hydrated);
  const loading = useSettingsStore((state) => state.loading);
  const error = useSettingsStore((state) => state.error);
  const ttsProvider = useSettingsStore((state) => state.ttsProvider);
  const setTtsProvider = useSettingsStore((state) => state.setTtsProvider);
  const fishVoiceId = useSettingsStore((state) => state.fishVoiceId);
  const supertonicVoiceStyle = useSettingsStore(
    (state) => state.supertonicVoiceStyle,
  );
  const supertonicLanguage = useSettingsStore(
    (state) => state.supertonicLanguage,
  );
  const saveTtsSettings = useSettingsStore((state) => state.saveTtsSettings);

  // Reported by <FishAudioSettings> so the provider picker can warn about a
  // missing key without fetching key status a second time here.
  const [fishKeyPresent, setFishKeyPresent] = useState<boolean | null>(null);

  const [voiceStyle, setVoiceStyle] =
    useState<SupertonicVoiceStyle>(supertonicVoiceStyle);
  const [language, setLanguage] =
    useState<SupertonicLanguage>(supertonicLanguage);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const [testStatus, setTestStatus] = useState<string | null>(null);
  const [testError, setTestError] = useState<string | null>(null);
  const [supertonicModelStatus, setSupertonicModelStatus] =
    useState<SupertonicModelStatus | null>(null);
  const [supertonicModelProgress, setSupertonicModelProgress] =
    useState<SupertonicModelProgress | null>(null);
  const [downloadingSupertonicModel, setDownloadingSupertonicModel] =
    useState(false);
  const [supertonicModelError, setSupertonicModelError] = useState<
    string | null
  >(null);

  useEffect(() => {
    setVoiceStyle(supertonicVoiceStyle);
    setLanguage(supertonicLanguage);
  }, [supertonicLanguage, supertonicVoiceStyle]);

  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }

    let cancelled = false;
    void api
      .getSupertonicModelStatus()
      .then((status) => {
        if (!cancelled) {
          setSupertonicModelStatus(status);
        }
      })
      .catch((error) => {
        if (!cancelled) {
          setSupertonicModelError(
            displayError(error),
          );
        }
      });

    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }

    let cancelled = false;
    let unlisten: (() => void) | null = null;
    void listen<SupertonicModelProgress>(
      "supertonic-model-download-progress",
      (event) => {
        setSupertonicModelProgress(event.payload);
      },
    )
      .then((cleanup) => {
        if (cancelled) {
          cleanup();
        } else {
          unlisten = cleanup;
        }
      })
      .catch((error) => {
        if (!cancelled) {
          setSupertonicModelError(
            displayError(error),
          );
        }
      });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  async function save() {
    setSaving(true);
    setSaved(false);
    setSaveError(null);
    try {
      await persistDraft();
      setSaved(true);
      window.setTimeout(() => setSaved(false), 1600);
    } catch (error) {
      // Surface the failure and never leave the button stuck on "Saving...".
      setSaveError(displayError(error));
    } finally {
      setSaving(false);
    }
  }

  async function persistDraft() {
    await saveTtsSettings({
      supertonicVoiceStyle: voiceStyle,
      supertonicLanguage: language,
    });
  }

  async function testProvider() {
    const providerLabel =
      TTS_PROVIDERS.find((provider) => provider.id === ttsProvider)?.label ??
      ttsProvider;

    setTesting(true);
    setTestStatus(`Loading ${providerLabel}...`);
    setTestError(null);

    try {
      const engine = createSpeechEngine({
        ttsProvider,
        supertonicLanguage: language,
        fishVoiceId,
      });
      await engine.ensureReady(setTestStatus);

      setTestStatus(`Generating ${providerLabel} sample...`);
      const blob = await engine.synthesize({
        text: SAMPLE_TEXT,
        voice: ttsProvider === "supertonic" ? voiceStyle : (fishVoiceId ?? ""),
        speed: 1,
      });

      setTestStatus(`Playing ${providerLabel} sample...`);
      await playBlob(blob);
      setTestStatus(`${providerLabel} test complete.`);
    } catch (error) {
      setTestError(displayError(error));
      setTestStatus(null);
    } finally {
      setTesting(false);
    }
  }

  async function downloadSupertonicModel() {
    setDownloadingSupertonicModel(true);
    setSupertonicModelError(null);
    setSupertonicModelProgress({
      file: "Preparing",
      downloaded: supertonicModelStatus?.downloadedBytes ?? 0,
      total: supertonicModelStatus?.totalBytes ?? 0,
    });

    try {
      const directory = await api.ensureSupertonicModelDownloaded();
      const status = await api.getSupertonicModelStatus();
      setSupertonicModelStatus(status);
      setSupertonicModelProgress({
        file: "Complete",
        downloaded: status.downloadedBytes,
        total: status.totalBytes,
      });
      if (directory) {
        setSupertonicModelStatus({ ...status, directory });
      }
    } catch (error) {
      setSupertonicModelError(displayError(error));
    } finally {
      setDownloadingSupertonicModel(false);
    }
  }

  if (!hydrated && loading) {
    return (
      <p className="inline-flex items-center gap-2 text-sm text-neutral-500 dark:text-neutral-400">
        <Loader2 className="size-4 animate-spin" aria-hidden="true" />
        Loading...
      </p>
    );
  }

  const supertonicDownloaded =
    supertonicModelProgress?.downloaded ??
    supertonicModelStatus?.downloadedBytes ??
    0;
  const supertonicTotal =
    supertonicModelProgress?.total ?? supertonicModelStatus?.totalBytes ?? 0;
  const supertonicProgressPercent =
    supertonicTotal > 0
      ? Math.min(
          100,
          Math.round((supertonicDownloaded / supertonicTotal) * 100),
        )
      : 0;
  const supertonicStatusText = supertonicModelStatus?.downloaded
    ? "Model ready"
    : supertonicModelStatus
      ? "Model not downloaded"
      : "Checking model";

  return (
    <section className="flex flex-col gap-4">
      <div className="rounded-md border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900">
        <h3 className="text-sm font-semibold">Voice engine</h3>
        <div className="mt-3 grid gap-2 sm:grid-cols-2">
          {TTS_PROVIDERS.map((provider) => (
            <button
              aria-pressed={ttsProvider === provider.id}
              className={`rounded-md border px-4 py-3 text-left text-sm transition-colors focus:outline-none focus:ring-2 focus:ring-brand-500 ${
                ttsProvider === provider.id
                  ? "border-brand-500 bg-brand-50 dark:border-brand-500 dark:bg-neutral-800"
                  : "border-neutral-200 hover:bg-stone-100 dark:border-neutral-800 dark:hover:bg-neutral-800"
              }`}
              key={provider.id}
              onClick={() => void setTtsProvider(provider.id)}
              type="button"
            >
              <span className="block font-medium">{provider.label}</span>
              <span className="block text-xs text-neutral-500 dark:text-neutral-400">
                {provider.hint}
              </span>
            </button>
          ))}
        </div>

        {ttsProvider === "fish" && fishKeyPresent === false ? (
          <p className="mt-3 rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-sm text-amber-800 dark:border-amber-950 dark:bg-amber-950/30 dark:text-amber-300">
            Fish Audio is selected but no API key is saved yet. Add one below
            to use it.
          </p>
        ) : null}
      </div>

      <div className="rounded-md border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900">
        <div className="rounded-md border border-neutral-200 bg-stone-50 px-3 py-2 text-sm dark:border-neutral-800 dark:bg-neutral-950">
          <span className="font-medium">Supertonic</span>
          <span className="ml-2 text-neutral-500 dark:text-neutral-400">
            Local multilingual model
          </span>
        </div>

        <div className="mt-5 rounded-md border border-neutral-200 bg-stone-50 p-4 dark:border-neutral-800 dark:bg-neutral-950">
          <div className="grid gap-4 lg:grid-cols-2">
            <label className="flex flex-col gap-2 text-sm font-medium">
              Voice style
              <select
                className="h-10 rounded-md border border-neutral-200 bg-white px-3 text-sm font-normal outline-none focus:border-brand-500 focus:ring-2 focus:ring-brand-500/20 dark:border-neutral-800 dark:bg-neutral-950"
                onChange={(event) =>
                  setVoiceStyle(event.target.value as SupertonicVoiceStyle)
                }
                value={voiceStyle}
              >
                {SUPERTONIC_VOICES.map((voice) => (
                  <option key={voice.id} value={voice.id}>
                    {voice.name}
                  </option>
                ))}
              </select>
            </label>

            <label className="flex flex-col gap-2 text-sm font-medium">
              Language
              <select
                className="h-10 rounded-md border border-neutral-200 bg-white px-3 text-sm font-normal outline-none focus:border-brand-500 focus:ring-2 focus:ring-brand-500/20 dark:border-neutral-800 dark:bg-neutral-950"
                onChange={(event) =>
                  setLanguage(event.target.value as SupertonicLanguage)
                }
                value={language}
              >
                {SUPERTONIC_LANGUAGES.map((option) => (
                  <option key={option.id} value={option.id}>
                    {option.name}
                  </option>
                ))}
              </select>
            </label>
          </div>

          <div className="mt-4 flex flex-wrap items-center justify-between gap-3 border-t border-neutral-200 pt-4 dark:border-neutral-800">
            <div>
              <h3 className="text-sm font-semibold">Supertonic model</h3>
              <p className="mt-1 text-sm text-neutral-500 dark:text-neutral-400">
                {supertonicStatusText}
                {supertonicTotal > 0
                  ? ` - ${formatBytes(supertonicDownloaded)} / ${formatBytes(supertonicTotal)}`
                  : ""}
              </p>
            </div>
            <button
              className="inline-flex h-10 items-center gap-2 rounded-md border border-neutral-200 px-4 text-sm font-medium text-neutral-700 hover:bg-stone-100 focus:outline-none focus:ring-2 focus:ring-brand-500 disabled:cursor-not-allowed disabled:opacity-50 dark:border-neutral-800 dark:text-neutral-200 dark:hover:bg-neutral-800"
              disabled={
                downloadingSupertonicModel ||
                supertonicModelStatus?.downloaded
              }
              onClick={() => void downloadSupertonicModel()}
              type="button"
            >
              {downloadingSupertonicModel ? (
                <Loader2 className="size-4 animate-spin" aria-hidden="true" />
              ) : supertonicModelStatus?.downloaded ? (
                <Check className="size-4" aria-hidden="true" />
              ) : (
                <Download className="size-4" aria-hidden="true" />
              )}
              {supertonicModelStatus?.downloaded
                ? "Downloaded"
                : "Download model"}
            </button>
          </div>

          <div className="mt-3 h-2 overflow-hidden rounded-full bg-neutral-100 dark:bg-neutral-800">
            <div
              className="h-full rounded-full bg-brand-700 transition-[width]"
              style={{ width: `${supertonicProgressPercent}%` }}
            />
          </div>

          {supertonicModelProgress?.file ? (
            <p className="mt-2 text-xs text-neutral-500 dark:text-neutral-400">
              {supertonicModelProgress.file}
            </p>
          ) : null}
        </div>

        <div className="mt-4 flex flex-wrap items-center gap-3">
          <button
            className="inline-flex h-10 items-center gap-2 rounded-md bg-brand-700 px-4 text-sm font-medium text-white hover:bg-brand-500 focus:outline-none focus:ring-2 focus:ring-brand-500 disabled:cursor-not-allowed disabled:opacity-50"
            disabled={saving}
            onClick={() => void save()}
            type="button"
          >
            {saving ? (
              <Loader2 className="size-4 animate-spin" aria-hidden="true" />
            ) : saved ? (
              <Check className="size-4" aria-hidden="true" />
            ) : (
              <Save className="size-4" aria-hidden="true" />
            )}
            {saved ? "Saved" : "Save"}
          </button>
          <button
            className="inline-flex h-10 items-center gap-2 rounded-md border border-neutral-200 px-4 text-sm font-medium text-neutral-700 hover:bg-stone-100 focus:outline-none focus:ring-2 focus:ring-brand-500 disabled:cursor-not-allowed disabled:opacity-50 dark:border-neutral-800 dark:text-neutral-200 dark:hover:bg-neutral-800"
            disabled={testing}
            onClick={() => void testProvider()}
            type="button"
          >
            {testing ? (
              <Loader2 className="size-4 animate-spin" aria-hidden="true" />
            ) : (
              <Play className="size-4" aria-hidden="true" />
            )}
            Test{" "}
            {TTS_PROVIDERS.find((provider) => provider.id === ttsProvider)
              ?.label ?? ttsProvider}{" "}
            voice
          </button>
        </div>
      </div>

      <FishAudioSettings
        onKeyStatusChange={(status: FishKeyStatus) =>
          setFishKeyPresent(status.present)
        }
      />

      {error ? (
        <p className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700 dark:border-red-950 dark:bg-red-950/30 dark:text-red-300">
          {error}
        </p>
      ) : null}

      {saveError ? (
        <p className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700 dark:border-red-950 dark:bg-red-950/30 dark:text-red-300">
          {saveError}
        </p>
      ) : null}

      {testStatus ? (
        <p className="rounded-md border border-neutral-200 bg-white px-3 py-2 text-sm text-neutral-600 dark:border-neutral-800 dark:bg-neutral-900 dark:text-neutral-300">
          {testStatus}
        </p>
      ) : null}

      {testError ? (
        <p className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700 dark:border-red-950 dark:bg-red-950/30 dark:text-red-300">
          {testError}
        </p>
      ) : null}

      {supertonicModelError ? (
        <p className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700 dark:border-red-950 dark:bg-red-950/30 dark:text-red-300">
          {supertonicModelError}
        </p>
      ) : null}

      <p className="text-xs text-neutral-500 dark:text-neutral-400">
        LibreTexts Reader is an independent open-source project. It is not affiliated
        with, endorsed by, or sponsored by LibreTexts or OpenStax.
      </p>
    </section>
  );
}

let testAudio: HTMLAudioElement | null = null;
let testAudioUrl: string | null = null;

function formatBytes(bytes: number) {
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return "0 B";
  }

  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }

  return `${value >= 10 || unitIndex === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[unitIndex]}`;
}

async function playBlob(blob: Blob) {
  if (blob.size === 0) {
    throw new Error("Generated audio was empty.");
  }

  stopTestAudio();
  testAudioUrl = URL.createObjectURL(blob);
  testAudio = new Audio(testAudioUrl);

  try {
    await new Promise<void>((resolve, reject) => {
      const audio = testAudio;
      if (!audio) {
        reject(new Error("Audio playback failed."));
        return;
      }
      const timeoutId = window.setTimeout(() => {
        reject(new Error("Audio playback timed out."));
      }, TEST_PLAYBACK_TIMEOUT_MS);
      const finish = (callback: () => void) => {
        window.clearTimeout(timeoutId);
        callback();
      };
      audio.onended = () => finish(resolve);
      audio.onerror = () =>
        finish(() => reject(new Error("Audio playback failed.")));
      void audio.play().catch((error) => finish(() => reject(error)));
    });
  } finally {
    stopTestAudio();
  }
}

function stopTestAudio() {
  if (testAudio) {
    testAudio.pause();
    testAudio.src = "";
    testAudio = null;
  }
  if (testAudioUrl) {
    URL.revokeObjectURL(testAudioUrl);
    testAudioUrl = null;
  }
}
