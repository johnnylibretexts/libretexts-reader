import { create } from "zustand";
import {
  asSupertonicLanguage,
  asSupertonicVoiceStyle,
  type SupertonicLanguage,
  type SupertonicVoiceStyle,
} from "../lib/supertonic";
import { api } from "../lib/tauri";

export type AppTheme = "light" | "dark" | "system";
export type ModelPrecision = "fp32" | "q8";
export type TtsProvider = "kokoro" | "supertonic" | "system";
export type SelectableTtsProvider = "kokoro" | "supertonic";

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
  setTtsProvider: (provider: SelectableTtsProvider) => Promise<void>;
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
      const loaded = await loadSettings();
      const ttsProvider =
        loaded.ttsProvider === undefined || loaded.ttsProvider === "system"
          ? DEFAULT_SETTINGS.ttsProvider
          : loaded.ttsProvider;
      set({
        ...DEFAULT_SETTINGS,
        ...definedSettings(loaded),
        ttsProvider,
        hydrated: true,
        loading: false,
      });
      if (loaded.ttsProvider === "system") {
        void api.setSetting("tts_provider", ttsProvider).catch(() => undefined);
      }
    } catch (error) {
      set({
        ...DEFAULT_SETTINGS,
        theme: localTheme() ?? DEFAULT_SETTINGS.theme,
        hydrated: true,
        loading: false,
        error: error instanceof Error ? error.message : String(error),
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
        error: error instanceof Error ? error.message : String(error),
      });
    }
  },
  setTtsProvider: async (provider: SelectableTtsProvider) => {
    set({ ttsProvider: provider, error: null });

    try {
      await api.setSetting("tts_provider", provider);
    } catch (error) {
      set({
        error: error instanceof Error ? error.message : String(error),
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
    const requestedProvider =
      ttsSettings.ttsProvider ??
      get().ttsProvider ??
      DEFAULT_SETTINGS.ttsProvider;
    const ttsProvider =
      requestedProvider === "system"
        ? DEFAULT_SETTINGS.ttsProvider
        : requestedProvider;
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
        error: error instanceof Error ? error.message : String(error),
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

function asTtsProvider(value: unknown): TtsProvider | undefined {
  if (value === "gemini" || value === "fish") {
    return "supertonic";
  }
  return value === "kokoro" || value === "supertonic" || value === "system"
    ? value
    : undefined;
}

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
