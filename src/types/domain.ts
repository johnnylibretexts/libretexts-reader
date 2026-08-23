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

/** `progress` is how far the resume cursor sits into the book, 0 to 1. Derived on read from `playback_state`, never stored -- the cursor is the one thing that decides where playback resumes. Zero for a book never opened. */
export type Document = { id: string, title: string, sourceType: SourceType, sourceMetadata: JsonValue, coverImagePath: string | null, license: string | null, attribution: string | null, wordCount: number, importedAt: string, lastOpenedAt: string | null, progress: number, };

export type Section = { id: string, documentId: string, ordinal: number, title: string, wordCount: number, };

/** `text` is the display form; `sentenceSpeech` holds one speech form per entry in `sentenceOffsets`, in the same order. */
export type Paragraph = { id: string, sectionId: string, ordinal: number, text: string, sentenceOffsets: Array<[number, number]>, sentenceSpeech: string[], };

export type SectionImage = { id: string, sectionId: string, ordinal: number, sourceUrl: string, localPath: string, altText: string | null, caption: string | null, contentType: string | null, anchorParagraphOrdinal: number | null, };

export type PlaybackState = { documentId: string, sectionId: string, paragraphId: string, sentenceIndex: number, sentenceOffsetMs: number, voiceId: string, speed: number, updatedAt: string, };

export type OpenStaxBook = { uuid: string, slug: string, title: string, subject: string, edition: string, coverUrl: string | null, license: string, language: string, };

export type LibreTextsBook = { bookId: string, title: string, author: string, affiliation: string, library: string, subject: string, license: string, summary: string, thumbnail: string | null, onlineUrl: string | null, lastUpdated: string | null, location: string, program: string, };

/** One Pressbooks Catalog on offer. Pressbooks calls these "networks" and the picker uses that word, because it is the publisher's own; the type is not named after it. `bookCount` conveys scale in the picker -- the live count comes from the Catalog at browse time. */
export type PressbooksCatalog = { host: string, name: string, bookCount: number, isDefault: boolean, };

/**
 * A Catalog as the browser shows it. `totalBooks` is what the Catalog says it
 * holds, not what arrived — the two differ while a crawl is unfinished, and a
 * partial Catalog reporting only its books would read as a small complete one.
 */
export type PressbooksCatalogListing = { books: PressbooksBook[], totalBooks: number, isComplete: boolean, };

/** Payload of the `catalog-progress` event. Pages fetched against pages needed. */
export type PressbooksCatalogProgress = { host: string, current: number, total: number, };

/** `bookUrl` is the book's canonical URL and its identity everywhere: the catalog row key, the value `sourceMetadata` carries on an imported Document, and what the browser matches on to tell an imported book from a new one. */
export type PressbooksBook = { bookUrl: string, title: string, subtitle: string | null, coverUrl: string | null, thumbnailUrl: string | null, authors: string, licenseName: string, licenseUrl: string | null, wordCount: number, };

export type LibreTextsLibrary = { subdomain: string, title: string, };

export type ImportStage = "fetching" | "parsing" | "tokenizing" | "storing" | "complete" | "failed";

export type ImportProgress = { documentId: string, stage: ImportStage, current: number, total: number, message: string | null, };

/** Mirrors `AppError::kind` in `src-tauri/src/error.rs`. Kept in sync by `scripts/ci/check-error-kinds.sh`. */
export type AppErrorKind = "database" | "pool" | "io" | "serde" | "http" | "readability" | "epub" | "pdf" | "openstax" | "libretexts" | "pressbooks" | "model" | "voice" | "auth" | "tts" | "drm_protected" | "tauri" | "invalid_input" | "cancelled" | "migration" | "payment_required" | "rate_limited";

export type AppError = { kind: AppErrorKind, message: string, retryable: boolean, };
