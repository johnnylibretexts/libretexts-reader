import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type * as Domain from "../../types/domain";

const listLibreTextsLibraries = vi.fn();
const listLibreTextsCatalog = vi.fn();

vi.mock("../../lib/tauri", () => ({
  isTauriRuntime: () => true,
  api: {
    get listLibreTextsLibraries() {
      return listLibreTextsLibraries;
    },
    get listLibreTextsCatalog() {
      return listLibreTextsCatalog;
    },
  },
}));

const { LibreTextsBrowser } = await import("./LibreTextsBrowser");
const { useImportsStore } = await import("../../stores/imports");
const { useLibraryStore } = await import("../../stores/library");

const BOOK: Domain.LibreTextsBook = {
  bookId: "bio-1764",
  title: "General Biology",
  author: "OpenStax",
  affiliation: "LibreTexts",
  library: "bio",
  subject: "Biology",
  license: "CC BY",
  summary: "",
  thumbnail: null,
  onlineUrl: null,
  lastUpdated: null,
  location: "",
  program: "",
};

/** A library document that was imported from BOOK. */
function importedDocument(overrides: Partial<Domain.Document> = {}): Domain.Document {
  return {
    id: "doc-1",
    // Deliberately not the catalog title: the Open action must carry the
    // document's identity, not the card's.
    title: "General Biology (imported)",
    sourceType: "libretexts",
    sourceMetadata: { book_id: "bio-1764" },
    coverImagePath: null,
    license: null,
    attribution: null,
    wordCount: 0,
    importedAt: "2026-08-17T00:00:00Z",
    lastOpenedAt: null,
    progress: 0,
    ...overrides,
  };
}

async function renderBrowser(onOpenDocument = vi.fn()) {
  render(<LibreTextsBrowser onOpenDocument={onOpenDocument} />);
  // The catalog fetch is debounced by 250ms, so nothing renders immediately.
  await waitFor(() => expect(screen.getByText("General Biology")).toBeInTheDocument());
  return onOpenDocument;
}

describe("LibreTextsBrowser in-library cards", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    listLibreTextsLibraries.mockResolvedValue([]);
    listLibreTextsCatalog.mockResolvedValue([BOOK]);
    useImportsStore.setState({ active: null, completed: null, error: null });
    useLibraryStore.setState({ documents: [] });
  });

  it("says the library filter failed rather than showing no libraries", async () => {
    // An empty dropdown is a claim about LibreTexts -- "this Source has no
    // libraries" -- and the reader has no way to tell it from a request that
    // failed. Browsing still works meanwhile, so this must not read as fatal.
    listLibreTextsLibraries.mockRejectedValue(new Error("network unreachable"));

    render(<LibreTextsBrowser onOpenDocument={vi.fn()} />);

    expect(
      await screen.findByText(/could not load the library filter/i),
    ).toBeInTheDocument();
  });

  it("offers Add for a book that is not in the library", async () => {
    await renderBrowser();

    expect(screen.getByRole("button", { name: /add/i })).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /open general biology in the reader/i }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("In library")).not.toBeInTheDocument();
  });

  it("replaces Add with an Open action once the book is in the library", async () => {
    useLibraryStore.setState({ documents: [importedDocument()] });

    await renderBrowser();

    expect(screen.getByText("In library")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /open general biology in the reader/i }),
    ).toBeInTheDocument();
    // "in place of + Add" -- both must not be offered at once.
    expect(screen.queryByRole("button", { name: /add/i })).not.toBeInTheDocument();
  });

  it("opens the imported document, not the catalog entry", async () => {
    // The whole point of findImportedBook returning a Document rather than a
    // boolean. A call site that used only its truthiness could still render
    // "In library" while having no id to open.
    useLibraryStore.setState({ documents: [importedDocument()] });
    const onOpenDocument = await renderBrowser();

    await userEvent.click(
      screen.getByRole("button", { name: /open general biology in the reader/i }),
    );

    expect(onOpenDocument).toHaveBeenCalledWith({
      id: "doc-1",
      title: "General Biology (imported)",
    });
  });

  it("keeps Open usable while an unrelated import is running", async () => {
    // Add is globally disabled during an import, deliberately. Open is not:
    // reading a book already on disk touches nothing the guard protects.
    useLibraryStore.setState({ documents: [importedDocument()] });
    useImportsStore.setState({
      active: {
        bookId: "chem-999",
        title: "Chemistry",
        stage: "fetching",
        current: 1,
        total: 10,
      },
    });
    const onOpenDocument = await renderBrowser();

    const open = screen.getByRole("button", { name: /open general biology in the reader/i });
    expect(open).toBeEnabled();

    await userEvent.click(open);
    expect(onOpenDocument).toHaveBeenCalledTimes(1);
  });
});
