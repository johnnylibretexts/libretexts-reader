import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type * as Domain from "../../types/domain";

// Same stand-in DocumentCard.test.tsx uses: the real `convertFileSrc`
// delegates to a global the Tauri runtime injects, which jsdom has none of.
vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (path: string) =>
    `asset://localhost/${encodeURIComponent(path)}`,
}));

const deleteDocument = vi.fn(async (_id: string) => undefined);
const listDocuments = vi.fn(async (): Promise<Domain.Document[]> => []);
const searchDocuments = vi.fn(async (_query: string) => []);

vi.mock("../../lib/tauri", () => ({
  api: {
    deleteDocument: (id: string) => deleteDocument(id),
    listDocuments: () => listDocuments(),
    searchDocuments: (q: string) => searchDocuments(q),
    // The empty state asks whether the reader's first Play will have to
    // download a voice. Answered "already there" so this file keeps testing
    // the grid; the warning itself is covered in EmptyState.test.tsx.
    getSupertonicModelStatus: async () => ({
      downloaded: true,
      directory: "/models",
      downloadedBytes: 401_276_744,
      totalBytes: 401_276_744,
      missingFiles: [],
    }),
  },
}));

const { LibraryGrid } = await import("./Grid");
const { useLibraryStore } = await import("../../stores/library");
const { usePlayerStore } = await import("../../stores/player");

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
    progress: 0,
    ...overrides,
  };
}

describe("LibraryGrid", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    deleteDocument.mockResolvedValue(undefined);
    // Seeded on both sides: the grid re-reads the library when it mounts, so
    // a store seeded alone is overwritten by whatever the backend answers.
    listDocuments.mockResolvedValue([libraryDocument()]);
    useLibraryStore.setState({
      documents: [libraryDocument()],
      loading: false,
      error: null,
    });
    usePlayerStore.setState({ document: null });
  });

  describe("deleting", () => {
    const TITLE = "A Concise Introduction to Logic";

    /** The trash button on the card, which deleted on the first click. */
    function clickTrash() {
      fireEvent.click(screen.getByRole("button", { name: `Delete ${TITLE}` }));
    }

    it("asks first, and names the book it is about to destroy", async () => {
      // One click destroyed a multi-hundred-MB import: rows, cover, every
      // downloaded figure, and since #28 the Source page cache too, so a
      // re-import genuinely re-downloads. The button sits next to "more
      // actions" on every card.
      render(<LibraryGrid onOpenDocument={vi.fn()} />);
      clickTrash();

      const dialog = await screen.findByRole("dialog");
      expect(dialog).toHaveTextContent(TITLE);
      expect(deleteDocument).not.toHaveBeenCalled();
    });

    it("leaves the book alone when the reader cancels", async () => {
      render(<LibraryGrid onOpenDocument={vi.fn()} />);
      clickTrash();
      fireEvent.click(
        await screen.findByRole("button", { name: /^Cancel$/ }),
      );

      expect(deleteDocument).not.toHaveBeenCalled();
      expect(screen.getByText(TITLE)).toBeInTheDocument();
    });

    it("deletes once the reader confirms", async () => {
      render(<LibraryGrid onOpenDocument={vi.fn()} />);
      clickTrash();
      fireEvent.click(await screen.findByRole("button", { name: /^Delete$/ }));

      await waitFor(() => expect(deleteDocument).toHaveBeenCalledWith("doc-1"));
    });

    it("resets the player when the book being read is the one deleted", async () => {
      // The MiniPlayer stayed bound to the deleted document, and the next
      // section change ran against rows that no longer exist.
      usePlayerStore.setState({ document: libraryDocument() });
      render(<LibraryGrid onOpenDocument={vi.fn()} />);
      clickTrash();
      fireEvent.click(await screen.findByRole("button", { name: /^Delete$/ }));

      await waitFor(() =>
        expect(usePlayerStore.getState().document).toBeNull(),
      );
    });

    it("keeps the reader's place when the delete fails", async () => {
      // `remove` reports failure by writing the store's `error` field, not by
      // throwing, so `await remove(...)` resolves either way. Resetting on
      // that bare await closes the book the reader is in the middle of for a
      // deletion that never happened -- and the book is still right there in
      // the library.
      deleteDocument.mockRejectedValue(new Error("disk is busy"));
      const reading = libraryDocument();
      usePlayerStore.setState({ document: reading });
      render(<LibraryGrid onOpenDocument={vi.fn()} />);
      clickTrash();
      fireEvent.click(await screen.findByRole("button", { name: /^Delete$/ }));

      await waitFor(() => expect(deleteDocument).toHaveBeenCalled());
      expect(usePlayerStore.getState().document).toEqual(reading);
      expect(screen.getByText(TITLE)).toBeInTheDocument();
    });

    it("can be reopened after Escape dismisses it", async () => {
      // Escape closes a native dialog through the DOM without telling React.
      // Left unsynced, `pendingDelete` stays set against a shut dialog -- and
      // `showModal()` on an already-open dialog throws, so it is the NEXT
      // delete that breaks, nowhere near the Escape that caused it.
      render(<LibraryGrid onOpenDocument={vi.fn()} />);
      clickTrash();
      const dialog = await screen.findByRole("dialog");

      fireEvent(dialog, new Event("cancel", { cancelable: true }));
      await waitFor(() => expect(dialog).not.toHaveAttribute("open"));

      clickTrash();
      await waitFor(() => expect(dialog).toHaveAttribute("open"));
      expect(deleteDocument).not.toHaveBeenCalled();
    });

    it("tells its host to let go of the reader route", async () => {
      // Resetting the player is not enough. AppShell holds the route that
      // opened the book, and the Reader nav entry is always there, so coming
      // back to it re-mounts the Reader with the dead id -- and Reader
      // re-fetches whenever its id and the player's document disagree, which
      // the reset has just guaranteed.
      const onOpenDocumentDeleted = vi.fn();
      usePlayerStore.setState({ document: libraryDocument() });
      render(
        <LibraryGrid
          onOpenDocument={vi.fn()}
          onOpenDocumentDeleted={onOpenDocumentDeleted}
        />,
      );
      clickTrash();
      fireEvent.click(await screen.findByRole("button", { name: /^Delete$/ }));

      await waitFor(() => expect(onOpenDocumentDeleted).toHaveBeenCalled());
    });

    it("does not disturb the reader route for a book it is not showing", async () => {
      const onOpenDocumentDeleted = vi.fn();
      usePlayerStore.setState({
        document: libraryDocument({ id: "doc-2", title: "Another Book" }),
      });
      render(
        <LibraryGrid
          onOpenDocument={vi.fn()}
          onOpenDocumentDeleted={onOpenDocumentDeleted}
        />,
      );
      clickTrash();
      fireEvent.click(await screen.findByRole("button", { name: /^Delete$/ }));

      await waitFor(() => expect(deleteDocument).toHaveBeenCalled());
      expect(onOpenDocumentDeleted).not.toHaveBeenCalled();
    });

    it("leaves the player alone when a different book is deleted", async () => {
      const reading = libraryDocument({ id: "doc-2", title: "Another Book" });
      usePlayerStore.setState({ document: reading });
      render(<LibraryGrid onOpenDocument={vi.fn()} />);
      clickTrash();
      fireEvent.click(await screen.findByRole("button", { name: /^Delete$/ }));

      await waitFor(() => expect(deleteDocument).toHaveBeenCalled());
      expect(usePlayerStore.getState().document).toEqual(reading);
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
    it("names every Source a reader can import from", async () => {
      // The copy went stale once already: it listed OpenStax, LibreTexts,
      // EPUB, PDF and pasted text while Pressbooks and article URLs had
      // both shipped. A reader who reads this and concludes their Source
      // is unsupported does not go looking in the sidebar.
      listDocuments.mockResolvedValue([]);
      useLibraryStore.setState({ documents: [], loading: false, error: null });
      render(<LibraryGrid onOpenDocument={vi.fn()} />);

      // Awaited, not synchronous: the grid re-reads the library on mount, so
      // the shelf is loading for a tick before it can be empty.
      const copy = (await screen.findByText(/get started/i)).textContent ?? "";
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

  describe("keeping progress current", () => {
    it("re-reads the library every time the shelf is shown", async () => {
      // `progress` is derived from the resume cursor at read time, so a list
      // fetched at launch shows where the reader was before they listened.
      // The grid unmounts while the Reader is open, which makes mounting
      // exactly "the reader came back to the shelf".
      render(<LibraryGrid onOpenDocument={vi.fn()} />);

      await waitFor(() => expect(listDocuments).toHaveBeenCalled());
    });
  });
});
