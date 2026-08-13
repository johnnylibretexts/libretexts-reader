import type * as Domain from "../types/domain";

/**
 * Find the library document a catalog entry was imported as, if any.
 *
 * The two catalogs identify books differently — LibreTexts writes `book_id`
 * into source_metadata, OpenStax writes `book_uuid` — so the key is a
 * parameter rather than baked in. sourceMetadata is JsonValue and may be null
 * or a non-object for other import kinds, hence the shape check.
 */
export function findImportedBook(
  documents: Domain.Document[],
  source: Domain.SourceType,
  metadataKey: string,
  catalogId: string,
): Domain.Document | null {
  const match = documents.find((document) => {
    if (document.sourceType !== source) {
      return false;
    }
    const metadata = document.sourceMetadata;
    if (typeof metadata !== "object" || metadata === null || Array.isArray(metadata)) {
      return false;
    }
    return (metadata as Record<string, unknown>)[metadataKey] === catalogId;
  });

  return match ?? null;
}
