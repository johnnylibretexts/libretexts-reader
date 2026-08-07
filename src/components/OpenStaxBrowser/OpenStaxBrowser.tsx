import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { BookOpen, ExternalLink, Loader2, Plus, Search } from "lucide-react";
import { api } from "../../lib/tauri";
import type * as Domain from "../../types/domain";
import { displayError } from "../../lib/errors";

interface OpenStaxBrowserProps {
  onImported: (documentId: string, title: string) => void;
}

export function OpenStaxBrowser({ onImported }: OpenStaxBrowserProps) {
  const [books, setBooks] = useState<Domain.OpenStaxBook[]>([]);
  const [query, setQuery] = useState("");
  const [subject, setSubject] = useState("all");
  const [language, setLanguage] = useState("all");
  const [loading, setLoading] = useState(true);
  const [importingUuid, setImportingUuid] = useState<string | null>(null);
  const [progress, setProgress] = useState<Domain.ImportProgress | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    api
      .listOpenstaxCatalog()
      .then((catalog) => {
        if (active) {
          setBooks(catalog);
        }
      })
      .catch((error) => {
        if (active) {
          setError(displayError(error));
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
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void listen<Domain.ImportProgress>("import-progress", (event) => {
      if (event.payload.documentId === importingUuid) {
        setProgress(event.payload);
      }
    })
      .then((dispose) => {
        unlisten = dispose;
      })
      .catch(() => undefined);

    return () => {
      unlisten?.();
    };
  }, [importingUuid]);

  const subjects = useMemo(
    () => uniqueOptions(books.map((book) => book.subject)),
    [books],
  );
  const languages = useMemo(
    () => uniqueOptions(books.map((book) => book.language)),
    [books],
  );
  const filteredBooks = useMemo(() => {
    const normalizedQuery = query.trim().toLowerCase();
    return books.filter((book) => {
      const matchesQuery =
        !normalizedQuery ||
        book.title.toLowerCase().includes(normalizedQuery) ||
        book.subject.toLowerCase().includes(normalizedQuery);
      const matchesSubject = subject === "all" || book.subject === subject;
      const matchesLanguage = language === "all" || book.language === language;
      return matchesQuery && matchesSubject && matchesLanguage;
    });
  }, [books, language, query, subject]);

  async function importBook(book: Domain.OpenStaxBook) {
    if (importingUuid) {
      return;
    }

    setError(null);
    setProgress({
      documentId: book.uuid,
      stage: "fetching",
      current: 0,
      total: 0,
      message: null,
    });
    setImportingUuid(book.uuid);

    try {
      const documentId = await api.importOpenstax(book.uuid);
      onImported(documentId, book.title);
    } catch (error) {
      setError(displayError(error));
    } finally {
      setImportingUuid(null);
      setProgress(null);
    }
  }

  return (
    <section className="flex flex-col gap-4">
      <div className="grid gap-3 rounded-md border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900 md:grid-cols-[minmax(14rem,1fr)_12rem_9rem]">
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
        <label className="flex flex-col gap-2 text-sm font-medium">
          Subject
          <select
            className="h-10 rounded-md border border-neutral-200 bg-white px-3 text-sm font-normal outline-none focus:border-brand-500 focus:ring-2 focus:ring-brand-500/20 dark:border-neutral-800 dark:bg-neutral-950"
            onChange={(event) => setSubject(event.target.value)}
            value={subject}
          >
            <option value="all">All</option>
            {subjects.map((option) => (
              <option key={option} value={option}>
                {option}
              </option>
            ))}
          </select>
        </label>
        <label className="flex flex-col gap-2 text-sm font-medium">
          Language
          <select
            className="h-10 rounded-md border border-neutral-200 bg-white px-3 text-sm font-normal outline-none focus:border-brand-500 focus:ring-2 focus:ring-brand-500/20 dark:border-neutral-800 dark:bg-neutral-950"
            onChange={(event) => setLanguage(event.target.value)}
            value={language}
          >
            <option value="all">All</option>
            {languages.map((option) => (
              <option key={option} value={option}>
                {option.toUpperCase()}
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
          Loading OpenStax catalog
        </div>
      ) : null}

      {!loading && filteredBooks.length === 0 ? (
        <div className="rounded-md border border-neutral-200 bg-white px-4 py-6 text-sm text-neutral-600 dark:border-neutral-800 dark:bg-neutral-900 dark:text-neutral-300">
          No OpenStax books matched the current search.
        </div>
      ) : null}

      <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
        {filteredBooks.map((book) => (
          <article
            className="flex min-h-52 flex-col justify-between gap-4 rounded-md border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900"
            key={book.uuid}
          >
            <div className="flex min-w-0 gap-3">
              <OpenStaxThumbnail coverUrl={book.coverUrl} />
              <div className="min-w-0">
                <h2 className="line-clamp-2 text-base font-semibold">
                  {book.title}
                </h2>
                <p className="mt-1 line-clamp-2 text-sm text-neutral-600 dark:text-neutral-400">
                  {book.subject} · {book.language.toUpperCase()}
                </p>
              </div>
            </div>

            <p className="line-clamp-3 text-sm text-neutral-600 dark:text-neutral-400">
              {book.license}
            </p>

            <div className="flex items-center justify-between gap-3">
              <p className="min-h-5 min-w-0 truncate text-sm text-neutral-500 dark:text-neutral-400">
                {importingUuid === book.uuid && progress
                  ? formatOpenStaxProgress(progress)
                  : `Edition ${book.edition}`}
              </p>
              <div className="flex shrink-0 items-center gap-2">
                <a
                  aria-label={`Open ${book.title} on OpenStax`}
                  className="grid size-9 place-items-center rounded-md border border-neutral-200 text-neutral-600 hover:bg-stone-100 focus:outline-none focus:ring-2 focus:ring-brand-500 dark:border-neutral-800 dark:text-neutral-300 dark:hover:bg-neutral-800"
                  href={openStaxBookUrl(book)}
                  rel="noreferrer"
                  target="_blank"
                  title="Open source"
                >
                  <ExternalLink className="size-4" aria-hidden="true" />
                </a>
                <button
                  className="inline-flex h-9 items-center gap-2 rounded-md bg-brand-700 px-3 text-sm font-medium text-white hover:bg-brand-500 disabled:cursor-not-allowed disabled:opacity-50"
                  disabled={Boolean(importingUuid)}
                  onClick={() => void importBook(book)}
                  type="button"
                >
                  {importingUuid === book.uuid ? (
                    <Loader2 className="size-4 animate-spin" aria-hidden="true" />
                  ) : (
                    <Plus className="size-4" aria-hidden="true" />
                  )}
                  Add
                </button>
              </div>
            </div>
          </article>
        ))}
      </div>
    </section>
  );
}

function OpenStaxThumbnail({ coverUrl }: { coverUrl: string | null }) {
  const [failed, setFailed] = useState(false);
  const showThumbnail = coverUrl && !failed;

  useEffect(() => {
    setFailed(false);
  }, [coverUrl]);

  return (
    <div className="grid size-12 shrink-0 place-items-center overflow-hidden rounded-md bg-brand-50 text-brand-700 dark:bg-neutral-800 dark:text-brand-500">
      {showThumbnail ? (
        <img
          alt=""
          className="size-full object-cover"
          loading="lazy"
          onError={() => setFailed(true)}
          src={coverUrl}
        />
      ) : (
        <BookOpen className="size-5" aria-hidden="true" />
      )}
    </div>
  );
}

function uniqueOptions(values: string[]) {
  return [...new Set(values.filter(Boolean))].sort((left, right) =>
    left.localeCompare(right),
  );
}

function formatOpenStaxProgress(progress: Domain.ImportProgress) {
  if (progress.total > 0) {
    return `Page ${progress.current}/${progress.total}`;
  }

  return "Preparing pages...";
}

function openStaxBookUrl(book: Domain.OpenStaxBook) {
  return `https://openstax.org/books/${encodeURIComponent(book.slug)}`;
}
