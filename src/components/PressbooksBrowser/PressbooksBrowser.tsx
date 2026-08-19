import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { BookOpen, ExternalLink, Loader2, Plus, Search } from "lucide-react";
import { api } from "../../lib/tauri";
import type * as Domain from "../../types/domain";
import { displayError } from "../../lib/errors";
import { useImportsStore } from "../../stores/imports";
import { useLibraryStore } from "../../stores/library";
import { findImportedBook } from "../../lib/importedBooks";

interface PressbooksBrowserProps {
  /** Same shape as LibraryGrid's callback — both open a book card in the reader. */
  onOpenDocument: (document: { id: string; title: string }) => void;
}

/**
 * One Catalog at a time, listed whole and searched locally. Pressbooks ignores
 * its own `search` parameter and caps a page at ten books, so a Catalog is
 * enumerated into the local cache once and every search after that reads the
 * cache — which is why typing costs no request. "Network" is Pressbooks' own
 * word for a Catalog and appears in the picker label for that reason; nothing
 * here is typed after it.
 */
export function PressbooksBrowser({ onOpenDocument }: PressbooksBrowserProps) {
  const [catalogs, setCatalogs] = useState<Domain.PressbooksCatalog[]>([]);
  const [host, setHost] = useState<string | null>(null);
  const [listing, setListing] = useState<Domain.PressbooksCatalogListing | null>(
    null,
  );
  // Pages fetched against pages needed, while a crawl is running. The largest
  // bundled Catalog is three hundred requests, which without this reads as a
  // frozen application.
  const [progress, setProgress] =
    useState<Domain.PressbooksCatalogProgress | null>(null);
  // Bumped to ask for the Catalog again, which is what continues an unfinished
  // crawl -- the Rust side resumes from the page it stopped on.
  const [continueToken, setContinueToken] = useState(0);
  // The Catalog whose books are in the local cache. Searching reads that cache,
  // so this is what says a search can be answered at all.
  const [listedHost, setListedHost] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  // null means "not searching", which is what distinguishes an unsearched
  // Catalog from a search that matched none of its books.
  const [matches, setMatches] = useState<Domain.PressbooksBook[] | null>(null);
  const [searchError, setSearchError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const activeImport = useImportsStore((state) => state.active);
  const startImport = useImportsStore((state) => state.start);
  const documents = useLibraryStore((state) => state.documents);

  // The bundled list needs no network, so the picker is populated before the
  // first Catalog is reached and stays usable if that Catalog cannot be.
  useEffect(() => {
    let active = true;
    api
      .listPressbooksCatalogs()
      .then((offered) => {
        if (active) {
          setCatalogs(offered);
          // The marked Catalog, not the first one. The list is ordered by size
          // and opening a Catalog crawls it, so `[0]` would spend three hundred
          // requests before the reader had asked for anything.
          const opensOn = offered.find((catalog) => catalog.isDefault) ?? offered[0];
          setHost((current) => current ?? opensOn?.host ?? null);
          // With no Catalog to open there is no second fetch to end the
          // spinner, so it is ended here rather than left running forever.
          if (offered.length === 0) {
            setLoading(false);
          }
        }
      })
      .catch((error) => {
        if (active) {
          setError(displayError(error));
          setLoading(false);
        }
      });

    return () => {
      active = false;
    };
  }, []);

  // Scoped to this component, unlike the Import listener: a crawl is awaited by
  // the call this component made, so a subscription outliving the component
  // would have nothing to report to.
  //
  // Declared before the effect that starts the crawl: crawl_catalog reports
  // progress synchronously, before it issues any request, so a subscription
  // registered after that effect can miss the first event entirely.
  useEffect(() => {
    // Nothing to report on until a Catalog is chosen, and a subscription made
    // before then could only ever compare against a host of `null`.
    if (!host) {
      return;
    }

    let active = true;
    let dispose: (() => void) | undefined;

    void listen<Domain.PressbooksCatalogProgress>("catalog-progress", (event) => {
      // A crawl the reader has navigated away from keeps running and keeps
      // reporting. Its progress must not drive the indicator for the Catalog
      // they are looking at now.
      if (event.payload.host === host) {
        setProgress(event.payload);
      }
    })
      .then((unlisten) => {
        if (active) {
          dispose = unlisten;
        } else {
          unlisten();
        }
      })
      .catch(() => undefined);

    return () => {
      active = false;
      dispose?.();
    };
  }, [host]);

  useEffect(() => {
    if (!host) {
      return;
    }

    let active = true;
    setLoading(true);
    setError(null);
    // Cleared on every switch: leaving the previous Catalog's books on screen
    // under a new Catalog's name would misattribute them. Matches go with
    // them -- they are the previous Catalog's books too.
    setListing(null);
    setMatches(null);
    setProgress(null);
    api
      .listPressbooksBooks(host)
      .then((catalog) => {
        if (active) {
          setListing(catalog);
          setListedHost(host);
        }
      })
      .catch((error) => {
        if (active) {
          setError(displayError(error));
          setListing(null);
        }
      })
      .finally(() => {
        if (active) {
          setLoading(false);
        }
      });

    return () => {
      active = false;
    };
  }, [host, continueToken]);

  // Undebounced on purpose: this reads the local cache listing the Catalog
  // filled, so a keystroke costs a SQL LIKE rather than a request. The `active`
  // flag is what keeps a slower earlier answer from landing on a later one.
  useEffect(() => {
    const term = query.trim();
    if (!host || term === "") {
      setMatches(null);
      setSearchError(null);
      return;
    }

    // Held until this Catalog is in the cache. Searching sooner asks an empty
    // cache, and answering "no matches" for a Catalog that has not arrived
    // would then stand even once it had.
    if (listedHost !== host) {
      return;
    }

    let active = true;
    api
      .searchPressbooksBooks(host, term)
      .then((found) => {
        if (active) {
          setMatches(found);
          setSearchError(null);
        }
      })
      .catch((error) => {
        if (active) {
          setMatches([]);
          setSearchError(displayError(error));
        }
      });

    return () => {
      active = false;
    };
  }, [host, listedHost, query]);

  const searchTerm = query.trim();
  const books = listing?.books ?? [];
  const visible = matches ?? books;
  const incomplete = Boolean(listing && !listing.isComplete);
  const failure = error ?? searchError;

  async function importBook(book: Domain.PressbooksBook) {
    await startImport({
      bookId: book.bookUrl,
      title: book.title,
      run: () => api.importPressbooks(book.bookUrl),
    });
  }

  return (
    <section className="flex flex-col gap-4">
      <div className="grid gap-3 rounded-md border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900 md:grid-cols-[minmax(14rem,1fr)_14rem]">
        <label className="flex min-w-0 flex-col gap-2 text-sm font-medium">
          Search
          <span className="relative">
            <Search
              className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-neutral-400"
              aria-hidden="true"
            />
            <input
              className="h-10 w-full rounded-md border border-neutral-200 bg-white pl-9 pr-3 text-sm font-normal outline-none focus:border-brand-500 focus:ring-2 focus:ring-brand-500/20 dark:border-neutral-800 dark:bg-neutral-950"
              onChange={(event) => setQuery(event.target.value)}
              value={query}
            />
          </span>
        </label>
        <label className="flex min-w-0 flex-col gap-2 text-sm font-medium">
          Network
          <select
            className="h-10 rounded-md border border-neutral-200 bg-white px-3 text-sm font-normal outline-none focus:border-brand-500 focus:ring-2 focus:ring-brand-500/20 dark:border-neutral-800 dark:bg-neutral-950"
            onChange={(event) => setHost(event.target.value)}
            value={host ?? ""}
          >
            {catalogs.map((catalog) => (
              <option key={catalog.host} value={catalog.host}>
                {catalog.name} ({catalog.bookCount} books)
              </option>
            ))}
          </select>
        </label>
      </div>

      {failure ? (
        <p className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700 dark:border-red-950 dark:bg-red-950/30 dark:text-red-300">
          {failure}
        </p>
      ) : null}

      {loading ? (
        <div className="flex flex-col gap-2 rounded-md border border-neutral-200 bg-white px-4 py-3 text-sm text-neutral-600 dark:border-neutral-800 dark:bg-neutral-900 dark:text-neutral-300">
          <span className="flex items-center gap-2">
            <Loader2 className="size-4 animate-spin" aria-hidden="true" />
            {progress
              ? `Loading Pressbooks catalog: page ${progress.current} of ${progress.total}`
              : "Loading Pressbooks catalog"}
          </span>
          {progress && progress.total > 0 ? (
            <progress
              className="h-1.5 w-full"
              max={progress.total}
              value={progress.current}
            />
          ) : null}
        </div>
      ) : null}

      {!loading && incomplete && listing ? (
        <div className="flex flex-wrap items-center justify-between gap-3 rounded-md border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-900 dark:border-amber-900 dark:bg-amber-950/30 dark:text-amber-200">
          {/* Counted rather than described: "incomplete" alone leaves a reader
              unable to tell a Catalog missing one page from one missing three
              hundred. */}
          <span>
            Showing {books.length} of {listing.totalBooks} books. This catalog
            did not finish loading.
          </span>
          <button
            className="inline-flex h-9 shrink-0 items-center gap-2 rounded-md border border-amber-300 px-3 text-sm font-medium hover:bg-amber-100 focus:outline-none focus:ring-2 focus:ring-brand-500 dark:border-amber-800 dark:hover:bg-amber-900/40"
            onClick={() => setContinueToken((token) => token + 1)}
            type="button"
          >
            Continue loading
          </button>
        </div>
      ) : null}

      {!loading && visible.length === 0 && !failure ? (
        <div className="rounded-md border border-neutral-200 bg-white px-4 py-6 text-sm text-neutral-600 dark:border-neutral-800 dark:bg-neutral-900 dark:text-neutral-300">
          {/* Three different nothings, and a reader has to be able to tell
              them apart: a term that found nothing, a Catalog with nothing in
              it, and no Catalogs at all. A Catalog that failed to load is the
              fourth and is reported above as an error, not here. */}
          {searchTerm
            ? `No books match “${searchTerm}”.`
            : catalogs.length === 0
              ? "No Pressbooks networks are available."
              : "This Pressbooks network has no books to show."}
        </div>
      ) : null}

      <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
        {visible.map((book) => {
          const imported = findImportedBook(
            documents,
            "pressbooks",
            "book_url",
            book.bookUrl,
          );

          return (
            <article
              className="flex min-h-52 flex-col justify-between gap-4 rounded-md border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900"
              key={book.bookUrl}
            >
              <div className="flex min-w-0 gap-3">
                <PressbooksThumbnail thumbnail={book.thumbnailUrl} />
                <div className="min-w-0">
                  <h2 className="line-clamp-2 text-base font-semibold">
                    {book.title}
                  </h2>
                  <p className="mt-1 line-clamp-2 text-sm text-neutral-600 dark:text-neutral-400">
                    {book.authors || "Pressbooks"}
                  </p>
                </div>
              </div>

              {book.subtitle ? (
                <p className="line-clamp-3 text-sm text-neutral-600 dark:text-neutral-400">
                  {book.subtitle}
                </p>
              ) : null}

              <div className="flex items-center justify-between gap-3">
                <p className="min-h-5 min-w-0 truncate text-sm text-neutral-500 dark:text-neutral-400">
                  {activeImport?.bookId === book.bookUrl
                    ? formatPressbooksProgress(activeImport)
                    : imported
                      ? "In library"
                      : book.licenseName || "Unknown licence"}
                </p>
                <div className="flex shrink-0 items-center gap-2">
                  <a
                    aria-label={`Open ${book.title} on Pressbooks`}
                    className="grid size-9 place-items-center rounded-md border border-neutral-200 text-neutral-600 hover:bg-stone-100 focus:outline-none focus:ring-2 focus:ring-brand-500 dark:border-neutral-800 dark:text-neutral-300 dark:hover:bg-neutral-800"
                    href={book.bookUrl}
                    rel="noreferrer"
                    target="_blank"
                    title="Open source"
                  >
                    <ExternalLink className="size-4" aria-hidden="true" />
                  </a>
                  {imported ? (
                    // Deliberately not disabled while another import runs:
                    // opening a book already on disk touches nothing the
                    // import guard protects.
                    <button
                      aria-label={`Open ${book.title} in the reader`}
                      className="inline-flex h-9 items-center gap-2 rounded-md border border-neutral-200 px-3 text-sm font-medium text-neutral-600 hover:bg-stone-100 focus:outline-none focus:ring-2 focus:ring-brand-500 dark:border-neutral-800 dark:text-neutral-300 dark:hover:bg-neutral-800"
                      onClick={() =>
                        onOpenDocument({ id: imported.id, title: imported.title })
                      }
                      type="button"
                    >
                      <BookOpen className="size-4" aria-hidden="true" />
                      Open
                    </button>
                  ) : (
                    <button
                      className="inline-flex h-9 items-center gap-2 rounded-md bg-brand-700 px-3 text-sm font-medium text-white hover:bg-brand-500 disabled:cursor-not-allowed disabled:opacity-50"
                      disabled={Boolean(activeImport)}
                      onClick={() => void importBook(book)}
                      type="button"
                    >
                      {activeImport?.bookId === book.bookUrl ? (
                        <Loader2 className="size-4 animate-spin" aria-hidden="true" />
                      ) : (
                        <Plus className="size-4" aria-hidden="true" />
                      )}
                      Add
                    </button>
                  )}
                </div>
              </div>
            </article>
          );
        })}
      </div>
    </section>
  );
}

function PressbooksThumbnail({ thumbnail }: { thumbnail: string | null }) {
  const [failed, setFailed] = useState(false);
  const showThumbnail = thumbnail && !failed;

  useEffect(() => {
    setFailed(false);
  }, [thumbnail]);

  return (
    <div className="grid size-12 shrink-0 place-items-center overflow-hidden rounded-md bg-brand-50 text-brand-700 dark:bg-neutral-800 dark:text-brand-500">
      {showThumbnail ? (
        <img
          alt=""
          className="size-full object-cover"
          loading="lazy"
          onError={() => setFailed(true)}
          src={thumbnail}
        />
      ) : (
        <BookOpen className="size-5" aria-hidden="true" />
      )}
    </div>
  );
}

function formatPressbooksProgress(progress: { current: number; total: number }) {
  if (progress.total > 0) {
    return `Section ${progress.current}/${progress.total}`;
  }

  return "Preparing sections...";
}
