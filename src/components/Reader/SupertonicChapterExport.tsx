import { useEffect, useMemo, useState } from "react";
import { Check, FileAudio, Loader2, Play, RefreshCw } from "lucide-react";
import {
  SUPERTONIC_LANGUAGES,
  SUPERTONIC_VOICES,
  supertonicPreviewText,
  type SupertonicLanguage,
  type SupertonicVoiceStyle,
} from "../../lib/supertonic";
import { displayError } from "../../lib/errors";
import { speechAudioToBlob } from "../../lib/speech";
import { api, type SupertonicChapterEstimate } from "../../lib/tauri";
import { usePlayerStore } from "../../stores/player";
import { useSettingsStore } from "../../stores/settings";

export function SupertonicChapterExport() {
  const document = usePlayerStore((state) => state.document);
  const sections = usePlayerStore((state) => state.sections);
  const currentSectionIndex = usePlayerStore(
    (state) => state.currentSectionIndex,
  );
  const paragraphs = usePlayerStore((state) => state.paragraphs);
  const defaultVoiceStyle = useSettingsStore(
    (state) => state.supertonicVoiceStyle,
  );
  const defaultLanguage = useSettingsStore((state) => state.supertonicLanguage);
  const saveTtsSettings = useSettingsStore((state) => state.saveTtsSettings);

  const [voiceStyle, setVoiceStyle] =
    useState<SupertonicVoiceStyle>(defaultVoiceStyle);
  const [language, setLanguage] = useState<SupertonicLanguage>(defaultLanguage);
  const [estimate, setEstimate] = useState<SupertonicChapterEstimate | null>(
    null,
  );
  const [estimating, setEstimating] = useState(false);
  const [estimateError, setEstimateError] = useState<string | null>(null);
  const [previewing, setPreviewing] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const section = sections[currentSectionIndex] ?? null;
  const sampleText = useMemo(
    () =>
      paragraphs.find((paragraph) => paragraph.text.trim().length > 0)?.text ??
      "",
    [paragraphs],
  );

  // Stop any in-flight preview playback and release its blob URL when the
  // reader view unmounts, not just when playback ends or a new preview starts.
  useEffect(() => () => stopPreview(), []);

  useEffect(() => {
    setVoiceStyle(defaultVoiceStyle);
  }, [defaultVoiceStyle]);

  useEffect(() => {
    setLanguage(defaultLanguage);
  }, [defaultLanguage]);

  useEffect(() => {
    if (!document || !section) {
      setEstimate(null);
      return;
    }

    let cancelled = false;
    setEstimating(true);
    setEstimateError(null);
    void api
      .estimateSupertonicChapter({
        documentId: document.id,
        sectionId: section.id,
        provider: "supertonic",
        voiceStyle,
        language,
      })
      .then((result) => {
        if (!cancelled) {
          setEstimate(result);
        }
      })
      .catch((error) => {
        if (!cancelled) {
          setEstimate(null);
          setEstimateError(displayError(error));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setEstimating(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [document, language, section, voiceStyle]);

  if (!document || !section) {
    return null;
  }
  const activeDocument = document;
  const activeSection = section;

  async function persistSupertonicDefaults() {
    await saveTtsSettings({
      supertonicVoiceStyle: voiceStyle,
      supertonicLanguage: language,
    });
  }

  async function preview() {
    setPreviewing(true);
    setError(null);
    setStatus(null);

    try {
      await persistSupertonicDefaults();
      const speech = await api.previewSupertonicTts({
        // Sent as display text: preview_supertonic_tts normalizes it with the
        // same code the chapter export uses, so preview and export agree.
        text: supertonicPreviewText(activeSection.title, sampleText),
        voiceStyle,
        language,
      });
      await playBlob(speechAudioToBlob(speech));
    } catch (error) {
      setError(displayError(error));
    } finally {
      setPreviewing(false);
    }
  }

  async function exportChapter(force = false) {
    setExporting(true);
    setError(null);
    setStatus(
      force ? "Regenerating chapter MP3..." : "Generating chapter MP3...",
    );

    try {
      await persistSupertonicDefaults();
      const result = await api.exportSupertonicChapterMp3({
        documentId: activeDocument.id,
        sectionId: activeSection.id,
        provider: "supertonic",
        voiceStyle,
        language,
        force,
      });
      setEstimate(result.estimate);
      setStatus(
        `${result.cached ? "Loaded cached MP3" : "Saved MP3"}: ${result.outputPath}`,
      );
    } catch (error) {
      setError(displayError(error));
      setStatus(null);
    } finally {
      setExporting(false);
    }
  }

  return (
    <section className="rounded-md border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <h2 className="inline-flex items-center gap-2 text-base font-semibold">
            <FileAudio className="size-4 text-brand-700" aria-hidden="true" />
            Supertonic chapter MP3
          </h2>
          <p className="mt-1 text-sm text-neutral-500 dark:text-neutral-400">
            {section.title}
          </p>
        </div>
        <div className="text-right text-sm">
          {estimating ? (
            <span className="inline-flex items-center gap-2 text-neutral-500 dark:text-neutral-400">
              <Loader2 className="size-4 animate-spin" aria-hidden="true" />
              Estimating
            </span>
          ) : estimate ? (
            <>
              <p className="font-semibold">
                {formatDuration(estimate.estimatedSeconds)}
              </p>
              <p className="text-neutral-500 dark:text-neutral-400">
                {estimate.wordCount.toLocaleString()} words ·{" "}
                {estimate.chunkCount.toLocaleString()}{" "}
                {estimate.chunkCount === 1 ? "chunk" : "chunks"}
              </p>
            </>
          ) : null}
        </div>
      </div>

      <div className="mt-4 grid gap-3 md:grid-cols-2 lg:grid-cols-[minmax(10rem,0.25fr)_minmax(12rem,0.32fr)_auto_auto_auto]">
        <label className="flex flex-col gap-2 text-sm font-medium">
          Voice
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

        <button
          className="inline-flex h-10 items-center justify-center gap-2 self-end rounded-md border border-neutral-200 px-4 text-sm font-medium text-neutral-700 hover:bg-stone-100 focus:outline-none focus:ring-2 focus:ring-brand-500 disabled:cursor-not-allowed disabled:opacity-50 dark:border-neutral-800 dark:text-neutral-200 dark:hover:bg-neutral-800"
          disabled={previewing || exporting}
          onClick={() => void preview()}
          type="button"
        >
          {previewing ? (
            <Loader2 className="size-4 animate-spin" aria-hidden="true" />
          ) : (
            <Play className="size-4" aria-hidden="true" />
          )}
          Preview
        </button>

        <button
          className="inline-flex h-10 items-center justify-center gap-2 self-end rounded-md bg-brand-700 px-4 text-sm font-medium text-white hover:bg-brand-500 focus:outline-none focus:ring-2 focus:ring-brand-500 disabled:cursor-not-allowed disabled:opacity-50"
          disabled={exporting}
          onClick={() => void exportChapter(false)}
          type="button"
        >
          {exporting ? (
            <Loader2 className="size-4 animate-spin" aria-hidden="true" />
          ) : estimate?.cached ? (
            <Check className="size-4" aria-hidden="true" />
          ) : (
            <FileAudio className="size-4" aria-hidden="true" />
          )}
          {estimate?.cached ? "Save Cached MP3" : "Generate MP3"}
        </button>

        <button
          className="inline-flex h-10 items-center justify-center gap-2 self-end rounded-md border border-neutral-200 px-4 text-sm font-medium text-neutral-700 hover:bg-stone-100 focus:outline-none focus:ring-2 focus:ring-brand-500 disabled:cursor-not-allowed disabled:opacity-50 dark:border-neutral-800 dark:text-neutral-200 dark:hover:bg-neutral-800"
          disabled={exporting}
          onClick={() => void exportChapter(true)}
          type="button"
        >
          <RefreshCw className="size-4" aria-hidden="true" />
          Regenerate
        </button>
      </div>

      {estimate ? (
        <p className="mt-4 break-words text-xs text-neutral-500 dark:text-neutral-400">
          Estimate uses {estimate.chunkCount} local generation{" "}
          {estimate.chunkCount === 1 ? "chunk" : "chunks"} at approximately{" "}
          {formatDuration(estimate.estimatedSeconds)}. Output path:{" "}
          {estimate.outputPath}
        </p>
      ) : null}

      {estimateError ? (
        <p className="mt-4 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700 dark:border-red-950 dark:bg-red-950/30 dark:text-red-300">
          {estimateError}
        </p>
      ) : null}

      {status ? (
        <p className="mt-4 break-words rounded-md border border-neutral-200 bg-stone-50 px-3 py-2 text-sm text-neutral-600 dark:border-neutral-800 dark:bg-neutral-950 dark:text-neutral-300">
          {status}
        </p>
      ) : null}

      {error ? (
        <p className="mt-4 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700 dark:border-red-950 dark:bg-red-950/30 dark:text-red-300">
          {error}
        </p>
      ) : null}
    </section>
  );
}

let previewAudio: HTMLAudioElement | null = null;
let previewUrl: string | null = null;

async function playBlob(blob: Blob) {
  if (blob.size === 0) {
    throw new Error("Generated audio was empty.");
  }

  stopPreview();
  previewUrl = URL.createObjectURL(blob);
  previewAudio = new Audio(previewUrl);

  try {
    await new Promise<void>((resolve, reject) => {
      const audio = previewAudio;
      if (!audio) {
        reject(new Error("Audio playback failed."));
        return;
      }
      audio.onended = () => resolve();
      audio.onerror = () => reject(new Error("Audio playback failed."));
      void audio.play().catch(reject);
    });
  } finally {
    stopPreview();
  }
}

function stopPreview() {
  if (previewAudio) {
    previewAudio.pause();
    previewAudio.src = "";
    previewAudio = null;
  }
  if (previewUrl) {
    URL.revokeObjectURL(previewUrl);
    previewUrl = null;
  }
}

function formatDuration(seconds: number) {
  if (!Number.isFinite(seconds) || seconds <= 0) {
    return "0 min";
  }
  const minutes = Math.max(1, Math.round(seconds / 60));
  return `${minutes} min`;
}
