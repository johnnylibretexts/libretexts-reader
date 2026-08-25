import { fireEvent, render, screen } from "@testing-library/react";
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
    sourceLanguage: "en",
    importedAt: "2026-08-17T00:00:00Z",
    lastOpenedAt: null,
    progress: 0,
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

  it("falls back to the source icon when the stored cover will not render", () => {
    // A file that is missing, or that is not the image its name claims, would
    // otherwise leave a broken-image glyph on the card with no way back. The
    // fallback that already exists for "no cover" should cover "bad cover".
    const { container } = renderCard(
      libraryDocument({ coverImagePath: "/covers/abc123.png" }),
    );
    const cover = container.querySelector("img");

    fireEvent.error(cover!);

    expect(container.querySelector("img")).toBeNull();
    expect(screen.getByText("Pressbooks")).toBeInTheDocument();
  });

  it("falls back to the source icon when the document has no cover", () => {
    const { container } = renderCard(libraryDocument());

    expect(container.querySelector("img")).toBeNull();
    expect(screen.getByText("Pressbooks")).toBeInTheDocument();
  });

  it("fills the progress bar to where the reader stopped", () => {
    // The bar was `const progress = 0` -- hardcoded, because nothing read the
    // cursor back. Every card in the Library advertised an unstarted book.
    renderCard(libraryDocument({ progress: 0.42 }));

    const bar = screen.getByRole("progressbar");
    expect(bar).toHaveAttribute("aria-valuenow", "42");
  });

  it("leaves the bar empty for a book nobody has opened", () => {
    renderCard(libraryDocument({ progress: 0 }));

    expect(screen.getByRole("progressbar")).toHaveAttribute(
      "aria-valuenow",
      "0",
    );
  });

  it("names the book the progress belongs to", () => {
    // One bar per card, all of them identical to a screen reader without this.
    renderCard(libraryDocument({ progress: 0.42 }));

    expect(
      screen.getByRole("progressbar", { name: /A Concise Introduction to Logic/ }),
    ).toBeInTheDocument();
  });
});
