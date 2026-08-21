import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type * as Domain from "../../types/domain";

// Same stand-in DocumentCard.test.tsx uses: the real `convertFileSrc`
// delegates to a global the Tauri runtime injects, which jsdom has none of.
vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (path: string) =>
    `asset://localhost/${encodeURIComponent(path)}`,
}));

const { LibraryGrid } = await import("./Grid");
const { useLibraryStore } = await import("../../stores/library");

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

describe("LibraryGrid", () => {
  beforeEach(() => {
    useLibraryStore.setState({
      documents: [libraryDocument()],
      loading: false,
      error: null,
    });
  });

  describe("context menu", () => {
    /** Right-clicks the card, which is what opens the menu. */
    function openMenu() {
      fireEvent.contextMenu(
        screen.getByText("A Concise Introduction to Logic"),
      );
    }

    it("offers no action the app cannot perform", () => {
      // Export shipped here permanently disabled. A greyed-out item is a
      // promise the app does not keep, and it read as an unfinished feature
      // rather than a deliberate omission -- chapter audio export lives in
      // the Reader.
      render(<LibraryGrid onOpenDocument={vi.fn()} />);
      openMenu();

      expect(screen.getByRole("menu")).toBeInTheDocument();
      expect(
        screen.queryByRole("menuitem", { name: /export/i }),
      ).not.toBeInTheDocument();

      // Every item that *is* offered must be usable.
      for (const item of screen.getAllByRole("menuitem")) {
        expect(item).toBeEnabled();
      }
    });

    it("still offers Open and Delete", () => {
      render(<LibraryGrid onOpenDocument={vi.fn()} />);
      openMenu();

      expect(
        screen.getByRole("menuitem", { name: /open/i }),
      ).toBeInTheDocument();
      expect(
        screen.getByRole("menuitem", { name: /delete/i }),
      ).toBeInTheDocument();
    });

    it("opens the document from the menu", () => {
      const onOpenDocument = vi.fn();
      render(<LibraryGrid onOpenDocument={onOpenDocument} />);
      openMenu();

      fireEvent.click(screen.getByRole("menuitem", { name: /open/i }));

      expect(onOpenDocument).toHaveBeenCalledWith({
        id: "doc-1",
        title: "A Concise Introduction to Logic",
      });
    });
  });

  describe("empty state", () => {
    it("names every Source a reader can import from", () => {
      // The copy went stale once already: it listed OpenStax, LibreTexts,
      // EPUB, PDF and pasted text while Pressbooks and article URLs had
      // both shipped. A reader who reads this and concludes their Source
      // is unsupported does not go looking in the sidebar.
      useLibraryStore.setState({ documents: [], loading: false, error: null });
      render(<LibraryGrid onOpenDocument={vi.fn()} />);

      const copy = screen.getByText(/get started/i).textContent ?? "";
      for (const source of [
        "OpenStax",
        "LibreTexts",
        "Pressbooks",
        "EPUB",
        "PDF",
        "article",
        "pasted text",
      ]) {
        expect(copy).toContain(source);
      }
    });
  });
});
