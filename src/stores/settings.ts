import { create } from "zustand";
import {
  asSupertonicLanguage,
  asSupertonicVoiceStyle,
  type SupertonicLanguage,
  type SupertonicVoiceStyle,
} from "../lib/supertonic";
import { api } from "../lib/tauri";
import { displayError } from "../lib/errors";

export type AppTheme = "light" | "dark" | "system";
export type ModelPrecision = "fp32" | "q8";
/** Mirrors `SpeechEngineId` — every provider here is one a reader can pick. */
export type TtsProvider = "kokoro" | "supertonic";

export interface TtsSettingsPatch {
  ttsProvider?: TtsProvider;
  supertonicVoiceStyle?: SupertonicVoiceStyle;
  supertonicLanguage?: SupertonicLanguage;
}

export interface SettingsState {
  defaultVoiceId: string;
  defaultSpeed: number;
  exportDirectory: string;
  modelPrecision: ModelPrecision;
  theme: AppTheme;
  telemetryOptIn: boolean;
  autoCheckUpdates: boolean;
  modelDownloaded: boolean;
  ttsProvider: TtsProvider;
  supertonicVoiceStyle: SupertonicVoiceStyle;
  supertonicLanguage: SupertonicLanguage;
}

interface SettingsStore extends SettingsState {
  hydrated: boolean;
  loading: boolean;
  error: string | null;
  hydrate: () => Promise<void>;
  markModelDownloaded: (precision: ModelPrecision) => void;
  setTheme: (theme: AppTheme) => Promise<void>;
  setTtsProvider: (provider: TtsProvider) => Promise<void>;
  saveTtsSettings: (settings: TtsSettingsPatch) => Promise<void>;
}

const DEFAULT_SETTINGS: SettingsState = {
  defaultVoiceId: "af_heart",
  defaultSpeed: 1,
  exportDirectory: "",
  modelPrecision: "q8",
  theme: "system",
  telemetryOptIn: false,
  autoCheckUpdates: true,
  modelDownloaded: false,
  ttsProvider: "kokoro",
  supertonicVoiceStyle: "M1",
  supertonicLanguage: "en",
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
  markModelDownloaded: (precision: ModelPrecision) => {
    set({
      modelDownloaded: true,
      modelPrecision: precision,
      error: null,
    });
  },
  setTheme: async (theme: AppTheme) => {
    set({ theme, error: null });
    persistLocalTheme(theme);

    try {
      await api.setSetting("theme", theme);
    } catch (error) {
      set({
        error: displayError(error),
      });
    }
  },
  setTtsProvider: async (provider: TtsProvider) => {
    set({ ttsProvider: provider, error: null });

    try {
      await api.setSetting("tts_provider", provider);
    } catch (error) {
      set({
        error: displayError(error),
      });
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
    set({
      ttsProvider,
      supertonicVoiceStyle,
      supertonicLanguage,
      error: null,
    });

    try {
      await Promise.all([
        api.setSetting("tts_provider", ttsProvider),
        api.setSetting("supertonic_voice_style", supertonicVoiceStyle),
        api.setSetting("supertonic_language", supertonicLanguage),
      ]);
    } catch (error) {
      set({
        error: displayError(error),
      });
    }
  },
}));

export async function loadSettings(): Promise<Partial<SettingsState>> {
  const settings = await api.getAllSettings();
  return definedSettings({
    defaultVoiceId: asString(settings.default_voice_id),
    defaultSpeed: asNumber(settings.default_speed),
    exportDirectory: asString(settings.export_directory),
    modelPrecision: asModelPrecision(settings.model_precision),
    theme: asTheme(settings.theme),
    telemetryOptIn: asBoolean(settings.telemetry_opt_in),
    autoCheckUpdates: asBoolean(settings.auto_check_updates),
    modelDownloaded: asBoolean(settings.model_downloaded),
    ttsProvider: asTtsProvider(settings.tts_provider),
    supertonicVoiceStyle: asSupertonicVoiceStyle(
      settings.supertonic_voice_style,
    ),
    supertonicLanguage: asSupertonicLanguage(settings.supertonic_language),
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

function asModelPrecision(value: unknown): ModelPrecision | undefined {
  return value === "fp32" || value === "q8" ? value : undefined;
}

/**
 * The single place stored provider values are interpreted, including retired
 * ones. `system` was the Web Speech path, removed once every engine sat behind
 * SpeechEngine; `gemini` and `fish` predate Supertonic. All fall back to the
 * default rather than being written back — the next settings save overwrites
 * the stale row anyway.
 */
function asTtsProvider(value: unknown): TtsProvider | undefined {
  if (value === "gemini" || value === "fish") {
    return "supertonic";
  }
  return value === "kokoro" || value === "supertonic" ? value : undefined;
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
