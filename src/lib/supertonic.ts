export type SupertonicVoiceStyle =
  | "M1"
  | "M2"
  | "M3"
  | "M4"
  | "M5"
  | "F1"
  | "F2"
  | "F3"
  | "F4"
  | "F5";

export type SupertonicLanguage =
  | "en"
  | "ko"
  | "ja"
  | "ar"
  | "bg"
  | "cs"
  | "da"
  | "de"
  | "el"
  | "es"
  | "et"
  | "fi"
  | "fr"
  | "hi"
  | "hr"
  | "hu"
  | "id"
  | "it"
  | "lt"
  | "lv"
  | "nl"
  | "pl"
  | "pt"
  | "ro"
  | "ru"
  | "sk"
  | "sl"
  | "sv"
  | "tr"
  | "uk"
  | "vi"
  | "na";

export interface SupertonicVoiceOption {
  id: SupertonicVoiceStyle;
  name: string;
}

export interface SupertonicLanguageOption {
  id: SupertonicLanguage;
  name: string;
}

export const SUPERTONIC_VOICES: SupertonicVoiceOption[] = [
  { id: "M1", name: "Male 1" },
  { id: "M2", name: "Male 2" },
  { id: "M3", name: "Male 3" },
  { id: "M4", name: "Male 4" },
  { id: "M5", name: "Male 5" },
  { id: "F1", name: "Female 1" },
  { id: "F2", name: "Female 2" },
  { id: "F3", name: "Female 3" },
  { id: "F4", name: "Female 4" },
  { id: "F5", name: "Female 5" },
];

export const SUPERTONIC_LANGUAGES: SupertonicLanguageOption[] = [
  { id: "en", name: "English" },
  { id: "ko", name: "Korean" },
  { id: "ja", name: "Japanese" },
  { id: "ar", name: "Arabic" },
  { id: "bg", name: "Bulgarian" },
  { id: "cs", name: "Czech" },
  { id: "da", name: "Danish" },
  { id: "de", name: "German" },
  { id: "el", name: "Greek" },
  { id: "es", name: "Spanish" },
  { id: "et", name: "Estonian" },
  { id: "fi", name: "Finnish" },
  { id: "fr", name: "French" },
  { id: "hi", name: "Hindi" },
  { id: "hr", name: "Croatian" },
  { id: "hu", name: "Hungarian" },
  { id: "id", name: "Indonesian" },
  { id: "it", name: "Italian" },
  { id: "lt", name: "Lithuanian" },
  { id: "lv", name: "Latvian" },
  { id: "nl", name: "Dutch" },
  { id: "pl", name: "Polish" },
  { id: "pt", name: "Portuguese" },
  { id: "ro", name: "Romanian" },
  { id: "ru", name: "Russian" },
  { id: "sk", name: "Slovak" },
  { id: "sl", name: "Slovenian" },
  { id: "sv", name: "Swedish" },
  { id: "tr", name: "Turkish" },
  { id: "uk", name: "Ukrainian" },
  { id: "vi", name: "Vietnamese" },
  { id: "na", name: "Language neutral" },
];

export function asSupertonicVoiceStyle(
  value: unknown,
): SupertonicVoiceStyle | undefined {
  return SUPERTONIC_VOICES.some((voice) => voice.id === value)
    ? (value as SupertonicVoiceStyle)
    : undefined;
}

export function asSupertonicLanguage(
  value: unknown,
): SupertonicLanguage | undefined {
  return SUPERTONIC_LANGUAGES.some((language) => language.id === value)
    ? (value as SupertonicLanguage)
    : undefined;
}

export function supertonicPreviewText(
  sectionTitle: string,
  sampleText?: string,
) {
  const cleaned = sampleText?.trim().replace(/\s+/g, " ");
  if (cleaned) {
    return cleaned.length > 220 ? `${cleaned.slice(0, 220)}.` : cleaned;
  }

  return `This is a preview for ${sectionTitle || "the current chapter"}. The narration should feel natural and steady for long-form listening.`;
}
