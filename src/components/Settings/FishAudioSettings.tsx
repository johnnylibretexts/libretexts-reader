import { useEffect, useRef, useState } from "react";
import {
  Check,
  ExternalLink,
  Key,
  Loader2,
  Save,
  Trash2,
} from "lucide-react";
import { api, isTauriRuntime, type FishKeyStatus } from "../../lib/tauri";
import { displayError } from "../../lib/errors";
import type { SpeechVoice } from "../../lib/speech/types";
import { useSettingsStore } from "../../stores/settings";

const FISH_API_KEYS_URL = "https://fish.audio/app/api-keys";

export interface FishAudioSettingsProps {
  /**
   * Reported whenever this component learns the key's presence, so a parent
   * (the provider picker in SettingsPanel) can warn about a missing key
   * without invoking `getFishKeyStatus` a second time. This component works
   * fine without it -- it is an integration hook, not a dependency.
   */
  onKeyStatusChange?: (status: FishKeyStatus) => void;
}

/**
 * Fish Audio API key entry and voice selection.
 *
 * The key itself never enters this component's state for longer than a save
 * call needs, and is never rendered: there is no command that returns it, by
 * design (see `src-tauri/src/commands/fish.rs`).
 */
export function FishAudioSettings({
  onKeyStatusChange,
}: FishAudioSettingsProps) {
  const fishVoiceId = useSettingsStore((state) => state.fishVoiceId);
  const saveTtsSettings = useSettingsStore((state) => state.saveTtsSettings);
  // The store is showing DEFAULT_SETTINGS rather than the reader's rows (see
  // `hydrateFailed`), so "Current voice id" below reads as unset whatever
  // they actually have saved. Choosing here would write over a voice this panel
  // never showed them -- the same reason Settings disables Save.
  const settingsFailed = useSettingsStore((state) => state.hydrateFailed);

  const [keyStatus, setKeyStatus] = useState<FishKeyStatus | null>(null);
  const [keyStatusError, setKeyStatusError] = useState<string | null>(null);
  const [editingKey, setEditingKey] = useState(false);
  const [keyInput, setKeyInput] = useState("");
  const [savingKey, setSavingKey] = useState(false);
  const [removingKey, setRemovingKey] = useState(false);
  const [keyError, setKeyError] = useState<string | null>(null);

  const [voices, setVoices] = useState<SpeechVoice[] | null>(null);
  const [loadingVoices, setLoadingVoices] = useState(false);
  const [voicesError, setVoicesError] = useState<string | null>(null);
  const [customVoiceId, setCustomVoiceId] = useState("");
  const [savingVoice, setSavingVoice] = useState(false);
  /**
   * Which control's save just succeeded, so only that one shows the tick.
   * Shared, it put a green check on the pasted-id button after a save started
   * from the dropdown -- on a button simultaneously disabled because no id
   * was pasted, for a paste that never happened.
   */
  const [savedFrom, setSavedFrom] = useState<"list" | "pasted" | null>(null);
  const savedTimer = useRef<number | null>(null);

  /**
   * Shows the tick for a moment, replacing any tick still counting down.
   * A bare `setTimeout` let an earlier save's timer clear a later save's
   * confirmation -- two saves inside 1.6s and the second showed none -- and
   * kept firing after the reader navigated away.
   */
  function showSavedFor(from: "list" | "pasted") {
    if (savedTimer.current !== null) {
      window.clearTimeout(savedTimer.current);
    }
    setSavedFrom(from);
    savedTimer.current = window.setTimeout(() => {
      savedTimer.current = null;
      setSavedFrom(null);
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
  const [voiceError, setVoiceError] = useState<string | null>(null);
  // The voice a save is currently writing. `saveTtsSettings` applies only
  // once the write lands, so without this the select -- controlled off the
  // store -- renders the *previous* voice for the whole round trip and the
  // reader watches their pick snap back.
  const [savingVoiceId, setSavingVoiceId] = useState<string | null>(null);
  /** Which control started the save, so only that one shows it running. */
  const [savingFrom, setSavingFrom] = useState<"list" | "pasted" | null>(null);

  // No network call -- safe to run on mount, per `key_status_from` in
  // `commands/fish.rs`.
  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }

    let cancelled = false;
    void api
      .getFishKeyStatus()
      .then((status) => {
        if (cancelled) {
          return;
        }
        setKeyStatus(status);
        onKeyStatusChange?.(status);
      })
      .catch((error) => {
        if (!cancelled) {
          setKeyStatusError(displayError(error));
        }
      });

    return () => {
      cancelled = true;
    };
    // Runs once. `onKeyStatusChange` is deliberately not a dependency: it is
    // stable across renders in every real caller (a Zustand setter), and
    // later status changes are reported from the handlers below.
    //
    // No eslint-disable here -- this project has no ESLint (its frontend
    // gates are `tsc` and vitest), so a suppression comment would suppress
    // nothing while implying a rule was in force.
  }, []);

  useEffect(() => {
    if (!keyStatus?.present) {
      setVoices(null);
      setVoicesError(null);
      return;
    }

    let cancelled = false;
    setLoadingVoices(true);
    setVoicesError(null);
    void api
      .listFishVoices()
      .then((list) => {
        if (!cancelled) {
          setVoices(list);
        }
      })
      .catch((error) => {
        if (!cancelled) {
          setVoicesError(displayError(error));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setLoadingVoices(false);
        }
      });

    return () => {
      cancelled = true;
    };
    // Keyed on the status object, not `keyStatus.present`. Replacing one key
    // with another leaves `present` true either side, so a boolean dep does
    // not re-run and the previous account's voice models stay listed --
    // picking one persists a reference_id the new account does not own, and
    // the first synthesis 404s into "Choose a Fish Audio voice in Settings"
    // while one is chosen. Every `setKeyStatus` call passes a fresh object,
    // so identity changes exactly when the stored key might have.
  }, [keyStatus]);

  function startReplace() {
    setKeyError(null);
    setKeyInput("");
    setEditingKey(true);
  }

  function cancelReplace() {
    setKeyError(null);
    setKeyInput("");
    setEditingKey(false);
  }

  async function handleSaveKey() {
    // Captured locally so the state can be cleared immediately: the key
    // never needs to sit in state any longer than this call takes.
    const key = keyInput;
    setKeyInput("");
    setSavingKey(true);
    setKeyError(null);
    try {
      const status = await api.setFishApiKey(key);
      setKeyStatus(status);
      setEditingKey(false);
      onKeyStatusChange?.(status);
    } catch (error) {
      // set_fish_api_key validates before storing, so a rejected key never
      // reaches the keychain; displayError surfaces Rust's "auth" message
      // ("Fish Audio rejected the API key: ...") rather than a generic one.
      setKeyError(displayError(error));
    } finally {
      setSavingKey(false);
    }
  }

  async function handleRemoveKey() {
    setRemovingKey(true);
    setKeyError(null);
    try {
      await api.clearFishApiKey();
      const status: FishKeyStatus = {
        present: false,
        valid: null,
        credit: null,
      };
      setKeyStatus(status);
      setEditingKey(false);
      setKeyInput("");
      onKeyStatusChange?.(status);
    } catch (error) {
      setKeyError(displayError(error));
    } finally {
      setRemovingKey(false);
    }
  }

  async function persistVoice(voiceId: string, from: "list" | "pasted") {
    const trimmed = voiceId.trim();
    if (!trimmed) {
      return;
    }

    setSavingVoice(true);
    setSavingVoiceId(trimmed);
    setSavingFrom(from);
    setVoiceError(null);
    setSavedFrom(null);
    try {
      // saveTtsSettings reports a failed persist only by rejecting -- it does
      // not touch the store's shared `error` field, precisely so this catch
      // is the single place the message comes from. Reading that field
      // instead would be unreliable anyway: a concurrent unrelated settings
      // action could overwrite or clear it out from under this call.
      await saveTtsSettings({ fishVoiceId: trimmed });
      setCustomVoiceId("");
      showSavedFor(from);
    } catch (error) {
      setVoiceError(displayError(error));
    } finally {
      setSavingVoice(false);
      setSavingVoiceId(null);
      setSavingFrom(null);
    }
  }

  // A sentinel, not a real Fish voice id, so it can never collide with one.
  const PASTED_VOICE_OPTION = "__pasted-voice__";

  // The voice as of the reader's latest intent: while a save is in flight,
  // what they just chose; otherwise what is stored. Everything below derives
  // from this one value rather than each deciding for itself -- deriving
  // `selectedVoiceOption` from the pending pick while `isPastedVoice` still
  // came from the stored id gave the <select> a value whose <option> was not
  // rendered, so a pasted save blanked the control to "Choose a voice" for
  // the whole round trip. `saveTtsSettings` applies only once the write
  // lands, so the stored id is precisely the one that has not caught up yet.
  const effectiveVoiceId = savingVoiceId ?? fishVoiceId;

  const isKnownVoice =
    !!effectiveVoiceId &&
    !!voices?.some((voice) => voice.id === effectiveVoiceId);
  // The common case: the reader's chosen voice is a public model they don't
  // own, so it never appears in `voices` (their own models only). Without
  // this, the <select> falls back to its blank "Choose a voice" option and
  // reads as "nothing selected" even though a voice is very much in effect.
  const isPastedVoice = !!effectiveVoiceId && !isKnownVoice;
  const selectedVoiceOption = isKnownVoice
    ? (effectiveVoiceId as string)
    : isPastedVoice
      ? PASTED_VOICE_OPTION
      : "";

  return (
    <div className="rounded-md border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900">
      <div className="rounded-md border border-neutral-200 bg-stone-50 px-3 py-2 text-sm dark:border-neutral-800 dark:bg-neutral-950">
        <span className="font-medium">Fish Audio</span>
        <span className="ml-2 text-neutral-500 dark:text-neutral-400">
          Cloud voice cloning
        </span>
      </div>

      <div className="mt-5 rounded-md border border-neutral-200 bg-stone-50 p-4 dark:border-neutral-800 dark:bg-neutral-950">
        {keyStatus === null ? (
          <p className="inline-flex items-center gap-2 text-sm text-neutral-500 dark:text-neutral-400">
            <Loader2 className="size-4 animate-spin" aria-hidden="true" />
            Checking API key...
          </p>
        ) : keyStatus.present && !editingKey ? (
          <div className="flex flex-wrap items-center justify-between gap-3">
            <p className="inline-flex items-center gap-2 text-sm font-medium">
              <Key className="size-4 text-brand-700" aria-hidden="true" />A
              key is saved.
            </p>
            <div className="flex gap-2">
              <button
                className="inline-flex h-9 items-center gap-2 rounded-md border border-neutral-200 px-3 text-sm font-medium text-neutral-700 hover:bg-stone-100 focus:outline-none focus:ring-2 focus:ring-brand-500 dark:border-neutral-800 dark:text-neutral-200 dark:hover:bg-neutral-800"
                onClick={startReplace}
                type="button"
              >
                Replace
              </button>
              <button
                className="inline-flex h-9 items-center gap-2 rounded-md border border-neutral-200 px-3 text-sm font-medium text-neutral-700 hover:bg-stone-100 focus:outline-none focus:ring-2 focus:ring-brand-500 disabled:cursor-not-allowed disabled:opacity-50 dark:border-neutral-800 dark:text-neutral-200 dark:hover:bg-neutral-800"
                disabled={removingKey}
                onClick={() => void handleRemoveKey()}
                type="button"
              >
                {removingKey ? (
                  <Loader2 className="size-4 animate-spin" aria-hidden="true" />
                ) : (
                  <Trash2 className="size-4" aria-hidden="true" />
                )}
                Remove
              </button>
            </div>
          </div>
        ) : (
          <div className="flex flex-col gap-3">
            <label className="flex flex-col gap-2 text-sm font-medium">
              Fish Audio API key
              <input
                autoComplete="off"
                autoFocus={editingKey}
                className="h-10 rounded-md border border-neutral-200 bg-white px-3 text-sm font-normal outline-none focus:border-brand-500 focus:ring-2 focus:ring-brand-500/20 dark:border-neutral-800 dark:bg-neutral-950"
                onChange={(event) => setKeyInput(event.target.value)}
                placeholder="Paste your Fish Audio API key"
                type="password"
                value={keyInput}
              />
            </label>
            <div className="flex flex-wrap items-center gap-3">
              <button
                className="inline-flex h-10 items-center gap-2 rounded-md bg-brand-700 px-4 text-sm font-medium text-white hover:bg-brand-500 focus:outline-none focus:ring-2 focus:ring-brand-500 disabled:cursor-not-allowed disabled:opacity-50"
                disabled={savingKey || keyInput.trim().length === 0}
                onClick={() => void handleSaveKey()}
                type="button"
              >
                {savingKey ? (
                  <Loader2 className="size-4 animate-spin" aria-hidden="true" />
                ) : (
                  <Save className="size-4" aria-hidden="true" />
                )}
                Save
              </button>
              {keyStatus?.present ? (
                <button
                  className="inline-flex h-10 items-center gap-2 rounded-md border border-neutral-200 px-4 text-sm font-medium text-neutral-700 hover:bg-stone-100 focus:outline-none focus:ring-2 focus:ring-brand-500 dark:border-neutral-800 dark:text-neutral-200 dark:hover:bg-neutral-800"
                  disabled={savingKey}
                  onClick={cancelReplace}
                  type="button"
                >
                  Cancel
                </button>
              ) : null}
              <a
                className="inline-flex items-center gap-1 text-sm text-brand-700 hover:underline dark:text-brand-500"
                href={FISH_API_KEYS_URL}
                rel="noreferrer"
                target="_blank"
              >
                Get an API key
                <ExternalLink className="size-3.5" aria-hidden="true" />
              </a>
            </div>
          </div>
        )}
      </div>

      {keyStatusError ? (
        <p className="mt-3 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700 dark:border-red-950 dark:bg-red-950/30 dark:text-red-300">
          {keyStatusError}
        </p>
      ) : null}

      {keyError ? (
        <p className="mt-3 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700 dark:border-red-950 dark:bg-red-950/30 dark:text-red-300">
          {keyError}
        </p>
      ) : null}

      <div className="mt-5 rounded-md border border-neutral-200 bg-stone-50 p-4 dark:border-neutral-800 dark:bg-neutral-950">
        <h3 className="text-sm font-semibold">Voice</h3>

        {!keyStatus?.present ? (
          <p className="mt-2 text-sm text-neutral-500 dark:text-neutral-400">
            Add an API key above to see your saved voices.
          </p>
        ) : loadingVoices ? (
          <p className="mt-2 inline-flex items-center gap-2 text-sm text-neutral-500 dark:text-neutral-400">
            <Loader2 className="size-4 animate-spin" aria-hidden="true" />
            Loading voices...
          </p>
        ) : (
          <label className="mt-2 flex flex-col gap-2 text-sm font-medium">
            <span className="flex items-center gap-2">
              Your voices
              {savingFrom === "list" ? (
                // `aria-hidden` so the select's accessible name stays "Your
                // voices" rather than becoming "Your voices Saving...". The
                // status reaches assistive tech through `aria-busy` on the
                // select instead, which is what it describes.
                <span
                  aria-hidden="true"
                  className="inline-flex items-center gap-1 text-xs font-normal text-neutral-500 dark:text-neutral-400"
                >
                  <Loader2 className="size-3 animate-spin" />
                  Saving...
                </span>
              ) : null}
            </span>
            <select
              aria-busy={savingFrom === "list"}
              className="h-10 rounded-md border border-neutral-200 bg-white px-3 text-sm font-normal outline-none focus:border-brand-500 focus:ring-2 focus:ring-brand-500/20 dark:border-neutral-800 dark:bg-neutral-950"
              disabled={savingVoice || settingsFailed}
              onChange={(event) => {
                if (
                  event.target.value &&
                  event.target.value !== PASTED_VOICE_OPTION
                ) {
                  void persistVoice(event.target.value, "list");
                }
              }}
              value={selectedVoiceOption}
            >
              <option value="">
                {voices && voices.length > 0
                  ? "Choose a voice"
                  : "No voice models yet"}
              </option>
              {isPastedVoice ? (
                <option value={PASTED_VOICE_OPTION}>
                  Using a pasted voice id (current)
                </option>
              ) : null}
              {voices?.map((voice) => (
                <option
                  disabled={!voice.ready}
                  key={voice.id}
                  value={voice.id}
                >
                  {voice.name}
                  {voice.ready ? "" : " (training)"}
                </option>
              ))}
            </select>
          </label>
        )}

        {voicesError ? (
          <p className="mt-3 rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-sm text-amber-800 dark:border-amber-950 dark:bg-amber-950/30 dark:text-amber-300">
            {voicesError}
          </p>
        ) : null}

        <div className="mt-4 border-t border-neutral-200 pt-4 dark:border-neutral-800">
          <label className="flex flex-col gap-2 text-sm font-medium">
            Or paste a public voice id
            <span className="text-xs font-normal text-neutral-500 dark:text-neutral-400">
              The best narration voices are usually public models you do not
              own -- paste the id from a fish.audio voice page.
            </span>
            <div className="flex gap-2">
              <input
                className="h-10 flex-1 rounded-md border border-neutral-200 bg-white px-3 text-sm font-normal outline-none focus:border-brand-500 focus:ring-2 focus:ring-brand-500/20 dark:border-neutral-800 dark:bg-neutral-950"
                onChange={(event) => setCustomVoiceId(event.target.value)}
                placeholder="Voice id"
                type="text"
                value={customVoiceId}
              />
              <button
                className="inline-flex h-10 shrink-0 items-center gap-2 rounded-md border border-neutral-200 px-4 text-sm font-medium text-neutral-700 hover:bg-stone-100 focus:outline-none focus:ring-2 focus:ring-brand-500 disabled:cursor-not-allowed disabled:opacity-50 dark:border-neutral-800 dark:text-neutral-200 dark:hover:bg-neutral-800"
                disabled={
                  customVoiceId.trim().length === 0 ||
                  savingVoice ||
                  settingsFailed
                }
                onClick={() => void persistVoice(customVoiceId, "pasted")}
                type="button"
              >
                {savingFrom === "pasted" ? (
                  <Loader2 className="size-4 animate-spin" aria-hidden="true" />
                ) : savedFrom === "pasted" ? (
                  <Check className="size-4" aria-hidden="true" />
                ) : (
                  <Save className="size-4" aria-hidden="true" />
                )}
                Use voice
              </button>
            </div>
          </label>
        </div>

        {/*
          `effectiveVoiceId`, like everything else here: reading the stored id
          left this naming the previous voice for the whole round trip while
          the select beside it already said "Using a pasted voice id
          (current)" -- two controls, side by side, disagreeing about which
          voice is current.
        */}
        {settingsFailed ? (
          <p className="mt-3 rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-xs text-amber-800 dark:border-amber-950 dark:bg-amber-950/30 dark:text-amber-300">
            Your saved settings could not be loaded, so the voice below reads
            as unset whatever you actually have. Choosing here is off until
            they load, so this cannot write over it -- retry the load from the
            Supertonic settings above.
          </p>
        ) : null}

        <p className="mt-3 text-xs text-neutral-500 dark:text-neutral-400">
          {effectiveVoiceId
            ? `Current voice id: ${effectiveVoiceId}`
            : "No voice selected yet."}
        </p>

        {voiceError ? (
          <p className="mt-3 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700 dark:border-red-950 dark:bg-red-950/30 dark:text-red-300">
            {voiceError}
          </p>
        ) : null}
      </div>
    </div>
  );
}
