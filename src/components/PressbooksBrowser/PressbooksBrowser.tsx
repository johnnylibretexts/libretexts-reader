import { useEffect, useState } from "react";
import { BookOpen, ExternalLink, Loader2, Plus } from "lucide-react";
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
 * One Catalog at a time, listed whole. Pressbooks ignores its own `search`
 * parameter and caps a page at ten books, so searching means enumerating
 * first — that is a separate ticket. "Network" is Pressbooks' own word for a
 * Catalog and appears in the picker label for that reason; nothing here is
 * typed after it.
 */
export function PressbooksBrowser({ onOpenDocument }: PressbooksBrowserProps) {
  const [catalogs, setCatalogs] = useState<Domain.PressbooksCatalog[]>([]);
  const [host, setHost] = useState<string | null>(null);
  const [books, setBooks] = useState<Domain.PressbooksBook[]>([]);
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
          setHost((current) => current ?? offered[0]?.host ?? null);
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

  useEffect(() => {
    if (!host) {
      return;
    }

    let active = true;
    setLoading(true);
    setError(null);
    // Cleared on every switch: leaving the previous Catalog's books on screen
    // under a new Catalog's name would misattribute them.
    setBooks([]);
    api
      .listPressbooksBooks(host)
      .then((catalog) => {
        if (active) {
          setBooks(catalog);
        }
      })
      .catch((error) => {
        if (active) {
          setError(displayError(error));
          setBooks([]);
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
  }, [host]);

  async function importBook(book: Domain.PressbooksBook) {
    await startImport({
      bookId: book.bookUrl,
      title: book.title,
      run: () => api.importPressbooks(book.bookUrl),
    });
  }

  return (
    <section className="flex flex-col gap-4">
      <div className="grid gap-3 rounded-md border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900 md:grid-cols-[minmax(14rem,1fr)]">
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

      {error ? (
        <p className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700 dark:border-red-950 dark:bg-red-950/30 dark:text-red-300">
          {error}
        </p>
      ) : null}

      {loading ? (
        <div className="flex items-center gap-2 rounded-md border border-neutral-200 bg-white px-4 py-3 text-sm text-neutral-600 dark:border-neutral-800 dark:bg-neutral-900 dark:text-neutral-300">
          <Loader2 className="size-4 animate-spin" aria-hidden="true" />
          Loading Pressbooks catalog
        </div>
      ) : null}

      {!loading && books.length === 0 && !error ? (
        <div className="rounded-md border border-neutral-200 bg-white px-4 py-6 text-sm text-neutral-600 dark:border-neutral-800 dark:bg-neutral-900 dark:text-neutral-300">
          {catalogs.length === 0
            ? "No Pressbooks networks are available."
            : "This Pressbooks network has no books to show."}
        </div>
      ) : null}

      <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
        {books.map((book) => {
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
