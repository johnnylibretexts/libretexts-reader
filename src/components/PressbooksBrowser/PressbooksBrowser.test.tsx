import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type * as Domain from "../../types/domain";

const listPressbooksCatalogs = vi.fn();
const listPressbooksBooks = vi.fn();
const searchPressbooksBooks = vi.fn();
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
    get searchPressbooksBooks() {
      return searchPressbooksBooks;
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
    searchPressbooksBooks.mockResolvedValue([]);
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

  describe("search", () => {
    /** A Catalog holding `books`, searched the way the Rust side searches it. */
    function withCatalog(books: Domain.PressbooksBook[]) {
      listPressbooksBooks.mockResolvedValue(books);
      searchPressbooksBooks.mockImplementation((_host: string, query: string) =>
        Promise.resolve(
          books.filter((book) =>
            [book.title, book.subtitle ?? "", book.authors].some((field) =>
              field.toLowerCase().includes(query.trim().toLowerCase()),
            ),
          ),
        ),
      );
    }

    function searchBox() {
      return screen.getByRole("textbox", { name: /search/i });
    }

    it("narrows the listing to the books that match what is typed", async () => {
      withCatalog([BOOK, OTHER_BOOK]);
      await renderBrowser();

      await userEvent.type(searchBox(), "openteach");

      await waitFor(() =>
        expect(
          screen.queryByText("A Concise Introduction to Logic"),
        ).not.toBeInTheDocument(),
      );
      expect(screen.getByText("Openteach")).toBeInTheDocument();
    });

    it("searches the cache rather than re-listing the catalog", async () => {
      // Re-listing would put the Catalog's freshness check -- a network
      // request -- behind every keystroke.
      withCatalog([BOOK, OTHER_BOOK]);
      await renderBrowser();

      await userEvent.type(searchBox(), "logic");

      await waitFor(() =>
        expect(searchPressbooksBooks).toHaveBeenLastCalledWith(
          "milnepublishing.geneseo.edu",
          "logic",
        ),
      );
      expect(listPressbooksBooks).toHaveBeenCalledTimes(1);
    });

    it("restores the whole catalog when the search is cleared", async () => {
      withCatalog([BOOK, OTHER_BOOK]);
      await renderBrowser();
      await userEvent.type(searchBox(), "openteach");
      await waitFor(() =>
        expect(
          screen.queryByText("A Concise Introduction to Logic"),
        ).not.toBeInTheDocument(),
      );

      await userEvent.clear(searchBox());

      await waitFor(() =>
        expect(
          screen.getByText("A Concise Introduction to Logic"),
        ).toBeInTheDocument(),
      );
      expect(screen.getByText("Openteach")).toBeInTheDocument();
    });

    it("answers a search typed while the first catalog is still loading", async () => {
      // The cache is empty until the crawl commits, and the reader can type
      // before then. An empty answer kept from that moment would outlive the
      // Catalog's arrival.
      const listed = new Set<string>();
      let release: (books: Domain.PressbooksBook[]) => void = () => {};
      listPressbooksBooks.mockImplementation(
        (host: string) =>
          new Promise<Domain.PressbooksBook[]>((resolve) => {
            release = (books) => {
              listed.add(host);
              resolve(books);
            };
          }),
      );
      searchPressbooksBooks.mockImplementation((host: string, query: string) =>
        Promise.resolve(
          (listed.has(host) ? [BOOK, OTHER_BOOK] : []).filter((book) =>
            book.title.toLowerCase().includes(query.trim().toLowerCase()),
          ),
        ),
      );

      render(<PressbooksBrowser onOpenDocument={vi.fn()} />);
      await waitFor(() =>
        expect(screen.getByText(/Loading Pressbooks catalog/)).toBeInTheDocument(),
      );
      await userEvent.type(searchBox(), "logic");
      release([BOOK, OTHER_BOOK]);

      await waitFor(() =>
        expect(
          screen.getByText("A Concise Introduction to Logic"),
        ).toBeInTheDocument(),
      );
      // The Catalog arriving must not wash the search out: the book that does
      // not match has to stay gone.
      expect(screen.queryByText("Openteach")).not.toBeInTheDocument();
      expect(screen.queryByText(/No books match/i)).not.toBeInTheDocument();
      // One search, issued after the Catalog was listed. A search sent while
      // the cache was still empty would answer "nothing" about a Catalog that
      // had not arrived, and flash that answer when it did.
      expect(searchPressbooksBooks).toHaveBeenCalledTimes(1);
    });

    it("searches again once a newly chosen catalog has been listed", async () => {
      // The cache a search reads is the one listing the Catalog fills, so a
      // search issued before the listing lands asks an empty cache. Keeping
      // that answer would leave the reader on "no matches" after the Catalog
      // they switched to had arrived.
      const listed = new Set<string>(["milnepublishing.geneseo.edu"]);
      let release: (books: Domain.PressbooksBook[]) => void = () => {};
      listPressbooksBooks.mockImplementation((host: string) =>
        host === "oer.pressbooks.pub"
          ? new Promise<Domain.PressbooksBook[]>((resolve) => {
              release = (books) => {
                listed.add(host);
                resolve(books);
              };
            })
          : Promise.resolve([BOOK]),
      );
      searchPressbooksBooks.mockImplementation((host: string, query: string) => {
        const cached = listed.has(host)
          ? host === "oer.pressbooks.pub"
            ? // Two books, so a filter that stopped being applied would show.
              [OTHER_BOOK, BOOK]
            : [BOOK]
          : [];
        return Promise.resolve(
          cached.filter((book) =>
            book.title.toLowerCase().includes(query.trim().toLowerCase()),
          ),
        );
      });

      await renderBrowser();
      await userEvent.type(searchBox(), "openteach");
      await waitFor(() =>
        expect(screen.getByText(/No books match/i)).toBeInTheDocument(),
      );

      await userEvent.selectOptions(
        screen.getByRole("combobox", { name: /network/i }),
        "oer.pressbooks.pub",
      );
      release([OTHER_BOOK, BOOK]);

      await waitFor(() => expect(screen.getByText("Openteach")).toBeInTheDocument());
      expect(
        screen.queryByText("A Concise Introduction to Logic"),
      ).not.toBeInTheDocument();
    });

    it("says a search matched nothing, distinctly from a catalog with no books", async () => {
      withCatalog([BOOK]);
      await renderBrowser();

      await userEvent.type(searchBox(), "thermodynamics");

      await waitFor(() =>
        expect(screen.getByText(/No books match/i)).toBeInTheDocument(),
      );
      // The reader has to be able to tell "your term found nothing" from
      // "this Catalog is empty" and from "this Catalog would not load".
      expect(
        screen.queryByText("This Pressbooks network has no books to show."),
      ).not.toBeInTheDocument();
      expect(screen.queryByText(/failed with HTTP/)).not.toBeInTheDocument();
    });
  });
});
