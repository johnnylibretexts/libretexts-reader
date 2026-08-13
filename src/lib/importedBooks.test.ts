import { describe, expect, it } from "vitest";
import type * as Domain from "../types/domain";
import { findImportedBook } from "./importedBooks";

function doc(
  id: string,
  sourceType: Domain.SourceType,
  sourceMetadata: Domain.Document["sourceMetadata"],
): Domain.Document {
  return {
    id,
    title: "General Biology",
    sourceType,
    sourceMetadata,
    coverImagePath: null,
    license: null,
    attribution: null,
    wordCount: 1,
    importedAt: "2026-08-13T00:00:00Z",
    lastOpenedAt: null,
  };
}

describe("findImportedBook", () => {
  it("finds a LibreTexts document by book_id", () => {
    const documents = [doc("d1", "libretexts", { book_id: "bio-1764" })];
    expect(findImportedBook(documents, "libretexts", "book_id", "bio-1764")?.id).toBe("d1");
  });

  it("finds an OpenStax document by book_uuid", () => {
    const documents = [doc("d1", "openstax", { book_uuid: "uuid-42" })];
    expect(findImportedBook(documents, "openstax", "book_uuid", "uuid-42")?.id).toBe("d1");
  });

  it("returns null for a book that is not imported", () => {
    const documents = [doc("d1", "libretexts", { book_id: "bio-1764" })];
    expect(findImportedBook(documents, "libretexts", "book_id", "chem-999")).toBeNull();
  });

  it("does not match across source types even when the id string collides", () => {
    const documents = [doc("d1", "openstax", { book_id: "bio-1764" })];
    expect(findImportedBook(documents, "libretexts", "book_id", "bio-1764")).toBeNull();
  });

  it("tolerates null and non-object sourceMetadata", () => {
    const documents = [
      doc("d1", "libretexts", null),
      doc("d2", "libretexts", "not-an-object" as unknown as Domain.Document["sourceMetadata"]),
      doc("d3", "libretexts", { book_id: "bio-1764" }),
    ];
    expect(findImportedBook(documents, "libretexts", "book_id", "bio-1764")?.id).toBe("d3");
  });

  it("returns the first match when a book was imported twice", () => {
    const documents = [
      doc("d1", "libretexts", { book_id: "bio-1764" }),
      doc("d2", "libretexts", { book_id: "bio-1764" }),
    ];
    expect(findImportedBook(documents, "libretexts", "book_id", "bio-1764")?.id).toBe("d1");
  });
});
