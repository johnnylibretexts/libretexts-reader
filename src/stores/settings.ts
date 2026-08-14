import { create } from "zustand";
import {
  asSupertonicLanguage,
  asSupertonicVoiceStyle,
  type SupertonicLanguage,
  type SupertonicVoiceStyle,
} from "../lib/supertonic";
import { api } from "../lib/tauri";
import { displayError } from "../lib/errors";
import type { TtsProvider } from "../types/domain";

export type AppTheme = "light" | "dark" | "system";
/**
 * Mirrors `SpeechEngineId` — every provider here is one a reader can pick.
 * Re-exported from `types/domain.ts`, which is also where `tauri.ts` gets it
 * for the `provider` field Rust now requires on synthesis and chapter
 * requests; declaring it there once means this store and the invoke layer
 * cannot drift apart without a compile error.
 */
export type { TtsProvider };

export interface TtsSettingsPatch {
  ttsProvider?: TtsProvider;
  supertonicVoiceStyle?: SupertonicVoiceStyle;
  supertonicLanguage?: SupertonicLanguage;
  fishVoiceId?: string | null;
}

export interface SettingsState {
  defaultVoiceId: string;
  defaultSpeed: number;
  exportDirectory: string;
  theme: AppTheme;
  telemetryOptIn: boolean;
  autoCheckUpdates: boolean;
  ttsProvider: TtsProvider;
  supertonicVoiceStyle: SupertonicVoiceStyle;
  supertonicLanguage: SupertonicLanguage;
  /**
   * The reader's chosen Fish voice id, or null when none has been picked yet.
   * Settings UI for this lands in a later task; declared here now because
   * `createSpeechEngine` requires it on every `SpeechEngineSettings`, and this
   * store hands its whole state to that function.
   */
  fishVoiceId: string | null;
}

interface SettingsStore extends SettingsState {
  hydrated: boolean;
  loading: boolean;
  error: string | null;
  hydrate: () => Promise<void>;
  setTheme: (theme: AppTheme) => Promise<void>;
  setTtsProvider: (provider: TtsProvider) => Promise<void>;
  saveTtsSettings: (settings: TtsSettingsPatch) => Promise<void>;
}

const DEFAULT_SETTINGS: SettingsState = {
  defaultVoiceId: "M1",
  defaultSpeed: 1,
  exportDirectory: "",
  theme: "system",
  telemetryOptIn: false,
  autoCheckUpdates: true,
  ttsProvider: "supertonic",
  supertonicVoiceStyle: "M1",
  supertonicLanguage: "en",
  fishVoiceId: null,
};

export const useSettingsStore = create<SettingsStore>((set, get) => ({
  ...DEFAULT_SETTINGS,
  hydrated: false,
  loading: false,
  error: null,
  hydrate: async () => {
    const { hydrated, loading } = get();
    if (hydrated || loading) {
      return;
    }

    set({ loading: true, error: null });
    try {
      // Legacy stored values are coerced in asTtsProvider, so anything that
      // survives loadSettings is already a provider a reader can pick.
      const loaded = await loadSettings();
      set({
        ...DEFAULT_SETTINGS,
        ...definedSettings(loaded),
        hydrated: true,
        loading: false,
      });
    } catch (error) {
      set({
        ...DEFAULT_SETTINGS,
        theme: localTheme() ?? DEFAULT_SETTINGS.theme,
        hydrated: true,
        loading: false,
        error: displayError(error),
      });
    }
  },
  setTheme: async (theme: AppTheme) => {
    const previousTheme = get().theme;
    set({ theme, error: null });
    persistLocalTheme(theme);

    try {
      await api.setSetting("theme", theme);
    } catch (error) {
      // Revert the optimistic set before rethrowing: a failed persist must
      // not leave the store (or the localStorage fallback `hydrate` reads
      // on its own failure path) claiming a value that was never saved, or
      // this session disagrees with what the next app start loads. Set the
      // shared banner message, then rethrow so a caller awaiting this call
      // sees the same failure via its own try/catch rather than having to
      // read this store's mutable `error` field, which a concurrent
      // unrelated action could overwrite or clear first. Every call site
      // must catch this — see Sidebar.tsx.
      set({ theme: previousTheme, error: displayError(error) });
      persistLocalTheme(previousTheme);
      throw error;
    }
  },
  setTtsProvider: async (provider: TtsProvider) => {
    const previousProvider = get().ttsProvider;
    set({ ttsProvider: provider, error: null });

    try {
      await api.setSetting("tts_provider", provider);
    } catch (error) {
      // Revert before rethrowing -- see the note on setTheme above. Without
      // this, a failed save leaves the store (and therefore the UI) showing
      // a provider that was never persisted, disagreeing with what the next
      // app start loads from the settings table.
      set({ ttsProvider: previousProvider, error: displayError(error) });
      throw error;
    }
  },
  saveTtsSettings: async (ttsSettings: TtsSettingsPatch) => {
    const supertonicVoiceStyle =
      ttsSettings.supertonicVoiceStyle ??
      get().supertonicVoiceStyle ??
      DEFAULT_SETTINGS.supertonicVoiceStyle;
    const supertonicLanguage =
      ttsSettings.supertonicLanguage ??
      get().supertonicLanguage ??
      DEFAULT_SETTINGS.supertonicLanguage;
    const ttsProvider =
      ttsSettings.ttsProvider ??
      get().ttsProvider ??
      DEFAULT_SETTINGS.ttsProvider;
    const fishVoiceId =
      ttsSettings.fishVoiceId !== undefined
        ? ttsSettings.fishVoiceId
        : get().fishVoiceId;
    set({
      ttsProvider,
      supertonicVoiceStyle,
      supertonicLanguage,
      fishVoiceId,
      error: null,
    });

    try {
      await Promise.all([
        api.setSetting("tts_provider", ttsProvider),
        api.setSetting("supertonic_voice_style", supertonicVoiceStyle),
        api.setSetting("supertonic_language", supertonicLanguage),
        api.setSetting("fish_voice_id", fishVoiceId),
      ]);
    } catch (error) {
      // See the rethrow note on setTheme above; the same reasoning applies.
      set({ error: displayError(error) });
      throw error;
    }
  },
}));

export async function loadSettings(): Promise<Partial<SettingsState>> {
  const settings = await api.getAllSettings();
  return definedSettings({
    defaultVoiceId: asString(settings.default_voice_id),
    defaultSpeed: asNumber(settings.default_speed),
    exportDirectory: asString(settings.export_directory),
    theme: asTheme(settings.theme),
    telemetryOptIn: asBoolean(settings.telemetry_opt_in),
    autoCheckUpdates: asBoolean(settings.auto_check_updates),
    ttsProvider: asTtsProvider(settings.tts_provider),
    supertonicVoiceStyle: asSupertonicVoiceStyle(
      settings.supertonic_voice_style,
    ),
    supertonicLanguage: asSupertonicLanguage(settings.supertonic_language),
    fishVoiceId: asString(settings.fish_voice_id) ?? null,
  });
}

function definedSettings(settings: Partial<SettingsState>) {
  return Object.fromEntries(
    Object.entries(settings).filter(([, value]) => value !== undefined),
  ) as Partial<SettingsState>;
}

function asString(value: unknown): string | undefined {
  return typeof value === "string" ? value : undefined;
}

function asNumber(value: unknown): number | undefined {
  return typeof value === "number" ? value : undefined;
}

function asBoolean(value: unknown): boolean | undefined {
  return typeof value === "boolean" ? value : undefined;
}

function asTheme(value: unknown): AppTheme | undefined {
  return value === "light" || value === "dark" || value === "system"
    ? value
    : undefined;
}

/**
 * The single place stored provider values are interpreted, including retired
 * ones. `system` was the Web Speech path, removed once every engine sat behind
 * SpeechEngine; `gemini` predates Supertonic; `kokoro` was removed once it
 * proved it could not produce audio in a bundled build. Retired values fall
 * back to the default rather than being written back — the next settings save
 * overwrites the stale row anyway.
 *
 * `fish` is no longer retired: it is a real, selectable provider again (see
 * `SpeechEngineId` in `lib/speech/types.ts`).
 */
function asTtsProvider(value: unknown): TtsProvider | undefined {
  if (value === "gemini" || value === "kokoro") {
    return "supertonic";
  }
  return value === "supertonic" || value === "fish" ? value : undefined;
}

// The localStorage key is intentionally still "johnny-reader-theme" from the
// pre-rename app name. Do NOT rename it to match the LibreTexts Reader
// rebrand — every existing user's stored key would stop matching and their
// theme preference would silently reset to "system". Leave this key alone.
function localTheme(): AppTheme | undefined {
  if (typeof window === "undefined") {
    return undefined;
  }

  return asTheme(window.localStorage.getItem("johnny-reader-theme"));
}

function persistLocalTheme(theme: AppTheme) {
  if (typeof window !== "undefined") {
    window.localStorage.setItem("johnny-reader-theme", theme);
  }
}
