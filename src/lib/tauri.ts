import { invoke } from "@tauri-apps/api/core";
import type * as Domain from "../types/domain";
import type { SpeechVoice } from "./speech/types";

type InvokeArgs = Record<string, unknown>;

export interface SynthesizeSpeechRequest {
  text: string;
  speed: number;
  /**
   * Which engine speaks this text. Rust deserializes this with no
   * `#[serde(default)]` — a request that omits it is rejected, not defaulted
   * to whichever engine used to be the only one. See `SynthesizeSpeechRequest`
   * in `src-tauri/src/commands/tts.rs`.
   */
  provider: Domain.TtsProvider;
  voiceId?: string | null;
  language?: string | null;
}

export interface SpeechAudio {
  audio: number[];
  mimeType: string;
}

export interface SupertonicPreviewRequest {
  text: string;
  voiceStyle?: string | null;
  language?: string | null;
}

export interface SupertonicChapterRequest {
  documentId: string;
  sectionId: string;
  /** Which engine renders this chapter. No default; see `ChapterRequest` in `src-tauri/src/tts/supertonic/mod.rs`. */
  provider: Domain.TtsProvider;
  voiceStyle?: string | null;
  language?: string | null;
  outputPath?: string | null;
  force?: boolean | null;
}

export interface SupertonicChapterEstimate {
  wordCount: number;
  estimatedSeconds: number;
  chunkCount: number;
  cached: boolean;
  outputPath: string;
  /**
   * How many characters this export will actually be billed for. Always 0
   * for a cached chapter and for any non-billed provider. Mirrors
   * `billable_characters` on `ChapterEstimate` in
   * `src-tauri/src/tts/supertonic/mod.rs` -- the gate in
   * `SupertonicChapterExport.tsx` reads this, never a locally computed price.
   */
  billableCharacters: number;
  /** Which engine this estimate was computed for. */
  provider: Domain.TtsProvider;
}

export interface SupertonicChapterExport {
  outputPath: string;
  cached: boolean;
  byteLength: number;
  estimate: SupertonicChapterEstimate;
}

export interface SupertonicModelStatus {
  downloaded: boolean;
  directory: string;
  downloadedBytes: number;
  totalBytes: number;
  missingFiles: string[];
}

export interface SupertonicModelProgress {
  file: string;
  downloaded: number;
  total: number;
}

/**
 * Presence/validity/credit only — there is deliberately no command that
 * returns the stored key itself. Mirrors `FishKeyStatus` in
 * `src-tauri/src/commands/fish.rs`.
 */
export interface FishKeyStatus {
  present: boolean;
  valid: boolean | null;
  credit: number | null;
}

const DESKTOP_RUNTIME_ERROR = "This action requires the Tauri desktop runtime.";

export function isTauriRuntime() {
  return (
    typeof window !== "undefined" && Reflect.has(window, "__TAURI_INTERNALS__")
  );
}

function invokeDesktop<T>(command: string, args?: InvokeArgs) {
  if (!isTauriRuntime()) {
    return Promise.reject(new Error(DESKTOP_RUNTIME_ERROR));
  }

  return invoke<T>(command, args);
}

function invokeWithBrowserFallback<T>(
  fallback: T,
  command: string,
  args?: InvokeArgs,
) {
  return isTauriRuntime()
    ? invoke<T>(command, args)
    : Promise.resolve(fallback);
}

export const api = {
  listDocuments: () =>
    invokeWithBrowserFallback<Domain.Document[]>([], "list_documents"),
  getDocument: (id: string) =>
    invokeDesktop<Domain.Document>("get_document", { id }),
  listSections: (documentId: string) =>
    invokeDesktop<Domain.Section[]>("list_sections", { documentId }),
  listParagraphs: (sectionId: string) =>
    invokeDesktop<Domain.Paragraph[]>("list_paragraphs", { sectionId }),
  listSectionImages: (sectionId: string) =>
    invokeDesktop<Domain.SectionImage[]>("list_section_images", { sectionId }),
  deleteDocument: (id: string) =>
    invokeDesktop<void>("delete_document", { id }),
  searchDocuments: (query: string) =>
    invokeWithBrowserFallback<Domain.Document[]>([], "search_documents", {
      query,
    }),

  importOpenstax: (bookUuid: string) =>
    invokeDesktop<string>("import_openstax", { bookUuid }),
  importLibreTexts: (bookId: string) =>
    invokeDesktop<string>("import_libretexts", { bookId }),
  importPressbooks: (bookUrl: string) =>
    invokeDesktop<string>("import_pressbooks", { bookUrl }),
  importEpub: (filePath: string) =>
    invokeDesktop<string>("import_epub", { filePath }),
  importPdf: (filePath: string) =>
    invokeDesktop<string>("import_pdf", { filePath }),
  importPastedText: (title: string, text: string) =>
    invokeDesktop<string>("import_pasted_text", { title, text }),
  importUrl: (url: string) => invokeDesktop<string>("import_url", { url }),
  listOpenstaxCatalog: () =>
    invokeWithBrowserFallback<Domain.OpenStaxBook[]>(
      [],
      "list_openstax_catalog",
    ),
  listLibreTextsCatalog: (query?: string, library?: string) =>
    invokeWithBrowserFallback<Domain.LibreTextsBook[]>(
      [],
      "list_libretexts_catalog",
      {
        query: query?.trim() ? query.trim() : null,
        library: library && library !== "all" ? library : null,
      },
    ),
  listLibreTextsLibraries: () =>
    invokeWithBrowserFallback<Domain.LibreTextsLibrary[]>(
      [],
      "list_libretexts_libraries",
    ),
  listPressbooksCatalog: () =>
    invokeWithBrowserFallback<Domain.PressbooksBook[]>(
      [],
      "list_pressbooks_catalog",
    ),

  savePlaybackState: (playback: Domain.PlaybackState) =>
    invokeDesktop<void>("save_playback_state", { playback }),

  synthesizeSpeech: (request: SynthesizeSpeechRequest) =>
    invokeDesktop<SpeechAudio>("synthesize_speech", { request }),
  getSupertonicModelStatus: () =>
    invokeDesktop<SupertonicModelStatus>("get_supertonic_model_status"),
  ensureSupertonicModelDownloaded: () =>
    invokeDesktop<string>("ensure_supertonic_model_downloaded"),
  previewSupertonicTts: (request: SupertonicPreviewRequest) =>
    invokeDesktop<SpeechAudio>("preview_supertonic_tts", { request }),
  estimateSupertonicChapter: (request: SupertonicChapterRequest) =>
    invokeDesktop<SupertonicChapterEstimate>("estimate_supertonic_chapter", {
      request,
    }),
  exportSupertonicChapterMp3: (request: SupertonicChapterRequest) =>
    invokeDesktop<SupertonicChapterExport>("export_supertonic_chapter_mp3", {
      request,
    }),

  getFishKeyStatus: () => invokeDesktop<FishKeyStatus>("get_fish_key_status"),
  /**
   * The live wallet balance, fetched over the network. Unlike
   * `getFishKeyStatus` -- deliberately network-free so Settings can render
   * on mount -- this one calls Fish's wallet endpoint, for the chapter
   * export confirmation gate, which needs the real balance at the moment it
   * asks the reader to approve spending.
   */
  getFishCredit: () => invokeDesktop<number>("get_fish_credit"),
  setFishApiKey: (key: string) =>
    invokeDesktop<FishKeyStatus>("set_fish_api_key", { key }),
  clearFishApiKey: () => invokeDesktop<void>("clear_fish_api_key"),
  listFishVoices: () => invokeDesktop<SpeechVoice[]>("list_fish_voices"),

  getSetting: <T = unknown>(key: string) =>
    invokeWithBrowserFallback<T | null>(null, "get_setting", { key }),
  setSetting: (key: string, value: unknown) =>
    invokeWithBrowserFallback<void>(undefined, "set_setting", { key, value }),
  getAllSettings: () =>
    invokeWithBrowserFallback<Record<string, unknown>>({}, "get_all_settings"),
};
