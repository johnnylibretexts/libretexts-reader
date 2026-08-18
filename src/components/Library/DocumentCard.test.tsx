import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type * as Domain from "../../types/domain";

// The real `convertFileSrc` delegates to a global the Tauri runtime injects,
// which jsdom has none of. This stands in with the shape the runtime produces.
vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (path: string) =>
    `asset://localhost/${encodeURIComponent(path)}`,
}));

const { DocumentCard } = await import("./DocumentCard");

function libraryDocument(
  overrides: Partial<Domain.Document> = {},
): Domain.Document {
  return {
    id: "doc-1",
    title: "A Concise Introduction to Logic",
    sourceType: "pressbooks",
    sourceMetadata: { book_url: "https://books.test/logic/" },
    coverImagePath: null,
    license: null,
    attribution: null,
    wordCount: 90000,
    importedAt: "2026-08-17T00:00:00Z",
    lastOpenedAt: null,
    ...overrides,
  };
}

function renderCard(document: Domain.Document) {
  return render(
    <DocumentCard
      document={document}
      onContextMenu={vi.fn()}
      onDelete={vi.fn()}
      onOpen={vi.fn()}
    />,
  );
}

describe("DocumentCard", () => {
  it("shows the cover a source stored with the document", () => {
    // Queried as an element rather than by role: the cover is decorative, so
    // it carries an empty alt and has no accessible name to find it by. The
    // title beside it is what names the card.
    const { container } = renderCard(
      libraryDocument({ coverImagePath: "/covers/abc123.png" }),
    );

    const cover = container.querySelector("img");
    expect(cover).not.toBeNull();
    // Converted, not passed through. The webview blocks a bare file path, so a
    // cover handed over unconverted renders as a broken image -- which is why
    // what this asserts is that the src is *not* the stored path.
    expect(cover?.getAttribute("src")).not.toBe("/covers/abc123.png");
    expect(cover?.getAttribute("src")).toContain("asset://localhost/");
    expect(cover?.getAttribute("src")).toContain("abc123.png");
  });

  it("falls back to the source icon when the document has no cover", () => {
    const { container } = renderCard(libraryDocument());

    expect(container.querySelector("img")).toBeNull();
    expect(screen.getByText("Pressbooks")).toBeInTheDocument();
  });
});
