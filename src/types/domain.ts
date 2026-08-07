export type JsonValue = null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };

export type SourceType = "openstax" | "libretexts" | "epub" | "pdf" | "pasted" | "url";

export type Document = { id: string, title: string, sourceType: SourceType, sourceMetadata: JsonValue, coverImagePath: string | null, license: string | null, attribution: string | null, wordCount: number, importedAt: string, lastOpenedAt: string | null, };

export type Section = { id: string, documentId: string, ordinal: number, title: string, wordCount: number, };

export type Paragraph = { id: string, sectionId: string, ordinal: number, text: string, sentenceOffsets: Array<[number, number]>, };

export type SectionImage = { id: string, sectionId: string, ordinal: number, sourceUrl: string, localPath: string, altText: string | null, caption: string | null, contentType: string | null, anchorParagraphOrdinal: number | null, };

export type PlaybackState = { documentId: string, sectionId: string, paragraphId: string, sentenceIndex: number, sentenceOffsetMs: number, voiceId: string, speed: number, updatedAt: string, };

export type Voice = { id: string, displayName: string, language: string, gender: string, isBundled: boolean, isDownloaded: boolean, sizeBytes: number, previewPath: string | null, };

export type OpenStaxBook = { uuid: string, slug: string, title: string, subject: string, edition: string, coverUrl: string | null, license: string, language: string, };

export type LibreTextsBook = { bookId: string, title: string, author: string, affiliation: string, library: string, subject: string, license: string, summary: string, thumbnail: string | null, onlineUrl: string | null, lastUpdated: string | null, location: string, program: string, };

export type LibreTextsLibrary = { subdomain: string, title: string, };

export type ImportStage = "fetching" | "parsing" | "tokenizing" | "storing" | "complete" | "failed";

export type ImportProgress = { documentId: string, stage: ImportStage, current: number, total: number, message: string | null, };

/** Mirrors `AppError::kind` in `src-tauri/src/error.rs`. Kept in sync by `scripts/ci/check-error-kinds.sh`. */
export type AppErrorKind = "database" | "pool" | "io" | "serde" | "http" | "readability" | "epub" | "pdf" | "openstax" | "libretexts" | "model" | "voice" | "tts" | "drm_protected" | "tauri" | "invalid_input" | "migration";

export type AppError = { kind: AppErrorKind, message: string, retryable: boolean, };
