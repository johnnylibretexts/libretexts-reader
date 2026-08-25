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
  /**
   * A sentence in this language, for the Settings Test button.
   *
   * Supertonic does not translate and has no language embedding
   * (`n_langs: 0` in the model's own `tts.json`) -- `preprocess_text` wraps
   * the text as `<es>...</es>` and the tag only selects letter-to-sound
   * rules. So a single English sample under a Spanish tag demonstrates
   * English read with a Spanish accent, which is exactly what made this
   * setting look broken. The audition has to be written in the language
   * being auditioned or it shows the reader the wrong thing.
   */
  sample: string;
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
  {
    id: "en",
    name: "English",
    sample: "This is a voice test for LibreTexts Reader.",
  },
  {
    id: "ko",
    name: "Korean",
    sample: "리브레텍스트 리더의 음성 테스트입니다.",
  },
  {
    id: "ja",
    name: "Japanese",
    sample: "これはリブレテキスト・リーダーの音声テストです。",
  },
  {
    id: "ar",
    name: "Arabic",
    sample: "هذا اختبار صوتي لتطبيق ليبريتكستس ريدر.",
  },
  {
    id: "bg",
    name: "Bulgarian",
    sample: "Това е тест на гласа за LibreTexts Reader.",
  },
  {
    id: "cs",
    name: "Czech",
    sample: "Toto je test hlasu pro LibreTexts Reader.",
  },
  {
    id: "da",
    name: "Danish",
    sample: "Dette er en stemmetest til LibreTexts Reader.",
  },
  {
    id: "de",
    name: "German",
    sample: "Dies ist ein Sprachtest für LibreTexts Reader.",
  },
  {
    id: "el",
    name: "Greek",
    sample: "Αυτή είναι μια δοκιμή φωνής για το LibreTexts Reader.",
  },
  {
    id: "es",
    name: "Spanish",
    sample: "Esta es una prueba de voz de LibreTexts Reader.",
  },
  {
    id: "et",
    name: "Estonian",
    sample: "See on LibreTexts Readeri häälekatse.",
  },
  {
    id: "fi",
    name: "Finnish",
    sample: "Tämä on LibreTexts Readerin äänitesti.",
  },
  {
    id: "fr",
    name: "French",
    sample: "Ceci est un test vocal pour LibreTexts Reader.",
  },
  {
    id: "hi",
    name: "Hindi",
    sample: "यह लिब्रेटेक्स्ट्स रीडर के लिए एक आवाज़ परीक्षण है।",
  },
  {
    id: "hr",
    name: "Croatian",
    sample: "Ovo je glasovni test za LibreTexts Reader.",
  },
  {
    id: "hu",
    name: "Hungarian",
    sample: "Ez egy hangteszt a LibreTexts Readerhez.",
  },
  {
    id: "id",
    name: "Indonesian",
    sample: "Ini adalah tes suara untuk LibreTexts Reader.",
  },
  {
    id: "it",
    name: "Italian",
    sample: "Questa è una prova vocale per LibreTexts Reader.",
  },
  {
    id: "lt",
    name: "Lithuanian",
    sample: "Tai yra LibreTexts Reader balso testas.",
  },
  {
    id: "lv",
    name: "Latvian",
    sample: "Šis ir LibreTexts Reader balss tests.",
  },
  {
    id: "nl",
    name: "Dutch",
    sample: "Dit is een stemtest voor LibreTexts Reader.",
  },
  {
    id: "pl",
    name: "Polish",
    sample: "To jest test głosu dla LibreTexts Reader.",
  },
  {
    id: "pt",
    name: "Portuguese",
    sample: "Este é um teste de voz do LibreTexts Reader.",
  },
  {
    id: "ro",
    name: "Romanian",
    sample: "Acesta este un test vocal pentru LibreTexts Reader.",
  },
  {
    id: "ru",
    name: "Russian",
    sample: "Это проверка голоса для LibreTexts Reader.",
  },
  {
    id: "sk",
    name: "Slovak",
    sample: "Toto je hlasový test pre LibreTexts Reader.",
  },
  {
    id: "sl",
    name: "Slovenian",
    sample: "To je glasovni preizkus za LibreTexts Reader.",
  },
  {
    id: "sv",
    name: "Swedish",
    sample: "Det här är ett rösttest för LibreTexts Reader.",
  },
  {
    id: "tr",
    name: "Turkish",
    sample: "Bu, LibreTexts Reader için bir ses testidir.",
  },
  {
    id: "uk",
    name: "Ukrainian",
    sample: "Це перевірка голосу для LibreTexts Reader.",
  },
  {
    id: "vi",
    name: "Vietnamese",
    sample: "Đây là bài kiểm tra giọng nói cho LibreTexts Reader.",
  },
  {
    id: "na",
    name: "Language neutral",
    // No language to write this one in -- `na` is the tag for text whose
    // language Supertonic should not assume. English, and worded so the
    // reader hears which setting they are on.
    sample: "This sample uses the language-neutral pronunciation setting.",
  },
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

/**
 * The sentence the Settings Test button speaks for `language`.
 *
 * Falls back to English rather than failing, the same posture as
 * `normalize_language` in the Rust engine: a `supertonic_language` row
 * written by an older release must leave Test with something to say.
 */
export function supertonicSampleText(language: SupertonicLanguage): string {
  const option =
    SUPERTONIC_LANGUAGES.find((candidate) => candidate.id === language) ??
    SUPERTONIC_LANGUAGES[0];
  return option.sample;
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
