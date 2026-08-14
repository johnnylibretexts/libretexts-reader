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

const PROVIDER_LABEL: Record<string, string> = {
  supertonic: "Supertonic",
  fish: "Fish Audio",
};

export function SupertonicChapterExport() {
  const document = usePlayerStore((state) => state.document);
  const sections = usePlayerStore((state) => state.sections);
  const currentSectionIndex = usePlayerStore(
    (state) => state.currentSectionIndex,
  );
  const paragraphs = usePlayerStore((state) => state.paragraphs);
  const ttsProvider = useSettingsStore((state) => state.ttsProvider);
  const fishVoiceId = useSettingsStore((state) => state.fishVoiceId);
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
  // Set instead of exporting directly whenever the estimate says this export
  // is billed. Cleared on confirm or cancel; never auto-dismissed, so a
  // slow credit-balance fetch cannot let the export through before the
  // reader has seen it.
  const [pendingExport, setPendingExport] = useState<{
    force: boolean;
  } | null>(null);
  const [checkingExport, setCheckingExport] = useState(false);
  // The live balance, fetched fresh (over the network) each time the gate
  // opens -- never the value get_fish_key_status returns, which is
  // deliberately stale/network-free so Settings can render on mount without
  // waiting on Fish. See getFishCredit / get_fish_credit.
  const [fishCredit, setFishCredit] = useState<number | null>(null);
  const [fishCreditLoading, setFishCreditLoading] = useState(false);
  const [fishCreditError, setFishCreditError] = useState<string | null>(null);

  const section = sections[currentSectionIndex] ?? null;
  const sampleText = useMemo(
    () =>
      paragraphs.find((paragraph) => paragraph.text.trim().length > 0)?.text ??
      "",
    [paragraphs],
  );
  const isFish = ttsProvider === "fish";
  const providerLabel = PROVIDER_LABEL[ttsProvider] ?? ttsProvider;
  // Fish has no sensible built-in voice (see FishProvider::voice in
  // src-tauri/src/tts/fish/provider.rs), so a request is only sent once one
  // is configured -- matching the backend's own guard rather than racing it.
  const fishVoiceReady = !isFish || !!fishVoiceId;

  // Stop any in-flight preview playback and release its blob URL when the
  // reader view unmounts, not just when playback ends or a new preview starts.
  useEffect(() => () => stopPreview(), []);

  useEffect(() => {
    setVoiceStyle(defaultVoiceStyle);
  }, [defaultVoiceStyle]);

  useEffect(() => {
    setLanguage(defaultLanguage);
  }, [defaultLanguage]);

  // A provider or section change invalidates any confirmation already on
  // screen -- it named a character count and provider that no longer apply.
  useEffect(() => {
    setPendingExport(null);
  }, [ttsProvider, section?.id]);

  useEffect(() => {
    if (!document || !section || !fishVoiceReady) {
      setEstimate(null);
      setEstimateError(null);
      return;
    }

    let cancelled = false;
    setEstimating(true);
    setEstimateError(null);
    void api
      .estimateSupertonicChapter({
        documentId: document.id,
        sectionId: section.id,
        provider: ttsProvider,
        voiceStyle: isFish ? fishVoiceId : voiceStyle,
        language: isFish ? null : language,
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
  }, [
    document,
    fishVoiceId,
    fishVoiceReady,
    isFish,
    language,
    section,
    ttsProvider,
    voiceStyle,
  ]);

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

  async function runExport(force: boolean) {
    setExporting(true);
    setError(null);
    setStatus(
      force ? "Regenerating chapter MP3..." : "Generating chapter MP3...",
    );

    try {
      if (!isFish) {
        await persistSupertonicDefaults();
      }
      const result = await api.exportSupertonicChapterMp3({
        documentId: activeDocument.id,
        sectionId: activeSection.id,
        provider: ttsProvider,
        voiceStyle: isFish ? fishVoiceId : voiceStyle,
        language: isFish ? null : language,
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

  // The gate: a billed export (`billableCharacters > 0`) stops here and
  // waits for an explicit confirmation naming the provider and the
  // character count, instead of calling the export command immediately.
  //
  // A forced export (Regenerate) re-synthesises even an already-cached
  // chapter -- a real, billed Fish request -- so the stale `estimate` state
  // (computed with `force: false` by the effect above) cannot be trusted to
  // decide this: for an already-exported Fish chapter it reads
  // `billableCharacters: 0` because the chapter is cached, which would skip
  // the gate for a request that is very much not free. Recompute the
  // estimate with the real `force` flag first, and gate on that. See
  // `billable_characters` in src-tauri/src/commands/chapter_tts.rs, which
  // takes the same `force` flag for the same reason.
  async function requestExport(force: boolean) {
    setError(null);

    let relevantEstimate = estimate;
    if (force && isFish) {
      setCheckingExport(true);
      try {
        relevantEstimate = await api.estimateSupertonicChapter({
          documentId: activeDocument.id,
          sectionId: activeSection.id,
          provider: ttsProvider,
          voiceStyle: fishVoiceId,
          language: null,
          force: true,
        });
        setEstimate(relevantEstimate);
      } catch (error) {
        setError(displayError(error));
        return;
      } finally {
        setCheckingExport(false);
      }
    }

    if ((relevantEstimate?.billableCharacters ?? 0) > 0) {
      setPendingExport({ force });
      void refreshFishCredit();
      return;
    }
    void runExport(force);
  }

  async function refreshFishCredit() {
    setFishCreditLoading(true);
    setFishCreditError(null);
    setFishCredit(null);
    try {
      // Unlike getFishKeyStatus, this DOES call the network -- Fish's own
      // wallet endpoint, via get_fish_credit -- because the gate needs the
      // live balance, not the value cached from the last key validation. A
      // failed fetch must not block an export the reader wants to make, so
      // this only records an error for display: the gate still shows the
      // character count and Confirm still works with no balance shown.
      const credit = await api.getFishCredit();
      setFishCredit(credit);
    } catch (error) {
      setFishCreditError(displayError(error));
    } finally {
      setFishCreditLoading(false);
    }
  }

  function confirmExport() {
    if (!pendingExport) {
      return;
    }
    const { force } = pendingExport;
    setPendingExport(null);
    void runExport(force);
  }

  return (
    <section className="rounded-md border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <h2 className="inline-flex items-center gap-2 text-base font-semibold">
            <FileAudio className="size-4 text-brand-700" aria-hidden="true" />
            {providerLabel} chapter MP3
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

      {isFish ? (
        <p className="mt-3 text-xs text-neutral-500 dark:text-neutral-400">
          {fishVoiceId
            ? `Voice: ${fishVoiceId}. Preview is not available for Fish Audio; export bills your account for uncached chapters.`
            : "No Fish Audio voice is configured. Choose one in Settings before exporting."}
        </p>
      ) : null}

      <div className="mt-4 grid gap-3 md:grid-cols-2 lg:grid-cols-[minmax(10rem,0.25fr)_minmax(12rem,0.32fr)_auto_auto_auto]">
        {isFish ? null : (
          <>
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
          </>
        )}

        {isFish ? null : (
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
        )}

        <button
          className="inline-flex h-10 items-center justify-center gap-2 self-end rounded-md bg-brand-700 px-4 text-sm font-medium text-white hover:bg-brand-500 focus:outline-none focus:ring-2 focus:ring-brand-500 disabled:cursor-not-allowed disabled:opacity-50"
          disabled={exporting || checkingExport || !fishVoiceReady}
          onClick={() => void requestExport(false)}
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
          disabled={exporting || checkingExport || !fishVoiceReady}
          onClick={() => void requestExport(true)}
          type="button"
        >
          {checkingExport ? (
            <Loader2 className="size-4 animate-spin" aria-hidden="true" />
          ) : (
            <RefreshCw className="size-4" aria-hidden="true" />
          )}
          Regenerate
        </button>
      </div>

      {estimate ? (
        <p className="mt-4 break-words text-xs text-neutral-500 dark:text-neutral-400">
          {isFish
            ? `Estimate: ${estimate.billableCharacters.toLocaleString()} billable characters at approximately ${formatDuration(estimate.estimatedSeconds)}.`
            : `Estimate uses ${estimate.chunkCount} local generation ${estimate.chunkCount === 1 ? "chunk" : "chunks"} at approximately ${formatDuration(estimate.estimatedSeconds)}.`}{" "}
          Output path: {estimate.outputPath}
        </p>
      ) : null}

      {pendingExport && estimate ? (
        <div className="mt-4 rounded-md border border-amber-300 bg-amber-50 p-4 text-sm dark:border-amber-900 dark:bg-amber-950/30">
          <p className="font-semibold text-amber-900 dark:text-amber-200">
            Confirm {providerLabel} export
          </p>
          <p className="mt-1 text-amber-800 dark:text-amber-300">
            This will send{" "}
            <strong>
              {estimate.billableCharacters.toLocaleString()} characters
            </strong>{" "}
            to {providerLabel} and bill your account. This request is not
            served from the cache, so it is not free.
          </p>
          <p className="mt-1 text-amber-800 dark:text-amber-300">
            {fishCreditLoading
              ? "Checking Fish Audio credit balance..."
              : fishCreditError
                ? // A failed balance check never blocks the export -- the
                  // character count above is still shown and Confirm still
                  // works.
                  `Could not check credit balance: ${fishCreditError}`
                : fishCredit != null
                  ? `Current Fish Audio credit balance: ${fishCredit.toLocaleString()}`
                  : "Fish Audio credit balance is unavailable."}
          </p>
          <div className="mt-3 flex flex-wrap gap-2">
            <button
              className="inline-flex h-9 items-center gap-2 rounded-md bg-amber-700 px-4 text-sm font-medium text-white hover:bg-amber-600 focus:outline-none focus:ring-2 focus:ring-amber-500"
              onClick={confirmExport}
              type="button"
            >
              Confirm and export with {providerLabel}
            </button>
            <button
              className="inline-flex h-9 items-center gap-2 rounded-md border border-amber-300 px-4 text-sm font-medium text-amber-900 hover:bg-amber-100 focus:outline-none focus:ring-2 focus:ring-amber-500 dark:border-amber-800 dark:text-amber-200 dark:hover:bg-amber-950/60"
              onClick={() => setPendingExport(null)}
              type="button"
            >
              Cancel
            </button>
          </div>
        </div>
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
