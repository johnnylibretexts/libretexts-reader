import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type * as Domain from "../../types/domain";

const listPressbooksCatalogs = vi.fn();
const listPressbooksBooks = vi.fn();
const searchPressbooksBooks = vi.fn();
const importPressbooks = vi.fn();

const listen = vi.fn();

// The crawl reports progress on its own Tauri event; the component subscribes.
vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, handler: (event: unknown) => void) =>
    listen(name, handler),
}));

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
  // Ordered as the bundled resource is, largest first, with the default
  // neither first nor last. A fixture that put the default first would let a
  // component that opens on `[0]` pass while crawling three thousand books.
  {
    host: "ecampusontario.pressbooks.pub",
    name: "eCampusOntario",
    bookCount: 3033,
    isDefault: false,
  },
  {
    host: "milnepublishing.geneseo.edu",
    name: "Milne Publishing",
    bookCount: 90,
    isDefault: true,
  },
  {
    host: "oer.pressbooks.pub",
    name: "PressbooksOER",
    bookCount: 43,
    isDefault: false,
  },
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

/** A complete Catalog holding `books`. */
function catalogListing(
  books: Domain.PressbooksBook[],
  overrides: Partial<Domain.PressbooksCatalogListing> = {},
): Domain.PressbooksCatalogListing {
  return {
    books,
    totalBooks: books.length,
    isComplete: true,
    ...overrides,
  };
}

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
      Promise.resolve(
        catalogListing(host === "oer.pressbooks.pub" ? [OTHER_BOOK] : [BOOK]),
      ),
    );
    listen.mockResolvedValue(() => {});
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

  it("offers every bundled catalog in the picker", async () => {
    await renderBrowser();

    expect(
      screen.getByRole("option", { name: /eCampusOntario \(3033 books\)/ }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("option", { name: /Milne Publishing \(90 books\)/ }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("option", { name: /PressbooksOER \(43 books\)/ }),
    ).toBeInTheDocument();
  });

  it("opens on the catalog marked as the default, not the first offered", async () => {
    // Opening a Catalog crawls it, and the offered list is ordered by size, so
    // opening on the first one costs a three-hundred-request crawl before the
    // reader has asked for anything.
    await renderBrowser();

    expect(screen.getByRole("combobox", { name: /network/i })).toHaveValue(
      "milnepublishing.geneseo.edu",
    );
    expect(listPressbooksBooks).toHaveBeenCalledTimes(1);
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
    let release: (listing: Domain.PressbooksCatalogListing) => void = () => {};
    listPressbooksBooks.mockImplementation((host: string) =>
      host === "oer.pressbooks.pub"
        ? new Promise((resolve) => {
            release = resolve;
          })
        : Promise.resolve(catalogListing([BOOK])),
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

    release(catalogListing([OTHER_BOOK]));
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
        : Promise.resolve(catalogListing([OTHER_BOOK])),
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

  describe("a large catalog", () => {
    /** Hands back the handler the component registered for `catalog-progress`. */
    function progressHandler() {
      // The last one: the component re-subscribes when the Catalog changes, and
      // only the live subscription knows which host it is reporting for.
      const registrations = listen.mock.calls.filter(
        ([event]) => event === "catalog-progress",
      );
      const registration = registrations[registrations.length - 1];
      expect(registration).toBeDefined();
      return registration![1] as (event: {
        payload: Domain.PressbooksCatalogProgress;
      }) => void;
    }

    it("subscribes to catalog-progress before starting the crawl", async () => {
      // crawl_catalog reports progress synchronously, before it issues any
      // request. A subscription registered after the crawl has already
      // started can miss that first event.
      render(<PressbooksBrowser onOpenDocument={vi.fn()} />);

      await waitFor(() =>
        expect(listen).toHaveBeenCalledWith("catalog-progress", expect.any(Function)),
      );
      expect(listPressbooksBooks).toHaveBeenCalled();

      const listenOrder = listen.mock.invocationCallOrder[0];
      const crawlOrder = listPressbooksBooks.mock.invocationCallOrder[0];
      expect(listenOrder).toBeLessThan(crawlOrder);
    });

    it("shows how far a crawl has got while it runs", async () => {
      // Three hundred requests with no sign of movement is indistinguishable
      // from a frozen application.
      let release: (listing: Domain.PressbooksCatalogListing) => void = () => {};
      listPressbooksBooks.mockImplementation(
        () =>
          new Promise<Domain.PressbooksCatalogListing>((resolve) => {
            release = resolve;
          }),
      );

      render(<PressbooksBrowser onOpenDocument={vi.fn()} />);
      await waitFor(() =>
        expect(screen.getByText(/Loading Pressbooks catalog/)).toBeInTheDocument(),
      );

      act(() =>
        progressHandler()({
          payload: {
            host: "milnepublishing.geneseo.edu",
            current: 42,
            total: 304,
          },
        }),
      );

      await waitFor(() =>
        expect(screen.getByText(/page 42 of 304/)).toBeInTheDocument(),
      );

      release(catalogListing([BOOK]));
      await waitFor(() =>
        expect(screen.queryByText(/page 42 of 304/)).not.toBeInTheDocument(),
      );
    });

    it("ignores progress reported for a catalog the reader is not looking at", async () => {
      // A crawl the reader navigated away from keeps running and keeps
      // reporting. Its pages must not drive this Catalog's indicator.
      listPressbooksBooks.mockImplementation(
        () => new Promise<Domain.PressbooksCatalogListing>(() => {}),
      );

      render(<PressbooksBrowser onOpenDocument={vi.fn()} />);
      await waitFor(() =>
        expect(screen.getByText(/Loading Pressbooks catalog/)).toBeInTheDocument(),
      );

      act(() =>
        progressHandler()({
          payload: { host: "oer.pressbooks.pub", current: 42, total: 304 },
        }),
      );

      expect(screen.queryByText(/page 42 of 304/)).not.toBeInTheDocument();
    });

    it("says how much of an unfinished catalog it is showing", async () => {
      // A partial Catalog that showed only its books would read as a small
      // complete one, and the reader would take a fifth of it for all of it.
      listPressbooksBooks.mockResolvedValue(
        catalogListing([BOOK], { totalBooks: 3033, isComplete: false }),
      );

      await renderBrowser();

      expect(screen.getByText(/Showing 1 of 3033 books/)).toBeInTheDocument();
    });

    it("continues an unfinished catalog without starting again", async () => {
      listPressbooksBooks.mockResolvedValue(
        catalogListing([BOOK], { totalBooks: 3033, isComplete: false }),
      );
      await renderBrowser();
      listPressbooksBooks.mockResolvedValue(
        catalogListing([BOOK, OTHER_BOOK], { totalBooks: 2, isComplete: true }),
      );

      await userEvent.click(
        screen.getByRole("button", { name: /continue loading/i }),
      );

      // The Rust side resumes from the page it stopped on, so continuing is
      // the same request again rather than a different one.
      await waitFor(() =>
        expect(screen.getByText("Openteach")).toBeInTheDocument(),
      );
      expect(screen.queryByText(/did not finish loading/)).not.toBeInTheDocument();
    });

    it("says nothing about completeness for a catalog that loaded whole", async () => {
      await renderBrowser();

      expect(screen.queryByText(/did not finish loading/)).not.toBeInTheDocument();
      expect(
        screen.queryByRole("button", { name: /continue loading/i }),
      ).not.toBeInTheDocument();
    });
  });

  describe("search", () => {
    /** A Catalog holding `books`, searched the way the Rust side searches it. */
    function withCatalog(books: Domain.PressbooksBook[]) {
      listPressbooksBooks.mockResolvedValue(catalogListing(books));
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
        () =>
          new Promise<Domain.PressbooksCatalogListing>((resolve) => {
            release = (books) => {
              listed.add("milnepublishing.geneseo.edu");
              resolve(catalogListing(books));
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
          ? new Promise<Domain.PressbooksCatalogListing>((resolve) => {
              release = (books) => {
                listed.add(host);
                resolve(catalogListing(books));
              };
            })
          : Promise.resolve(catalogListing([BOOK])),
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
