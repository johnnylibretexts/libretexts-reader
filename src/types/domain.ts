export type JsonValue = null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };

/**
 * Which TTS engine speaks. Declared here — the file `CLAUDE.md` designates for
 * mirroring payload shapes shared with Rust — rather than in
 * `stores/settings.ts` or `lib/speech/types.ts`, so both can import it without
 * either importing the other: `tauri.ts` needs it for the `provider` field
 * Rust now requires, and the settings store needs it for `TtsProvider`. This
 * module has no imports of its own, so it cannot introduce a cycle.
 */
export type TtsProvider = "supertonic" | "fish";

export type SourceType = "openstax" | "libretexts" | "pressbooks" | "epub" | "pdf" | "pasted" | "url";

export type Document = { id: string, title: string, sourceType: SourceType, sourceMetadata: JsonValue, coverImagePath: string | null, license: string | null, attribution: string | null, wordCount: number, importedAt: string, lastOpenedAt: string | null, };

export type Section = { id: string, documentId: string, ordinal: number, title: string, wordCount: number, };

/** `text` is the display form; `sentenceSpeech` holds one speech form per entry in `sentenceOffsets`, in the same order. */
export type Paragraph = { id: string, sectionId: string, ordinal: number, text: string, sentenceOffsets: Array<[number, number]>, sentenceSpeech: string[], };

export type SectionImage = { id: string, sectionId: string, ordinal: number, sourceUrl: string, localPath: string, altText: string | null, caption: string | null, contentType: string | null, anchorParagraphOrdinal: number | null, };

export type PlaybackState = { documentId: string, sectionId: string, paragraphId: string, sentenceIndex: number, sentenceOffsetMs: number, voiceId: string, speed: number, updatedAt: string, };

export type OpenStaxBook = { uuid: string, slug: string, title: string, subject: string, edition: string, coverUrl: string | null, license: string, language: string, };

export type LibreTextsBook = { bookId: string, title: string, author: string, affiliation: string, library: string, subject: string, license: string, summary: string, thumbnail: string | null, onlineUrl: string | null, lastUpdated: string | null, location: string, program: string, };

/** `bookUrl` is the book's canonical URL and its identity everywhere: the catalog row key, the value `sourceMetadata` carries on an imported Document, and what the browser matches on to tell an imported book from a new one. */
export type PressbooksBook = { bookUrl: string, title: string, subtitle: string | null, coverUrl: string | null, thumbnailUrl: string | null, authors: string, licenseName: string, licenseUrl: string | null, wordCount: number, };

export type LibreTextsLibrary = { subdomain: string, title: string, };

export type ImportStage = "fetching" | "parsing" | "tokenizing" | "storing" | "complete" | "failed";

export type ImportProgress = { documentId: string, stage: ImportStage, current: number, total: number, message: string | null, };

/** Mirrors `AppError::kind` in `src-tauri/src/error.rs`. Kept in sync by `scripts/ci/check-error-kinds.sh`. */
export type AppErrorKind = "database" | "pool" | "io" | "serde" | "http" | "readability" | "epub" | "pdf" | "openstax" | "libretexts" | "pressbooks" | "model" | "voice" | "auth" | "tts" | "drm_protected" | "tauri" | "invalid_input" | "migration" | "payment_required" | "rate_limited";

export type AppError = { kind: AppErrorKind, message: string, retryable: boolean, };
