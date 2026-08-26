import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { listen } from "@tauri-apps/api/event";
import { Download, Loader2 } from "lucide-react";
import { api } from "../../lib/tauri";
import { displayError } from "../../lib/errors";
import {
  SUPERTONIC_LANGUAGES,
  type SupertonicLanguage,
} from "../../lib/supertonic";
import { usePlayerStore } from "../../stores/player";
import { useSettingsStore } from "../../stores/settings";
import { useTranslationStore } from "../../stores/translation";
import type * as Domain from "../../types/domain";

const SUPERTONIC_LANGUAGE_IDS = new Set(
  SUPERTONIC_LANGUAGES.map((language) => language.id),
);

interface PendingDownload {
  language: SupertonicLanguage;
  status: Domain.TranslationModelStatus;
}

export function ListenInControl() {
  const openDocument = usePlayerStore((state) => state.document);
  const isPlaying = usePlayerStore((state) => state.isPlaying);
  const isBuffering = usePlayerStore((state) => state.isBuffering);
  const play = usePlayerStore((state) => state.play);
  const pause = usePlayerStore((state) => state.pause);
  const targetLanguage = useSettingsStore(
    (state) => state.translationTargetLang,
  );
  const saveTtsSettings = useSettingsStore(
    (state) => state.saveTtsSettings,
  );
  const settingsReady = useSettingsStore(
    (state) => state.hydrated && !state.hydrateFailed,
  );
  const translationRunning = useTranslationStore(
    (state) => state.sectionState.status === "running",
  );
  const sourceLanguage = openDocument?.sourceLanguage ?? "en";
  const [targets, setTargets] = useState<SupertonicLanguage[]>([]);
  const [checking, setChecking] = useState(false);
  const [activeModelStatus, setActiveModelStatus] =
    useState<Domain.TranslationModelStatus | null>(null);
  const [pendingDownload, setPendingDownload] =
    useState<PendingDownload | null>(null);
  const [downloading, setDownloading] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [progress, setProgress] =
    useState<Domain.TranslationModelDownloadProgress | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void api
      .listTranslationTargets(sourceLanguage)
      .then((available) => {
        if (cancelled) {
          return;
        }
        setTargets(
          available.filter(
            (language): language is SupertonicLanguage =>
              language !== "na" &&
              SUPERTONIC_LANGUAGE_IDS.has(language as SupertonicLanguage),
          ),
        );
      })
      .catch((reason) => {
        if (!cancelled) {
          setError(displayError(reason));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [sourceLanguage]);

  useEffect(() => {
    if (!targetLanguage || targetLanguage === sourceLanguage) {
      setActiveModelStatus(null);
      return;
    }
    let cancelled = false;
    void api
      .getTranslationModelStatus(sourceLanguage, targetLanguage)
      .then((status) => {
        if (!cancelled) {
          setActiveModelStatus(status);
        }
      })
      .catch((reason) => {
        if (!cancelled) {
          setError(displayError(reason));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [sourceLanguage, targetLanguage]);

  useEffect(() => {
    if (!pendingDownload) {
      return;
    }

    let disposed = false;
    let unlisten: (() => void) | null = null;
    const target = pendingDownload.language;
    void listen<Domain.TranslationModelDownloadProgress>(
      "translation-model-download-progress",
      (event) => {
        if (
          event.payload.sourceLang === sourceLanguage &&
          event.payload.targetLang === target
        ) {
          setProgress(event.payload);
        }
      },
    )
      .then((cleanup) => {
        if (disposed) {
          cleanup();
        } else {
          unlisten = cleanup;
        }
      })
      .catch((reason) => {
        if (!disposed) {
          setError(displayError(reason));
        }
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [pendingDownload, sourceLanguage]);

  async function commitLanguage(
    language: SupertonicLanguage | null,
    playWhenReady: boolean,
  ) {
    const resumePlayback = isPlaying;
    if (resumePlayback) {
      pause();
    }
    await saveTtsSettings({
      translationTargetLang: language,
      ...(language ? { supertonicLanguage: language } : {}),
    });
    resetChapterTranslation();
    if (playWhenReady || resumePlayback) {
      await play();
    }
  }

  async function requestLanguage(value: string) {
    setError(null);
    setProgress(null);
    setPendingDownload(null);
    if (value === "original") {
      if (targetLanguage !== null) {
        try {
          await commitLanguage(null, false);
        } catch (reason) {
          setError(displayError(reason));
        }
      }
      return;
    }

    const language = value as SupertonicLanguage;
    if (language === targetLanguage) {
      return;
    }
    setChecking(true);
    try {
      const status = await api.getTranslationModelStatus(
        sourceLanguage,
        language,
      );
      if (status.downloaded) {
        setActiveModelStatus(status);
        await commitLanguage(language, false);
      } else {
        setPendingDownload({ language, status });
      }
    } catch (reason) {
      setError(displayError(reason));
    } finally {
      setChecking(false);
    }
  }

  async function downloadAndPlay() {
    if (!pendingDownload) {
      return;
    }
    const target = pendingDownload.language;
    setDownloading(true);
    setCancelling(false);
    setError(null);
    setProgress({
      sourceLang: sourceLanguage,
      targetLang: target,
      pair: `${sourceLanguage}-${target}`,
      file: "Preparing",
      downloaded: pendingDownload.status.downloadedBytes,
      total: pendingDownload.status.totalBytes,
    });
    try {
      await api.ensureTranslationModelsDownloaded(sourceLanguage, target);
      setActiveModelStatus({
        ...pendingDownload.status,
        downloaded: true,
        downloadedBytes: pendingDownload.status.totalBytes,
      });
      await commitLanguage(target, true);
      setPendingDownload(null);
      setProgress(null);
    } catch (reason) {
      const message = displayError(reason);
      if (/cancel/i.test(message)) {
        setPendingDownload(null);
        setProgress(null);
      } else {
        setError(message);
      }
    } finally {
      setDownloading(false);
      setCancelling(false);
    }
  }

  function cancelDownload() {
    setCancelling(true);
    void api.cancelTranslationModelDownload().catch((reason) => {
      setCancelling(false);
      setError(displayError(reason));
    });
  }

  const total = progress?.total ?? pendingDownload?.status.totalBytes ?? 0;
  const downloaded = progress?.downloaded ?? 0;
  const percent =
    total > 0 ? Math.min(100, Math.round((downloaded / total) * 100)) : 0;

  return (
    <div className="flex min-w-48 flex-col gap-1">
      <label className="flex items-center gap-2 text-sm font-medium text-neutral-700 dark:text-neutral-200">
        Listen in
        <select
          aria-label="Listen in"
          className="h-10 min-w-36 rounded-md border border-neutral-200 bg-white px-3 text-sm font-normal outline-none focus:border-brand-500 focus:ring-2 focus:ring-brand-500/20 disabled:cursor-not-allowed disabled:opacity-60 dark:border-neutral-800 dark:bg-neutral-900"
          disabled={
            !settingsReady ||
            checking ||
            downloading ||
            isBuffering ||
            translationRunning
          }
          onChange={(event) => void requestLanguage(event.target.value)}
          value={targetLanguage ?? "original"}
        >
          <option value="original">{languageName(sourceLanguage)}</option>
          {targets.map((language) => (
            <option key={language} value={language}>
              {languageName(language)}
            </option>
          ))}
        </select>
        {checking ? (
          <Loader2
            aria-label="Checking language download"
            className="size-4 animate-spin text-neutral-500"
          />
        ) : null}
      </label>
      <span className="text-xs text-neutral-500 dark:text-neutral-400">
        {targetLanguage ? (
          <>
            {languageName(targetLanguage)} narration ·{" "}
            {activeModelStatus?.downloaded ? (
              "Ready offline"
            ) : activeModelStatus ? (
              <button
                className="font-medium text-brand-700 hover:text-brand-500 focus:outline-none focus:ring-2 focus:ring-brand-500 dark:text-brand-400"
                onClick={() =>
                  setPendingDownload({
                    language: targetLanguage,
                    status: activeModelStatus,
                  })
                }
                type="button"
              >
                Download · {formatModelSize(activeModelStatus.totalBytes)}
              </button>
            ) : (
              "Checking download"
            )}
          </>
        ) : (
          "Original narration"
        )}
      </span>
      {error && !pendingDownload ? (
        <span className="text-xs text-red-700 dark:text-red-300">{error}</span>
      ) : null}

      {pendingDownload
        ? createPortal(
            <div className="fixed inset-0 z-50 grid place-items-center bg-black/60 p-4">
              <div
                aria-label={`Download ${languageName(pendingDownload.language)} narration`}
                aria-modal="true"
                className="w-full max-w-md rounded-lg border border-neutral-200 bg-white p-5 text-neutral-950 shadow-xl dark:border-neutral-700 dark:bg-neutral-900 dark:text-neutral-100"
                role="dialog"
              >
                <div className="flex items-start gap-3">
                  <div className="grid size-9 shrink-0 place-items-center rounded-full bg-brand-100 text-brand-700 dark:bg-brand-950 dark:text-brand-400">
                    <Download className="size-4" aria-hidden="true" />
                  </div>
                  <div>
                    <h2 className="text-base font-semibold">
                      {downloading
                        ? `Downloading ${languageName(pendingDownload.language)} narration`
                        : `Download ${languageName(pendingDownload.language)} narration?`}
                    </h2>
                    <p className="mt-1 text-sm text-neutral-600 dark:text-neutral-300">
                      {formatModelSize(pendingDownload.status.totalBytes)} ·
                      stored locally · works offline
                    </p>
                  </div>
                </div>

                {downloading ? (
                  <div className="mt-5">
                    <div className="flex items-center justify-between gap-3 text-sm">
                      <span>
                        {formatModelSize(downloaded)} of {formatModelSize(total)}
                      </span>
                      <span className="font-semibold tabular-nums">{percent}%</span>
                    </div>
                    <div
                      aria-label="Narration model download"
                      aria-valuemax={100}
                      aria-valuemin={0}
                      aria-valuenow={percent}
                      className="mt-2 h-2 overflow-hidden rounded-full bg-neutral-200 dark:bg-neutral-700"
                      role="progressbar"
                    >
                      <div
                        className="h-full rounded-full bg-brand-700 transition-[width] dark:bg-brand-400"
                        style={{ width: `${percent}%` }}
                      />
                    </div>
                    <button
                      className="mt-4 h-10 rounded-md border border-neutral-300 px-4 text-sm font-medium hover:bg-neutral-100 focus:outline-none focus:ring-2 focus:ring-brand-500 disabled:cursor-wait disabled:opacity-60 dark:border-neutral-700 dark:hover:bg-neutral-800"
                      disabled={cancelling}
                      onClick={cancelDownload}
                      type="button"
                    >
                      {cancelling ? "Cancelling…" : "Cancel download"}
                    </button>
                  </div>
                ) : (
                  <>
                    <p className="mt-4 text-sm text-neutral-600 dark:text-neutral-300">
                      The chapter will be translated for narration, then played
                      in {languageName(pendingDownload.language)}. The book stays
                      in {languageName(sourceLanguage)} on screen.
                    </p>
                    <div className="mt-5 flex justify-end gap-2">
                      <button
                        className="h-10 rounded-md border border-neutral-300 px-4 text-sm font-medium hover:bg-neutral-100 focus:outline-none focus:ring-2 focus:ring-brand-500 dark:border-neutral-700 dark:hover:bg-neutral-800"
                        onClick={() => setPendingDownload(null)}
                        type="button"
                      >
                        Cancel
                      </button>
                      <button
                        className="h-10 rounded-md bg-brand-700 px-4 text-sm font-medium text-white hover:bg-brand-500 focus:outline-none focus:ring-2 focus:ring-brand-500"
                        onClick={() => void downloadAndPlay()}
                        type="button"
                      >
                        Download &amp; play
                      </button>
                    </div>
                  </>
                )}

                {error ? (
                  <p className="mt-3 text-sm text-red-700 dark:text-red-300">
                    {error}
                  </p>
                ) : null}
              </div>
            </div>,
            window.document.body,
          )
        : null}
    </div>
  );
}

function resetChapterTranslation() {
  useTranslationStore.setState({
    sectionState: {
      status: "idle",
      done: 0,
      total: 0,
      fallbackCount: 0,
      sentenceCount: 0,
      error: null,
    },
  });
}

function languageName(language: string) {
  return (
    SUPERTONIC_LANGUAGES.find((candidate) => candidate.id === language)?.name ??
    language.toUpperCase()
  );
}

function formatModelSize(bytes: number) {
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return "0 MB";
  }
  return `${Math.round(bytes / 1_000_000)} MB`;
}
