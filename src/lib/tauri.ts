import { invoke } from "@tauri-apps/api/core";
import type * as Domain from "../types/domain";

export type ModelPrecision = "fp32" | "q8";
type InvokeArgs = Record<string, unknown>;

export interface SynthesizeSpeechRequest {
  text: string;
  speed: number;
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

  savePlaybackState: (playback: Domain.PlaybackState) =>
    invokeDesktop<void>("save_playback_state", { playback }),

  listVoices: () =>
    invokeWithBrowserFallback<Domain.Voice[]>([], "list_voices"),
  downloadVoice: (voiceId: string) =>
    invokeDesktop<void>("download_voice", { voiceId }),
  deleteVoice: (voiceId: string) =>
    invokeDesktop<void>("delete_voice", { voiceId }),
  ensureModelDownloaded: (precision: ModelPrecision) =>
    invokeDesktop<string>("ensure_model_downloaded", { precision }),
  getModelPath: (precision: ModelPrecision) =>
    invokeDesktop<string>("get_model_path", { precision }),

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

  getSetting: <T = unknown>(key: string) =>
    invokeWithBrowserFallback<T | null>(null, "get_setting", { key }),
  setSetting: (key: string, value: unknown) =>
    invokeWithBrowserFallback<void>(undefined, "set_setting", { key, value }),
  getAllSettings: () =>
    invokeWithBrowserFallback<Record<string, unknown>>({}, "get_all_settings"),
};
