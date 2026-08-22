import { useEffect, useMemo, useState } from "react";
import {
  BookOpen,
  FileText,
  ListMusic,
  PanelLeft,
  Search,
  SlidersHorizontal,
} from "lucide-react";
import { ImportStatus } from "./ImportStatus";
import { MiniPlayer } from "./MiniPlayer";
import { Sidebar } from "./Sidebar";
import { EpubDialog } from "./Import/EpubDialog";
import { PasteDialog } from "./Import/PasteDialog";
import { PdfDialog } from "./Import/PdfDialog";
import { UrlDialog } from "./Import/UrlDialog";
import { LibraryGrid } from "./Library/Grid";
import { LibreTextsBrowser } from "./LibreTextsBrowser/LibreTextsBrowser";
import { PressbooksBrowser } from "./PressbooksBrowser/PressbooksBrowser";
import { OpenStaxBrowser } from "./OpenStaxBrowser/OpenStaxBrowser";
import { Reader } from "./Reader/Reader";
import { SettingsPanel } from "./Settings/SettingsPanel";
import { attachImportListener } from "../stores/imports";
import { attachLibraryListener, useLibraryStore } from "../stores/library";
import { usePlayerStore } from "../stores/player";

export type RouteId =
  | "library"
  | "openstax"
  | "libretexts"
  | "pressbooks"
  | "epub"
  | "pdf"
  | "paste"
  | "url"
  | "reader"
  | "settings";

export interface Route {
  id: RouteId;
  label: string;
}

interface ReaderDocument {
  id: string;
  title: string;
}

const ROUTES: Record<RouteId, Route> = {
  library: { id: "library", label: "Library" },
  openstax: { id: "openstax", label: "OpenStax" },
  libretexts: { id: "libretexts", label: "LibreTexts" },
  pressbooks: { id: "pressbooks", label: "Pressbooks" },
  epub: { id: "epub", label: "EPUB Import" },
  pdf: { id: "pdf", label: "PDF Import" },
  paste: { id: "paste", label: "Pasted Text" },
  url: { id: "url", label: "Article URL" },
  reader: { id: "reader", label: "Reader" },
  settings: { id: "settings", label: "Settings" },
};

export function AppShell() {
  const [route, setRoute] = useState<Route>(ROUTES.library);
  const [readerDocument, setReaderDocument] = useState<ReaderDocument | null>(
    null,
  );
  const refreshLibrary = useLibraryStore((state) => state.refresh);
  const resetPlayer = usePlayerStore((state) => state.reset);

  const mainIcon = useMemo(() => routeIcon(route.id), [route.id]);

  useEffect(() => {
    void refreshLibrary();
  }, [refreshLibrary]);

  // Both subscriptions live with the stores they write, so each can guard the
  // desktop runtime and report a failure the same way. As an effect here, the
  // library one swallowed its rejection and had no seam a test could reach.
  useEffect(() => attachLibraryListener(), []);

  useEffect(() => attachImportListener(), []);

  function openReader(document: ReaderDocument) {
    setReaderDocument(document);
    setRoute(ROUTES.reader);
  }

  async function handlePasteImported(documentId: string, title: string) {
    await refreshLibrary();
    openReader({ id: documentId, title });
  }

  async function handleEpubImported(documentId: string, title: string) {
    await refreshLibrary();
    openReader({ id: documentId, title });
  }

  async function handlePdfImported(documentId: string, title: string) {
    await refreshLibrary();
    openReader({ id: documentId, title });
  }

  async function handleUrlImported(documentId: string, title: string) {
    await refreshLibrary();
    openReader({ id: documentId, title });
  }

  return (
    <div className="flex min-h-screen bg-stone-50 text-neutral-950 dark:bg-neutral-950 dark:text-neutral-100">
      <Sidebar
        activeRoute={route.id}
        onNavigate={(id) => setRoute(ROUTES[id])}
      />

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex min-h-16 items-center justify-between border-b border-neutral-200 bg-white px-4 dark:border-neutral-800 dark:bg-neutral-950 md:px-5">
          <div className="flex min-w-0 items-center gap-3">
            <div className="grid size-9 shrink-0 place-items-center rounded-md border border-neutral-200 bg-stone-100 text-neutral-700 dark:border-neutral-800 dark:bg-neutral-900 dark:text-neutral-300">
              <PanelLeft className="size-4" aria-hidden="true" />
            </div>
            <div className="min-w-0">
              <p className="truncate text-sm text-neutral-500 dark:text-neutral-400">
                LibreTexts Reader
              </p>
              <p className="truncate text-base font-semibold">{route.label}</p>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <button
              className="grid size-9 place-items-center rounded-md border border-neutral-200 text-neutral-600 hover:bg-stone-100 focus:outline-none focus:ring-2 focus:ring-brand-500 dark:border-neutral-800 dark:text-neutral-300 dark:hover:bg-neutral-900"
              aria-label="Search library"
              onClick={() => setRoute(ROUTES.library)}
              title="Search"
              type="button"
            >
              <Search className="size-4" aria-hidden="true" />
            </button>
            <button
              className="grid size-9 place-items-center rounded-md border border-neutral-200 text-neutral-600 hover:bg-stone-100 focus:outline-none focus:ring-2 focus:ring-brand-500 dark:border-neutral-800 dark:text-neutral-300 dark:hover:bg-neutral-900"
              aria-label="Settings"
              onClick={() => setRoute(ROUTES.settings)}
              title="Settings"
              type="button"
            >
              <SlidersHorizontal className="size-4" aria-hidden="true" />
            </button>
          </div>
        </header>

        <main className="min-h-0 flex-1 overflow-auto px-4 py-6 md:px-6">
          <RoutePlaceholder
            route={route}
            icon={mainIcon}
            onOpenDocumentDeleted={() => {
          // Clearing this is what stops the Reader re-fetching a deleted book.
          // `Reader` reloads whenever `documentId` and the player's document
          // disagree, and resetting the player makes them disagree -- so
          // without this, the reset itself is what arms the doomed fetch for
          // the next visit to the Reader, which the sidebar always offers.
          setReaderDocument(null);
        }}
        readerDocument={readerDocument}
            onOpenDocument={openReader}
            onEpubImported={(documentId, title) =>
              void handleEpubImported(documentId, title)
            }
            onPasteImported={(documentId, title) =>
              void handlePasteImported(documentId, title)
            }
            onPdfImported={(documentId, title) =>
              void handlePdfImported(documentId, title)
            }
            onUrlImported={(documentId, title) =>
              void handleUrlImported(documentId, title)
            }
          />
        </main>

        <ImportStatus
          onOpen={(documentId, title) => {
            void refreshLibrary();
            openReader({ id: documentId, title });
          }}
        />
        <MiniPlayer onClose={resetPlayer} />
      </div>
    </div>
  );
}

function RoutePlaceholder({
  route,
  icon,
  readerDocument,
  onOpenDocument,
  onOpenDocumentDeleted,
  onEpubImported,
  onPasteImported,
  onPdfImported,
  onUrlImported,
}: {
  route: Route;
  icon: React.ReactNode;
  readerDocument: ReaderDocument | null;
  onOpenDocument: (document: ReaderDocument) => void;
  onOpenDocumentDeleted: () => void;
  onEpubImported: (documentId: string, title: string) => void;
  onPasteImported: (documentId: string, title: string) => void;
  onPdfImported: (documentId: string, title: string) => void;
  onUrlImported: (documentId: string, title: string) => void;
}) {
  const subtitle =
    route.id === "reader" && readerDocument
      ? readerDocument.title
      : routeSubtitle(route.id);

  return (
    <section className="mx-auto flex max-w-5xl flex-col gap-6">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div className="flex min-w-0 items-center gap-3">
          <div className="grid size-11 shrink-0 place-items-center rounded-md bg-white text-brand-700 shadow-sm ring-1 ring-neutral-200 dark:bg-neutral-900 dark:text-brand-500 dark:ring-neutral-800">
            {icon}
          </div>
          <div className="min-w-0">
            <h1 className="truncate text-2xl font-semibold">{route.label}</h1>
            <p className="mt-1 text-sm text-neutral-600 dark:text-neutral-400">
              {subtitle}
            </p>
          </div>
        </div>
      </div>

      {route.id === "epub" ? <EpubDialog onImported={onEpubImported} /> : null}
      {route.id === "openstax" ? (
        <OpenStaxBrowser onOpenDocument={onOpenDocument} />
      ) : null}
      {route.id === "pressbooks" ? (
        <PressbooksBrowser onOpenDocument={onOpenDocument} />
      ) : null}

      {route.id === "libretexts" ? (
        <LibreTextsBrowser onOpenDocument={onOpenDocument} />
      ) : null}
      {route.id === "pdf" ? <PdfDialog onImported={onPdfImported} /> : null}
      {route.id === "paste" ? (
        <PasteDialog onImported={onPasteImported} />
      ) : null}
      {route.id === "url" ? <UrlDialog onImported={onUrlImported} /> : null}

      {route.id === "library" ? (
        <LibraryGrid
          onOpenDocument={onOpenDocument}
          onOpenDocumentDeleted={onOpenDocumentDeleted}
        />
      ) : null}
      {route.id === "reader" ? (
        <Reader documentId={readerDocument?.id ?? null} />
      ) : null}
      {route.id === "settings" ? <SettingsPanel /> : null}
    </section>
  );
}

function routeIcon(route: RouteId) {
  const className = "size-5";
  switch (route) {
    case "library":
      return <ListMusic className={className} aria-hidden="true" />;
    case "openstax":
    case "libretexts":
    case "pressbooks":
      return <BookOpen className={className} aria-hidden="true" />;
    case "epub":
    case "pdf":
    case "paste":
    case "url":
      return <FileText className={className} aria-hidden="true" />;
    case "reader":
      return <BookOpen className={className} aria-hidden="true" />;
    case "settings":
      return <SlidersHorizontal className={className} aria-hidden="true" />;
  }
}

function routeSubtitle(route: RouteId): string {
  switch (route) {
    case "library":
      return "Books you have imported.";
    case "openstax":
      return "Browse and import OpenStax textbooks.";
    case "libretexts":
      return "Browse and import LibreTexts textbooks.";
    case "pressbooks":
      return "Browse and import books from Pressbooks networks.";
    case "epub":
      return "Import a book from an EPUB file.";
    case "pdf":
      return "Import a book from a PDF file.";
    case "paste":
      return "Paste text to listen to it.";
    case "url":
      return "Import an article from a web address.";
    case "reader":
      return "Open a book from your library to start reading.";
    case "settings":
      return "Voices, appearance, and export.";
  }
}
