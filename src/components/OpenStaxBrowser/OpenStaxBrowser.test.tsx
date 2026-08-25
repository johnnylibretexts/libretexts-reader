import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type * as Domain from "../../types/domain";

const listOpenstaxCatalog = vi.fn();

vi.mock("../../lib/tauri", () => ({
  isTauriRuntime: () => true,
  api: {
    get listOpenstaxCatalog() {
      return listOpenstaxCatalog;
    },
  },
}));

const { OpenStaxBrowser } = await import("./OpenStaxBrowser");
const { useImportsStore } = await import("../../stores/imports");
const { useLibraryStore } = await import("../../stores/library");

const BOOK: Domain.OpenStaxBook = {
  uuid: "uuid-42",
  slug: "biology-2e",
  title: "Biology 2e",
  subject: "Science",
  edition: "2",
  coverUrl: null,
  license: "CC BY",
  language: "en",
};

/**
 * A library document imported from BOOK. OpenStax keys on `book_uuid` where
 * LibreTexts uses `book_id` -- the reason findImportedBook takes the key as a
 * parameter, and worth pinning per source rather than testing once.
 */
function importedDocument(overrides: Partial<Domain.Document> = {}): Domain.Document {
  return {
    id: "doc-9",
    // Deliberately not the catalog title: the Open action must carry the
    // document's identity, not the card's.
    title: "Biology 2e (imported)",
    sourceType: "openstax",
    sourceMetadata: { book_uuid: "uuid-42" },
    coverImagePath: null,
    license: null,
    attribution: null,
    wordCount: 0,
    sourceLanguage: "en",
    importedAt: "2026-08-17T00:00:00Z",
    lastOpenedAt: null,
    progress: 0,
    ...overrides,
  };
}

async function renderBrowser(onOpenDocument = vi.fn()) {
  render(<OpenStaxBrowser onOpenDocument={onOpenDocument} />);
  await waitFor(() => expect(screen.getByText("Biology 2e")).toBeInTheDocument());
  return onOpenDocument;
}

describe("OpenStaxBrowser in-library cards", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    listOpenstaxCatalog.mockResolvedValue([BOOK]);
    useImportsStore.setState({ active: null, completed: null, error: null });
    useLibraryStore.setState({ documents: [] });
  });

  it("offers Add for a book that is not in the library", async () => {
    await renderBrowser();

    expect(screen.getByRole("button", { name: /add/i })).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /open biology 2e in the reader/i }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("In library")).not.toBeInTheDocument();
  });

  it("replaces Add with an Open action once the book is in the library", async () => {
    useLibraryStore.setState({ documents: [importedDocument()] });

    await renderBrowser();

    expect(screen.getByText("In library")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /open biology 2e in the reader/i }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /add/i })).not.toBeInTheDocument();
  });

  it("opens the imported document, not the catalog entry", async () => {
    useLibraryStore.setState({ documents: [importedDocument()] });
    const onOpenDocument = await renderBrowser();

    await userEvent.click(screen.getByRole("button", { name: /open biology 2e in the reader/i }));

    expect(onOpenDocument).toHaveBeenCalledWith({
      id: "doc-9",
      title: "Biology 2e (imported)",
    });
  });

  it("does not treat a LibreTexts import of the same id as in-library", async () => {
    // findImportedBook filters on sourceType too. Without that, a LibreTexts
    // book whose book_id collided with an OpenStax uuid would render Open and
    // hand the reader a document from the wrong catalog.
    useLibraryStore.setState({
      documents: [
        importedDocument({
          sourceType: "libretexts",
          sourceMetadata: { book_uuid: "uuid-42" },
        }),
      ],
    });

    await renderBrowser();

    expect(screen.getByRole("button", { name: /add/i })).toBeInTheDocument();
    expect(screen.queryByText("In library")).not.toBeInTheDocument();
  });
});
