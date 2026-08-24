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
import {
  SPEECH_ENGINE_EXPORT_FORMAT,
  SPEECH_ENGINE_LABELS,
  speechAudioToBlob,
} from "../../lib/speech";
import { api, type SupertonicChapterEstimate } from "../../lib/tauri";
import {
  useChapterExportStore,
  type SeedSignal,
} from "../../stores/chapterExport";
import { usePlayerStore } from "../../stores/player";
import { useSettingsStore } from "../../stores/settings";
import { requiresExportConfirmation } from "./exportGate";

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
  const settingsHydrated = useSettingsStore((state) => state.hydrated);
  const settingsFailed = useSettingsStore((state) => state.hydrateFailed);

  const chosenVoiceStyle = useChapterExportStore((state) => state.voiceStyle);
  const chosenLanguage = useChapterExportStore((state) => state.language);
  const chooseVoiceStyle = useChapterExportStore(
    (state) => state.chooseVoiceStyle,
  );
  const chooseLanguage = useChapterExportStore((state) => state.chooseLanguage);
  // Falling back during render, not seeding early: the panel renders in the
  // same commit hydration lands, one commit before the effect below seeds, and
  // a `<select>` handed `null` there would go uncontrolled and blank. This
  // shows the app voice for that commit exactly as the old `useState`
  // initialiser did -- and deliberately does not make `seeded` true, so the
  // estimate still waits for a real seed rather than pricing the chapter for
  // a fallback.
  const voiceStyle: SupertonicVoiceStyle = chosenVoiceStyle ?? defaultVoiceStyle;
  const language: SupertonicLanguage = chosenLanguage ?? defaultLanguage;
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
  // Carries its own estimate rather than reading the shared one. A forced
  // estimate is a property of one request, not of the chapter: writing it to
  // the shared state left the standing "Estimate: N billable characters" line
  // quoting a forced price after the reader cancelled, for a plain Generate
  // that would have been served from cache for nothing.
  const [pendingExport, setPendingExport] = useState<{
    force: boolean;
    estimate: SupertonicChapterEstimate;
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
  const providerLabel = SPEECH_ENGINE_LABELS[ttsProvider];
  // What the reader will actually receive -- see SPEECH_ENGINE_EXPORT_FORMAT.
  const exportFormat = SPEECH_ENGINE_EXPORT_FORMAT[ttsProvider];
  // Fish has no sensible built-in voice (see FishProvider::voice in
  // src-tauri/src/tts/fish/provider.rs), so a request is only sent once one
  // is configured -- matching the backend's own guard rather than racing it.
  const fishVoiceReady = !isFish || !!fishVoiceId;
  // Blocked only while something is genuinely in flight. This deliberately
  // does NOT include `!estimate`: a failed estimate fetch left both buttons
  // dead with no retry path -- the effect below only re-runs on a chapter,
  // voice or provider change -- so the reader had to navigate away and back,
  // and a free Supertonic export was disabled by a price it never needed.
  //
  // Nothing is lost by allowing the click. `requiresExportConfirmation`
  // treats a missing estimate as billed and gates on it (exportGate.ts, and
  // the "gates when there is no estimate at all" case in its tests), and the
  // Fish path in `requestExport` refetches the estimate before deciding
  // anything. The money invariant lives there, in a pure tested function --
  // not in a disabled attribute.
  // `settingsFailed` too: the store is reporting DEFAULT_SETTINGS, so this
  // panel may be showing the Supertonic export UI to a reader who uses Fish,
  // and Generate would write them an MP3 from an engine they never chose.
  // SettingsPanel disables Test for exactly this reason.
  const exportBlocked =
    exporting || checkingExport || estimating || settingsFailed;

  // Stop any in-flight preview playback and release its blob URL when the
  // reader view unmounts, not just when playback ends or a new preview starts.
  useEffect(() => () => stopPreview(), []);

  // Seed the drafts from the app defaults once settings finish loading, then
  // leave them alone: from there they are this export's voice and language,
  // not the app's. The `useState` initialisers above cannot do this on their
  // own -- hooks run before the render gate below, so on a mount that beats
  // `hydrate()` they capture the built-in defaults. Keyed on the hydration
  // transition rather than on the rows, so a later change to those rows
  // cannot pull a pick out from under the reader.
  //
  // Which settings snapshot this render is looking at, against the one the
  // drafts were last seeded from. A boolean `seeded` could not say that: on
  // the retry it was already true, so it gated nothing in the commit where
  // re-seeding was queued -- see the estimate effect below, which priced the
  // chapter for the pre-retry defaults there. This is computed *during*
  // render, so the two differ in that same commit and every effect in it
  // agrees the drafts are stale.
  const seedSignal: SeedSignal = !settingsHydrated
    ? null
    : settingsFailed
      ? "defaults"
      : "stored";
  const seededFrom = useChapterExportStore((state) => state.seededFrom);
  const seeded = seedSignal !== null && seededFrom === seedSignal;
  const seed = useChapterExportStore((state) => state.seed);
  useEffect(() => {
    if (seedSignal === null) {
      return;
    }
    // Never over a pick the reader made themselves -- `seed` leaves a chosen
    // draft alone, one flag per draft. This panel renders during a failed load
    // (with a notice saying the dropdowns are defaults) so they can choose
    // there, and a retry that finally brings the real rows in would otherwise
    // replace that choice with no indication, right before they click
    // Generate. The same flags are what stop a remount from re-seeding over a
    // pick made before a trip out of the Reader.
    seed(seedSignal, useSettingsStore.getState());
    // A retry from Settings changes `seedSignal` from "defaults" to "stored",
    // which is what re-seeds these drafts with the reader's real rows;
    // `settingsHydrated` is already true by then and would never fire this
    // again on its own. The panel still renders on the failure path -- see
    // the notice below -- so this seeds the defaults meanwhile rather than
    // leaving it blank.
  }, [seed, seedSignal]);

  // A provider or section change invalidates any confirmation already on
  // screen -- it named a character count and provider that no longer apply.
  useEffect(() => {
    setPendingExport(null);
  }, [ttsProvider, section?.id]);

  useEffect(() => {
    // Clear FIRST, on every dependency change, before anything async starts.
    // Overwriting the estimate only once the new one resolves left the
    // previous chapter's (or voice's) value readable for the whole round
    // trip, and `requestExport` read it to decide whether to gate -- so
    // moving to a new section and clicking Generate before the estimate
    // landed sent an unconfirmed billed Fish request priced at the previous
    // section's "cached, costs nothing". Nothing may read a stale estimate,
    // so no stale estimate may exist.
    setEstimate(null);

    // `seeded`, not `settingsHydrated`: every effect in one commit sees that
    // commit's closures, so on the render where hydration lands the seeding
    // effect above calls setState but *this* effect still holds the
    // pre-hydration draft. Gating on hydration let it price the chapter for
    // the built-in "M1" once before re-firing with the reader's row -- and on
    // Fish, reach the network to do it. `seeded` compares the snapshot the
    // drafts came from against the one this render sees, so it is false in
    // the commit a change arrives and stays false until the drafts catch up
    // -- on a retry as well as on the first load, where a flag the seeding
    // effect merely set could only cover the first.
    if (!document || !section || !fishVoiceReady || !seeded) {
      setEstimateError(null);
      // Cleared here too: a run cancelled by a dependency change skips its own
      // `finally`, so returning without this leaves `estimating` true for
      // good -- a spinner that never stops and `exportBlocked` stuck on.
      setEstimating(false);
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
    seeded,
    isFish,
    language,
    section,
    ttsProvider,
    voiceStyle,
  ]);

  // `settingsHydrated` gates the whole panel, not just the seeding effect
  // below: without it Preview and Generate are clickable in the window before
  // `hydrate()` resolves and send the built-in defaults rather than the
  // reader's saved rows -- and since this panel deliberately persists
  // nothing, nothing afterwards would reveal the mismatch. SettingsPanel
  // gates on the same thing for the same reason.
  if (!document || !section || !settingsHydrated) {
    return null;
  }
  const activeDocument = document;
  const activeSection = section;

  async function preview() {
    setPreviewing(true);
    setError(null);
    setStatus(null);

    try {
      // Deliberately does NOT persist the draft -- and neither does Generate;
      // nothing in this panel writes the shared Supertonic rows. Preview
      // sends `voiceStyle` below, so writing them bought nothing, and now
      // that `player.ts` keys its engine on those rows it repointed the
      // narration of the book being read in this very view.
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
      force
        ? `Regenerating chapter ${exportFormat}...`
        : `Generating chapter ${exportFormat}...`,
    );

    try {
      // Nothing here writes the shared Supertonic settings rows. The voice
      // and language below are this export's, sent on the request; playback
      // keys its engine on those rows, so persisting them switched the
      // narration of the chapter open in this very view and left Settings
      // showing a voice the reader never picked there. Awaiting that write
      // also let a failed settings save abort a local, network-free export
      // that never needed it. Settings owns the reading voice; this panel
      // picks a voice for one file.
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
        `${result.cached ? `Loaded cached ${exportFormat}` : `Saved ${exportFormat}`}: ${result.outputPath}`,
      );
    } catch (error) {
      setError(displayError(error));
      setStatus(null);
    } finally {
      setExporting(false);
    }
  }

  // The gate: a billed export stops here and waits for an explicit
  // confirmation naming the provider and the character count, instead of
  // calling the export command immediately. The decision itself lives in
  // `requiresExportConfirmation` (./exportGate.ts) so it can be unit-tested
  // exhaustively -- see that file for why each case decides as it does.
  //
  // EVERY Fish request refetches the estimate first, not just the forced
  // ones. The estimate held in state is computed by an effect that knows
  // nothing about `force` and lags a section or voice change by a network
  // round trip; deciding a billed request from it is deciding from a price
  // that may belong to a different request. Refetching here binds the
  // estimate to the exact chapter, voice and force flag being requested, and
  // `requiresExportConfirmation` refuses to proceed on a null one, so the
  // two failure modes (stale value, no value) are both closed.
  async function requestExport(force: boolean) {
    setError(null);

    let relevantEstimate = estimate;
    if (isFish) {
      setCheckingExport(true);
      try {
        relevantEstimate = await api.estimateSupertonicChapter({
          documentId: activeDocument.id,
          sectionId: activeSection.id,
          provider: ttsProvider,
          voiceStyle: fishVoiceId,
          language: null,
          force,
        });
        // Only an unforced estimate describes the chapter's standing price,
        // so only that one belongs in the shared display state.
        if (!force) {
          setEstimate(relevantEstimate);
        }
      } catch (error) {
        setError(displayError(error));
        return;
      } finally {
        setCheckingExport(false);
      }
    }

    if (
      requiresExportConfirmation({
        estimate: relevantEstimate,
        force,
        provider: ttsProvider,
      })
    ) {
      if (!relevantEstimate) {
        // Gating means this request is billed. With no price there is nothing
        // to show and nothing the reader could meaningfully approve, and the
        // gate used to render only when the shared estimate happened to be
        // set -- so this combination left the export neither running nor
        // confirmable.
        setError("Could not price this export. Try again.");
        return;
      }
      setPendingExport({ force, estimate: relevantEstimate });
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
            {providerLabel} chapter {exportFormat}
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

      {settingsFailed ? (
        // `settingsHydrated` is true on the failure path too, so the gate
        // above lets this render with DEFAULT_SETTINGS in the dropdowns. They
        // are editable and visible, unlike the Save in Settings that had to
        // be blocked outright -- but a reader whose saved rows say F3/Korean
        // would otherwise get an M1 English MP3 with nothing saying why.
        <p className="mt-3 rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-xs text-amber-800 dark:border-amber-950 dark:bg-amber-950/30 dark:text-amber-300">
          Your saved settings could not be loaded, so the voice engine, voice
          and language here are the built-in defaults rather than yours --
          this may not even be the engine you use. Exporting is off until they
          load; retry it from Settings.
        </p>
      ) : null}

      {isFish ? null : (
        // These two controls used to write the app's Supertonic rows, which
        // is how picking a voice here also changed what was being read aloud
        // -- mid-chapter, in the same view. They are this export's alone now,
        // and they sit directly above the paragraphs looking exactly like the
        // ones in Settings, so the difference has to be said rather than
        // inferred.
        <p className="mt-3 text-xs text-neutral-500 dark:text-neutral-400">
          These apply to this {exportFormat} only. The voice you are listening in is in
          Settings.
        </p>
      )}

      <div className="mt-4 grid gap-3 md:grid-cols-2 lg:grid-cols-[minmax(10rem,0.25fr)_minmax(12rem,0.32fr)_auto_auto_auto]">
        {isFish ? null : (
          <>
            <label className="flex flex-col gap-2 text-sm font-medium">
              Voice
              <select
                className="h-10 rounded-md border border-neutral-200 bg-white px-3 text-sm font-normal outline-none focus:border-brand-500 focus:ring-2 focus:ring-brand-500/20 dark:border-neutral-800 dark:bg-neutral-950"
                onChange={(event) => {
                  chooseVoiceStyle(event.target.value as SupertonicVoiceStyle);
                }}
                value={voiceStyle}
              >
                {SUPERTONIC_VOICES.map((voice) => (
                  <option key={voice.id} value={voice.id}>
                    {voice.name}
                  </option>
                ))}
              </select>
            </label>

            <div className="flex flex-col gap-2">
              <label className="flex flex-col gap-2 text-sm font-medium">
                Pronunciation language
                <select
                  aria-describedby="supertonic-export-language-help"
                  className="h-10 rounded-md border border-neutral-200 bg-white px-3 text-sm font-normal outline-none focus:border-brand-500 focus:ring-2 focus:ring-brand-500/20 dark:border-neutral-800 dark:bg-neutral-950"
                  onChange={(event) => {
                    chooseLanguage(event.target.value as SupertonicLanguage);
                  }}
                  value={language}
                >
                  {SUPERTONIC_LANGUAGES.map((option) => (
                    <option key={option.id} value={option.id}>
                      {option.name}
                    </option>
                  ))}
                </select>
              </label>
              {/*
                Worth the words here even more than in Settings: an export
                encodes the whole chapter before the reader hears any of it,
                so a language picked on the wrong understanding costs the full
                run. Supertonic does not translate -- the tag only selects the
                letter-to-sound rules for the chapter's own words. Kept
                outside the `label` so the select's accessible name stays the
                label alone.
              */}
              <span
                className="text-xs text-neutral-500 dark:text-neutral-400"
                id="supertonic-export-language-help"
              >
                Pronunciation only — this does not translate the chapter.
                Choose the language the chapter is written in.
              </span>
            </div>
          </>
        )}

        {isFish ? null : (
          <button
            className="inline-flex h-10 items-center justify-center gap-2 self-end rounded-md border border-neutral-200 px-4 text-sm font-medium text-neutral-700 hover:bg-stone-100 focus:outline-none focus:ring-2 focus:ring-brand-500 disabled:cursor-not-allowed disabled:opacity-50 dark:border-neutral-800 dark:text-neutral-200 dark:hover:bg-neutral-800"
            disabled={previewing || exporting || settingsFailed}
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
          disabled={exportBlocked || !fishVoiceReady}
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
          {estimate?.cached
            ? `Save Cached ${exportFormat}`
            : `Generate ${exportFormat}`}
        </button>

        <button
          className="inline-flex h-10 items-center justify-center gap-2 self-end rounded-md border border-neutral-200 px-4 text-sm font-medium text-neutral-700 hover:bg-stone-100 focus:outline-none focus:ring-2 focus:ring-brand-500 disabled:cursor-not-allowed disabled:opacity-50 dark:border-neutral-800 dark:text-neutral-200 dark:hover:bg-neutral-800"
          disabled={exportBlocked || !fishVoiceReady}
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

      {pendingExport ? (
        <div className="mt-4 rounded-md border border-amber-300 bg-amber-50 p-4 text-sm dark:border-amber-900 dark:bg-amber-950/30">
          <p className="font-semibold text-amber-900 dark:text-amber-200">
            Confirm {providerLabel} export
          </p>
          <p className="mt-1 text-amber-800 dark:text-amber-300">
            This will send{" "}
            <strong>
              {pendingExport.estimate.billableCharacters.toLocaleString()}{" "}
              characters
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
