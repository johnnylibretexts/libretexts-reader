import { useEffect, useRef, useState } from "react";
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

const TTS_PROVIDERS: {
  id: TtsProvider;
  label: string;
  hint: string;
  /**
   * True when a synthesis through this provider costs the user money. The
   * "Test voice" button below is one click for a provider that bills nothing
   * and asks first for one that does -- the chapter export panel gates the
   * same way, and the two surfaces sending money over the network must not
   * disagree about whether that needs consent.
   */
  bills: boolean;
}[] = [
  {
    id: "supertonic",
    label: "Supertonic",
    hint: "Local, on-device",
    bills: false,
  },
  { id: "fish", label: "Fish Audio", hint: "Cloud, voice cloning", bills: true },
];

const SAMPLE_TEXT = "LibreTexts Reader voice test.";
const TEST_PLAYBACK_TIMEOUT_MS = 30_000;

export function SettingsPanel() {
  const hydrated = useSettingsStore((state) => state.hydrated);
  const error = useSettingsStore((state) => state.error);
  // The values below are built-in defaults, not the reader's -- see
  // `hydrateFailed`. Saving them would write over rows this screen never
  // showed, so Save refuses while it is set.
  const hydrateFailed = useSettingsStore((state) => state.hydrateFailed);
  const hydrate = useSettingsStore((state) => state.hydrate);
  const [retryingHydrate, setRetryingHydrate] = useState(false);
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

  const activeProvider =
    TTS_PROVIDERS.find((provider) => provider.id === ttsProvider) ??
    TTS_PROVIDERS[0];
  const providerLabel = activeProvider.label;

  // Reported by <FishAudioSettings> so the provider picker can warn about a
  // missing key without fetching key status a second time here.
  const [fishKeyPresent, setFishKeyPresent] = useState<boolean | null>(null);

  const [voiceStyle, setVoiceStyle] =
    useState<SupertonicVoiceStyle>(supertonicVoiceStyle);
  const [language, setLanguage] =
    useState<SupertonicLanguage>(supertonicLanguage);
  // One flag per draft. These stay editable while a failed load has Save
  // disabled, so a reader can line up their pick while waiting -- and the
  // retry that finally succeeds must not replace it at the very moment Save
  // becomes usable. Same guard, same reasoning, as SupertonicChapterExport.
  const voiceChosen = useRef(false);
  const languageChosen = useRef(false);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const savedTimer = useRef<number | null>(null);

  /**
   * Shows the tick for a moment, replacing any tick still counting down.
   * A bare `setTimeout` let an earlier save's timer clear a later save's
   * confirmation, and kept firing after the reader navigated away -- the same
   * fix as `showSavedFor` in FishAudioSettings.
   */
  function showSaved() {
    if (savedTimer.current !== null) {
      window.clearTimeout(savedTimer.current);
    }
    setSaved(true);
    savedTimer.current = window.setTimeout(() => {
      savedTimer.current = null;
      setSaved(false);
    }, 1600);
  }

  useEffect(
    () => () => {
      if (savedTimer.current !== null) {
        window.clearTimeout(savedTimer.current);
      }
    },
    [],
  );
  // Which provider a click is currently writing, if any. `setTtsProvider`
  // applies only once its write lands (so playback never builds an engine for
  // a provider that did not stick), which means nothing in the picker moves
  // until then -- on a slow write the control looked inert and invited a
  // second click that just queued another write behind the first.
  const [switchingTo, setSwitchingTo] = useState<TtsProvider | null>(null);
  // Identifies *which click* the pending state belongs to. Keying on the
  // provider id instead let an earlier write's completion clear a later
  // click's spinner: click Fish, click Supertonic, click Fish again, and
  // write #1 landing would find `switchingTo === "fish"` and clear it while
  // #2 and #3 were still queued -- the picker inert with writes outstanding,
  // which is the state this was added to remove.
  const switchSeq = useRef(0);
  const [testing, setTesting] = useState(false);
  const [testStatus, setTestStatus] = useState<string | null>(null);
  const [testError, setTestError] = useState<string | null>(null);
  // Set instead of testing directly whenever the active provider bills for a
  // synthesis. Cleared on confirm or cancel, and by a provider change, so a
  // confirmation naming one provider can never be accepted for another.
  const [pendingTest, setPendingTest] = useState(false);
  const [supertonicModelStatus, setSupertonicModelStatus] =
    useState<SupertonicModelStatus | null>(null);
  const [supertonicModelProgress, setSupertonicModelProgress] =
    useState<SupertonicModelProgress | null>(null);
  const [downloadingSupertonicModel, setDownloadingSupertonicModel] =
    useState(false);
  const [supertonicModelError, setSupertonicModelError] = useState<
    string | null
  >(null);

  // Seed the drafts from the store once settings finish loading -- not on
  // every change to those rows. The draft belongs to the reader from the
  // moment they touch it: keyed on the rows, any later write to them would
  // pull a pending pick out from under them mid-edit. Hooks run before the
  // `!hydrated` gate below, so the `useState` initialisers cannot do this on
  // their own -- on a mount that beats `hydrate()` they capture the built-in
  // defaults.
  useEffect(() => {
    // `!hydrateFailed` as well as `hydrated`: a failed load reports hydrated
    // with every row at DEFAULT_SETTINGS, and this is also what re-seeds the
    // drafts when a retry finally brings the reader's rows in -- `hydrated`
    // is already true by then and would never fire again on its own.
    if (!hydrated || hydrateFailed) {
      return;
    }
    const settings = useSettingsStore.getState();
    if (!voiceChosen.current) {
      setVoiceStyle(settings.supertonicVoiceStyle);
    }
    if (!languageChosen.current) {
      setLanguage(settings.supertonicLanguage);
    }
  }, [hydrated, hydrateFailed]);

  // A provider change invalidates any confirmation already on screen: it
  // named a provider and a cost that no longer apply.
  useEffect(() => {
    setPendingTest(false);
  }, [ttsProvider]);

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
      showSaved();
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

  /**
   * The money gate for the test button.
   *
   * A provider that bills is never tested on a single click: this records the
   * request and the confirmation below spends it, naming the provider and the
   * exact character count first. A free provider goes straight through --
   * a confirmation on something that costs nothing only teaches people to
   * dismiss the one that doesn't.
   */
  function requestTest() {
    setTestError(null);
    if (activeProvider.bills) {
      setPendingTest(true);
      return;
    }
    void testProvider();
  }

  function confirmTest() {
    setPendingTest(false);
    void testProvider();
  }

  async function testProvider() {
    setTesting(true);
    setTestStatus(`Loading ${providerLabel}...`);
    setTestError(null);

    try {
      const engine = createSpeechEngine({
        ttsProvider,
        supertonicLanguage: language,
        // The pending selection, not the saved row -- Test previews what the
        // reader is about to save, exactly as `language` above does.
        supertonicVoiceStyle: voiceStyle,
        fishVoiceId,
        // A Test is the reader asking for this engine by name, so it may
        // fetch the model. The button is disabled outright unless the rows
        // loaded, so it can never be asking about a guessed provider.
        settingsSource: "loaded",
      });
      await engine.ensureReady(setTestStatus);

      setTestStatus(`Generating ${providerLabel} sample...`);
      // No voice on the request: the engine built above already holds the
      // one being tested (the pending `voiceStyle` for Supertonic, the saved
      // `fishVoiceId` for Fish). Passing one here read as if it selected the
      // test voice, and did not.
      const blob = await engine.synthesize({ text: SAMPLE_TEXT, speed: 1 });

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

  // `!hydrated`, not `!hydrated && loading`: the gap between mount and
  // `hydrate()` setting `loading` used to render fully editable controls
  // seeded from DEFAULT_SETTINGS, and a style picked in it was silently
  // overwritten when hydration landed. `hydrate` sets `hydrated` on its
  // failure path too, so this can never strand the panel on "Loading...".
  if (!hydrated) {
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
              aria-busy={switchingTo === provider.id}
              aria-pressed={ttsProvider === provider.id}
              className={`rounded-md border px-4 py-3 text-left text-sm transition-colors focus:outline-none focus:ring-2 focus:ring-brand-500 disabled:cursor-not-allowed disabled:opacity-60 ${
                ttsProvider === provider.id
                  ? "border-brand-500 bg-brand-50 dark:border-brand-500 dark:bg-neutral-800"
                  : "border-neutral-200 hover:bg-stone-100 dark:border-neutral-800 dark:hover:bg-neutral-800"
              }`}
              // Only the button being written, never both: a write that never
              // settles would otherwise leave the whole picker dead with no
              // way back, and the per-row queue in `writeRow` already makes
              // concurrent clicks safe -- the last one clicked is the last one
              // committed.
              //
              // `hydrateFailed` disables both, for the reason Save is
              // disabled: the highlight below is DEFAULT_SETTINGS, not the
              // reader's stored provider, so every option here -- including
              // the one that looks already chosen -- would commit a provider
              // this screen never showed them.
              disabled={switchingTo === provider.id || hydrateFailed}
              key={provider.id}
              onClick={() => {
                // Deliberately leaves `saveError` alone. These are separate
                // actions with separate error lines, and clearing it here
                // erased a "disk full" the reader may not have read yet --
                // about a voice and language that still are not saved.
                const seq = ++switchSeq.current;
                setSwitchingTo(provider.id);
                // setTtsProvider rethrows on a failed persist; this button
                // doesn't await it, so it must catch here or the rejection
                // goes unhandled. The `error` block below already renders
                // the store's shared error field on failure.
                void setTtsProvider(provider.id)
                  .catch(() => {})
                  .finally(() => {
                    if (switchSeq.current === seq) {
                      setSwitchingTo(null);
                    }
                  });
              }}
              type="button"
            >
              <span className="flex items-center gap-2 font-medium">
                {provider.label}
                {switchingTo === provider.id ? (
                  <Loader2
                    aria-label={`Switching to ${provider.label}`}
                    className="size-4 animate-spin"
                  />
                ) : null}
              </span>
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
        {/*
          Names the provider actually in use, not always "Supertonic". This
          card holds the Save and Test buttons, and Test speaks through the
          active engine -- labelling the card "Supertonic" while it sent a
          billed Fish Audio request was the contradiction this replaced. The
          Supertonic-only controls below carry their own heading.
        */}
        <div className="rounded-md border border-neutral-200 bg-stone-50 px-3 py-2 text-sm dark:border-neutral-800 dark:bg-neutral-950">
          <span className="font-medium">{providerLabel}</span>
          <span className="ml-2 text-neutral-500 dark:text-neutral-400">
            {activeProvider.hint}
          </span>
        </div>

        <div className="mt-5 rounded-md border border-neutral-200 bg-stone-50 p-4 dark:border-neutral-800 dark:bg-neutral-950">
          <h3 className="mb-3 text-sm font-semibold">Supertonic settings</h3>
          <div className="grid gap-4 lg:grid-cols-2">
            <label className="flex flex-col gap-2 text-sm font-medium">
              Voice style
              <select
                className="h-10 rounded-md border border-neutral-200 bg-white px-3 text-sm font-normal outline-none focus:border-brand-500 focus:ring-2 focus:ring-brand-500/20 dark:border-neutral-800 dark:bg-neutral-950"
                onChange={(event) => {
                  voiceChosen.current = true;
                  setVoiceStyle(event.target.value as SupertonicVoiceStyle);
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

            <label className="flex flex-col gap-2 text-sm font-medium">
              Language
              <select
                className="h-10 rounded-md border border-neutral-200 bg-white px-3 text-sm font-normal outline-none focus:border-brand-500 focus:ring-2 focus:ring-brand-500/20 dark:border-neutral-800 dark:bg-neutral-950"
                onChange={(event) => {
                  languageChosen.current = true;
                  setLanguage(event.target.value as SupertonicLanguage);
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

        {hydrateFailed ? (
          <div className="mt-4 flex flex-wrap items-center justify-between gap-3 rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-sm text-amber-800 dark:border-amber-950 dark:bg-amber-950/30 dark:text-amber-300">
            <p>
              Your saved settings could not be loaded, so these are the
              built-in defaults -- including the engine and the voice you are
              being read in. Saving and switching engines are off until they
              load, so this screen cannot write over the settings you actually
              have.
            </p>
            <button
              className="inline-flex h-9 shrink-0 items-center gap-2 rounded-md border border-amber-300 px-3 text-sm font-medium hover:bg-amber-100 focus:outline-none focus:ring-2 focus:ring-brand-500 disabled:cursor-not-allowed disabled:opacity-60 dark:border-amber-800 dark:hover:bg-amber-900/40"
              disabled={retryingHydrate}
              onClick={() => {
                setRetryingHydrate(true);
                void hydrate().finally(() => setRetryingHydrate(false));
              }}
              type="button"
            >
              {retryingHydrate ? (
                <Loader2 className="size-4 animate-spin" aria-hidden="true" />
              ) : null}
              Try again
            </button>
          </div>
        ) : null}

        <div className="mt-4 flex flex-wrap items-center gap-3">
          <button
            className="inline-flex h-10 items-center gap-2 rounded-md bg-brand-700 px-4 text-sm font-medium text-white hover:bg-brand-500 focus:outline-none focus:ring-2 focus:ring-brand-500 disabled:cursor-not-allowed disabled:opacity-50"
            disabled={saving || hydrateFailed}
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
            // `hydrateFailed` too: the provider shown above is
            // DEFAULT_SETTINGS, not the reader's, and this is the one control
            // that would act on it -- building that engine and, for
            // Supertonic, fetching its model.
            disabled={testing || hydrateFailed}
            onClick={requestTest}
            type="button"
          >
            {testing ? (
              <Loader2 className="size-4 animate-spin" aria-hidden="true" />
            ) : (
              <Play className="size-4" aria-hidden="true" />
            )}
            Test {providerLabel} voice
            {activeProvider.bills ? " (billed)" : ""}
          </button>
        </div>

        {activeProvider.bills ? (
          <p className="mt-2 text-xs text-neutral-500 dark:text-neutral-400">
            Testing {providerLabel} sends {SAMPLE_TEXT.length} characters to
            their servers and bills your account.
          </p>
        ) : null}

        {pendingTest ? (
          <div className="mt-3 rounded-md border border-amber-300 bg-amber-50 p-4 text-sm dark:border-amber-900 dark:bg-amber-950/30">
            <p className="font-semibold text-amber-900 dark:text-amber-200">
              Confirm {providerLabel} voice test
            </p>
            <p className="mt-1 text-amber-800 dark:text-amber-300">
              This will send{" "}
              <strong>{SAMPLE_TEXT.length} characters</strong> to{" "}
              {providerLabel} and bill your account. Nothing is cached for a
              test, so repeating it bills again.
            </p>
            <div className="mt-3 flex flex-wrap gap-2">
              <button
                className="inline-flex h-9 items-center gap-2 rounded-md bg-amber-700 px-4 text-sm font-medium text-white hover:bg-amber-600 focus:outline-none focus:ring-2 focus:ring-amber-500"
                onClick={confirmTest}
                type="button"
              >
                Confirm and test with {providerLabel}
              </button>
              <button
                className="inline-flex h-9 items-center gap-2 rounded-md border border-amber-300 px-4 text-sm font-medium text-amber-900 hover:bg-amber-100 focus:outline-none focus:ring-2 focus:ring-amber-500 dark:border-amber-800 dark:text-amber-200 dark:hover:bg-amber-950/60"
                onClick={() => setPendingTest(false)}
                type="button"
              >
                Cancel
              </button>
            </div>
          </div>
        ) : null}
      </div>

      <FishAudioSettings
        onKeyStatusChange={(status: FishKeyStatus) =>
          setFishKeyPresent(status.present)
        }
      />

      {/*
        The shared banner: a failed hydrate, and the provider buttons above,
        which have no error line of their own. A failed TTS save never reaches
        it -- `saveTtsSettings` only rejects, and the two callers that catch it
        (this panel's Save, and the Fish voice control) each render the message
        beside their own control.
      */}
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
