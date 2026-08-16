import { useEffect, useState } from "react";
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
  const [voiceSaved, setVoiceSaved] = useState(false);
  const [voiceError, setVoiceError] = useState<string | null>(null);

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

  async function persistVoice(voiceId: string) {
    const trimmed = voiceId.trim();
    if (!trimmed) {
      return;
    }

    setSavingVoice(true);
    setVoiceError(null);
    setVoiceSaved(false);
    try {
      // saveTtsSettings rethrows on a failed persist (after recording the
      // message in the shared settings store for the banner elsewhere), so
      // this ordinary try/catch is what detects a failure -- not a read of
      // the store's mutable `error` field, which a concurrent unrelated
      // settings action (e.g. a Supertonic save in flight) could otherwise
      // overwrite or clear out from under this call.
      await saveTtsSettings({ fishVoiceId: trimmed });
      setCustomVoiceId("");
      setVoiceSaved(true);
      window.setTimeout(() => setVoiceSaved(false), 1600);
    } catch (error) {
      setVoiceError(displayError(error));
    } finally {
      setSavingVoice(false);
    }
  }

  // A sentinel, not a real Fish voice id, so it can never collide with one.
  const PASTED_VOICE_OPTION = "__pasted-voice__";

  const isKnownVoice =
    !!fishVoiceId && !!voices?.some((voice) => voice.id === fishVoiceId);
  // The common case: the reader's chosen voice is a public model they don't
  // own, so it never appears in `voices` (their own models only). Without
  // this, the <select> falls back to its blank "Choose a voice" option and
  // reads as "nothing selected" even though a voice is very much in effect.
  const isPastedVoice = !!fishVoiceId && !isKnownVoice;
  const selectedVoiceOption = isKnownVoice
    ? (fishVoiceId as string)
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
            Your voices
            <select
              className="h-10 rounded-md border border-neutral-200 bg-white px-3 text-sm font-normal outline-none focus:border-brand-500 focus:ring-2 focus:ring-brand-500/20 dark:border-neutral-800 dark:bg-neutral-950"
              disabled={savingVoice}
              onChange={(event) => {
                if (
                  event.target.value &&
                  event.target.value !== PASTED_VOICE_OPTION
                ) {
                  void persistVoice(event.target.value);
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
                disabled={customVoiceId.trim().length === 0 || savingVoice}
                onClick={() => void persistVoice(customVoiceId)}
                type="button"
              >
                {savingVoice ? (
                  <Loader2 className="size-4 animate-spin" aria-hidden="true" />
                ) : voiceSaved ? (
                  <Check className="size-4" aria-hidden="true" />
                ) : (
                  <Save className="size-4" aria-hidden="true" />
                )}
                Use voice
              </button>
            </div>
          </label>
        </div>

        <p className="mt-3 text-xs text-neutral-500 dark:text-neutral-400">
          {fishVoiceId
            ? `Current voice id: ${fishVoiceId}`
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
