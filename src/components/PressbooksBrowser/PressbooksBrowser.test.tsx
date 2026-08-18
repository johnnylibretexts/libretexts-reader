import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type * as Domain from "../../types/domain";

const listPressbooksCatalogs = vi.fn();
const listPressbooksBooks = vi.fn();
const importPressbooks = vi.fn();

vi.mock("../../lib/tauri", () => ({
  isTauriRuntime: () => true,
  api: {
    get listPressbooksCatalogs() {
      return listPressbooksCatalogs;
    },
    get listPressbooksBooks() {
      return listPressbooksBooks;
    },
    get importPressbooks() {
      return importPressbooks;
    },
  },
}));

const { PressbooksBrowser } = await import("./PressbooksBrowser");
const { useImportsStore } = await import("../../stores/imports");
const { useLibraryStore } = await import("../../stores/library");

const CATALOGS: Domain.PressbooksCatalog[] = [
  { host: "milnepublishing.geneseo.edu", name: "Milne Publishing", bookCount: 90 },
  { host: "oer.pressbooks.pub", name: "PressbooksOER", bookCount: 43 },
];

const OTHER_BOOK: Domain.PressbooksBook = {
  bookUrl: "https://oer.pressbooks.pub/openteach/",
  title: "Openteach",
  subtitle: null,
  coverUrl: null,
  thumbnailUrl: null,
  authors: "Orna Farrell",
  licenseName: "CC BY",
  licenseUrl: null,
  wordCount: 25637,
};

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
    listPressbooksCatalogs.mockResolvedValue(CATALOGS);
    listPressbooksBooks.mockImplementation((host: string) =>
      Promise.resolve(host === "oer.pressbooks.pub" ? [OTHER_BOOK] : [BOOK]),
    );
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

  it("offers every bundled catalog in the picker and opens on the first", async () => {
    await renderBrowser();

    const picker = screen.getByRole("combobox", { name: /network/i });
    expect(picker).toHaveValue("milnepublishing.geneseo.edu");
    expect(
      screen.getByRole("option", { name: /Milne Publishing \(90 books\)/ }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("option", { name: /PressbooksOER \(43 books\)/ }),
    ).toBeInTheDocument();
    expect(listPressbooksBooks).toHaveBeenCalledWith("milnepublishing.geneseo.edu");
  });

  it("shows the chosen catalog's books when the reader switches", async () => {
    await renderBrowser();

    await userEvent.selectOptions(
      screen.getByRole("combobox", { name: /network/i }),
      "oer.pressbooks.pub",
    );

    await waitFor(() => expect(screen.getByText("Openteach")).toBeInTheDocument());
    expect(listPressbooksBooks).toHaveBeenLastCalledWith("oer.pressbooks.pub");
  });

  it("drops the previous catalog's books as soon as the reader switches", async () => {
    // Asserted while the new Catalog is still loading. Checking only after it
    // arrives proves nothing -- the new list replaces the old either way. The
    // window this covers is the one where a reader would otherwise see one
    // Catalog's books sitting under another Catalog's name.
    let release: (books: Domain.PressbooksBook[]) => void = () => {};
    listPressbooksBooks.mockImplementation((host: string) =>
      host === "oer.pressbooks.pub"
        ? new Promise((resolve) => {
            release = resolve;
          })
        : Promise.resolve([BOOK]),
    );

    await renderBrowser();
    await userEvent.selectOptions(
      screen.getByRole("combobox", { name: /network/i }),
      "oer.pressbooks.pub",
    );

    await waitFor(() =>
      expect(
        screen.queryByText("A Concise Introduction to Logic"),
      ).not.toBeInTheDocument(),
    );

    release([OTHER_BOOK]);
    await waitFor(() => expect(screen.getByText("Openteach")).toBeInTheDocument());
  });

  it("keeps the picker usable when the chosen catalog cannot be reached", async () => {
    // A Catalog that will not load must not strand the reader on it: the
    // bundled list needs no network, so switching away stays possible.
    listPressbooksBooks.mockImplementation((host: string) =>
      host === "milnepublishing.geneseo.edu"
        ? Promise.reject({
            kind: "pressbooks",
            message: "request to https://milne.test failed with HTTP 503",
            retryable: true,
          })
        : Promise.resolve([OTHER_BOOK]),
    );

    render(<PressbooksBrowser onOpenDocument={vi.fn()} />);
    await waitFor(() =>
      expect(screen.getByText(/failed with HTTP 503/)).toBeInTheDocument(),
    );

    await userEvent.selectOptions(
      screen.getByRole("combobox", { name: /network/i }),
      "oer.pressbooks.pub",
    );

    await waitFor(() => expect(screen.getByText("Openteach")).toBeInTheDocument());
    expect(screen.queryByText(/failed with HTTP 503/)).not.toBeInTheDocument();
  });

  it("says so rather than spinning forever when no catalogs are on offer", async () => {
    listPressbooksCatalogs.mockResolvedValue([]);

    render(<PressbooksBrowser onOpenDocument={vi.fn()} />);

    await waitFor(() =>
      expect(
        screen.getByText("No Pressbooks networks are available."),
      ).toBeInTheDocument(),
    );
    expect(screen.queryByText(/Loading Pressbooks catalog/)).not.toBeInTheDocument();
  });

  it("reports a catalog that cannot be reached rather than showing it as empty", async () => {
    listPressbooksBooks.mockRejectedValue({
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
