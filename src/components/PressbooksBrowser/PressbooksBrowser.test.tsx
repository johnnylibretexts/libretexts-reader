import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type * as Domain from "../../types/domain";

const listPressbooksCatalog = vi.fn();
const importPressbooks = vi.fn();

vi.mock("../../lib/tauri", () => ({
  isTauriRuntime: () => true,
  api: {
    get listPressbooksCatalog() {
      return listPressbooksCatalog;
    },
    get importPressbooks() {
      return importPressbooks;
    },
  },
}));

const { PressbooksBrowser } = await import("./PressbooksBrowser");
const { useImportsStore } = await import("../../stores/imports");
const { useLibraryStore } = await import("../../stores/library");

const BOOK: Domain.PressbooksBook = {
  bookUrl: "https://milnepublishing.geneseo.edu/concise-introduction-to-logic/",
  title: "A Concise Introduction to Logic",
  subtitle: null,
  coverUrl: null,
  thumbnailUrl: null,
  authors: "Craig DeLancey",
  licenseName: "CC BY-NC-SA",
  licenseUrl: null,
  wordCount: 90000,
};

/** A library document that was imported from BOOK. */
function importedDocument(overrides: Partial<Domain.Document> = {}): Domain.Document {
  return {
    id: "doc-1",
    // Deliberately not the catalog title: the Open action must carry the
    // document's identity, not the card's.
    title: "A Concise Introduction to Logic (imported)",
    sourceType: "pressbooks",
    sourceMetadata: { book_url: BOOK.bookUrl },
    coverImagePath: null,
    license: null,
    attribution: null,
    wordCount: 0,
    importedAt: "2026-08-17T00:00:00Z",
    lastOpenedAt: null,
    ...overrides,
  };
}

async function renderBrowser(onOpenDocument = vi.fn()) {
  render(<PressbooksBrowser onOpenDocument={onOpenDocument} />);
  await waitFor(() =>
    expect(screen.getByText("A Concise Introduction to Logic")).toBeInTheDocument(),
  );
  return onOpenDocument;
}

describe("PressbooksBrowser", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    listPressbooksCatalog.mockResolvedValue([BOOK]);
    importPressbooks.mockResolvedValue("doc-1");
    useImportsStore.setState({ active: null, completed: null, error: null });
    useLibraryStore.setState({ documents: [] });
  });

  it("lists a catalog book with its author and licence", async () => {
    await renderBrowser();

    expect(screen.getByText("Craig DeLancey")).toBeInTheDocument();
    expect(screen.getByText("CC BY-NC-SA")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /add/i })).toBeInTheDocument();
  });

  it("imports the book by its canonical URL", async () => {
    await renderBrowser();

    await userEvent.click(screen.getByRole("button", { name: /add/i }));

    await waitFor(() => expect(importPressbooks).toHaveBeenCalledWith(BOOK.bookUrl));
  });

  it("replaces Add with an Open action once the book is in the library", async () => {
    useLibraryStore.setState({ documents: [importedDocument()] });

    await renderBrowser();

    expect(screen.getByText("In library")).toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: /open a concise introduction to logic in the reader/i,
      }),
    ).toBeInTheDocument();
    // "in place of + Add" -- both must not be offered at once.
    expect(screen.queryByRole("button", { name: /add/i })).not.toBeInTheDocument();
  });

  it("opens the imported document, not the catalog entry", async () => {
    useLibraryStore.setState({ documents: [importedDocument()] });
    const onOpenDocument = await renderBrowser();

    await userEvent.click(
      screen.getByRole("button", {
        name: /open a concise introduction to logic in the reader/i,
      }),
    );

    expect(onOpenDocument).toHaveBeenCalledWith({
      id: "doc-1",
      title: "A Concise Introduction to Logic (imported)",
    });
  });

  it("disables Add while an import started from another catalog is running", async () => {
    // The guard is global and lives in the imports store, so an OpenStax
    // import must disable Add here too -- otherwise a reader can start a
    // second import from a catalog the first one never touched.
    useImportsStore.setState({
      active: {
        bookId: "some-openstax-uuid",
        title: "Biology 2e",
        stage: "fetching",
        current: 1,
        total: 10,
      },
    });

    await renderBrowser();

    expect(screen.getByRole("button", { name: /add/i })).toBeDisabled();
  });

  it("keeps Open usable while an unrelated import is running", async () => {
    useLibraryStore.setState({ documents: [importedDocument()] });
    useImportsStore.setState({
      active: {
        bookId: "some-openstax-uuid",
        title: "Biology 2e",
        stage: "fetching",
        current: 1,
        total: 10,
      },
    });
    const onOpenDocument = await renderBrowser();

    const open = screen.getByRole("button", {
      name: /open a concise introduction to logic in the reader/i,
    });
    expect(open).toBeEnabled();

    await userEvent.click(open);
    expect(onOpenDocument).toHaveBeenCalledTimes(1);
  });

  it("reports a catalog that cannot be reached rather than showing it as empty", async () => {
    listPressbooksCatalog.mockRejectedValue({
      kind: "pressbooks",
      message: "request to https://milne.test failed with HTTP 503",
      retryable: true,
    });

    render(<PressbooksBrowser onOpenDocument={vi.fn()} />);

    await waitFor(() =>
      expect(screen.getByText(/failed with HTTP 503/)).toBeInTheDocument(),
    );
    // An unreachable Catalog must not read as a Catalog with no books in it.
    expect(
      screen.queryByText("This Pressbooks network has no books to show."),
    ).not.toBeInTheDocument();
  });
});
