import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
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
import { OpenStaxBrowser } from "./OpenStaxBrowser/OpenStaxBrowser";
import { Reader } from "./Reader/Reader";
import { SettingsPanel } from "./Settings/SettingsPanel";
import { attachImportListener } from "../stores/imports";
import { useLibraryStore } from "../stores/library";
import { usePlayerStore } from "../stores/player";

export type RouteId =
  | "library"
  | "openstax"
  | "libretexts"
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

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void listen("library-changed", () => {
      void refreshLibrary();
    })
      .then((dispose) => {
        unlisten = dispose;
      })
      .catch(() => {});

    return () => {
      unlisten?.();
    };
  }, [refreshLibrary]);

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
  onEpubImported,
  onPasteImported,
  onPdfImported,
  onUrlImported,
}: {
  route: Route;
  icon: React.ReactNode;
  readerDocument: ReaderDocument | null;
  onOpenDocument: (document: ReaderDocument) => void;
  onEpubImported: (documentId: string, title: string) => void;
  onPasteImported: (documentId: string, title: string) => void;
  onPdfImported: (documentId: string, title: string) => void;
  onUrlImported: (documentId: string, title: string) => void;
}) {
  const statusRows = routeStatusRows(route.id, readerDocument);
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
      {route.id === "libretexts" ? (
        <LibreTextsBrowser onOpenDocument={onOpenDocument} />
      ) : null}
      {route.id === "pdf" ? <PdfDialog onImported={onPdfImported} /> : null}
      {route.id === "paste" ? (
        <PasteDialog onImported={onPasteImported} />
      ) : null}
      {route.id === "url" ? <UrlDialog onImported={onUrlImported} /> : null}

      {route.id === "library" ? (
        <LibraryGrid onOpenDocument={onOpenDocument} />
      ) : null}
      {route.id === "reader" ? (
        <Reader documentId={readerDocument?.id ?? null} />
      ) : null}
      {route.id === "settings" ? <SettingsPanel /> : null}

      {route.id !== "library" &&
      route.id !== "settings" &&
      route.id !== "reader" ? (
        <StatusTable rows={statusRows} />
      ) : null}
    </section>
  );
}

function StatusTable({
  rows,
}: {
  rows: Array<{ area: string; state: string; detail: string }>;
}) {
  return (
    <div className="overflow-x-auto rounded-md border border-neutral-200 bg-white dark:border-neutral-800 dark:bg-neutral-900">
      <div className="grid min-w-[34rem] grid-cols-[minmax(8rem,1fr)_minmax(8rem,1fr)_minmax(9rem,1.2fr)] border-b border-neutral-200 bg-stone-100 text-xs font-semibold uppercase text-neutral-500 dark:border-neutral-800 dark:bg-neutral-900 dark:text-neutral-400">
        <div className="px-4 py-3">Area</div>
        <div className="px-4 py-3">State</div>
        <div className="px-4 py-3">Detail</div>
      </div>
      {rows.map((row) => (
        <div
          className="grid min-w-[34rem] grid-cols-[minmax(8rem,1fr)_minmax(8rem,1fr)_minmax(9rem,1.2fr)] border-b border-neutral-100 text-sm last:border-b-0 dark:border-neutral-800"
          key={row.area}
        >
          <div className="min-w-0 px-4 py-3 font-medium">{row.area}</div>
          <div className="min-w-0 px-4 py-3 text-neutral-600 dark:text-neutral-300">
            {row.state}
          </div>
          <div className="min-w-0 px-4 py-3 text-neutral-600 dark:text-neutral-300">
            {row.detail}
          </div>
        </div>
      ))}
    </div>
  );
}

function routeIcon(route: RouteId) {
  const className = "size-5";
  switch (route) {
    case "library":
      return <ListMusic className={className} aria-hidden="true" />;
    case "openstax":
    case "libretexts":
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
      return "No documents selected.";
    case "openstax":
      return "Catalog view.";
    case "libretexts":
      return "LibreTexts catalog.";
    case "epub":
      return "File import.";
    case "pdf":
      return "File import.";
    case "paste":
      return "Scratchpad.";
    case "url":
      return "Article source.";
    case "reader":
      return "No active document.";
    case "settings":
      return "Preferences.";
  }
}

function routeStatusRows(
  route: RouteId,
  readerDocument: ReaderDocument | null,
) {
  const shared = [
    { area: "Storage", state: "Ready", detail: "Local SQLite" },
    { area: "Bridge", state: "Connected", detail: "Tauri IPC" },
  ];

  switch (route) {
    case "library":
      return [
        { area: "Documents", state: "Empty", detail: "Library" },
        { area: "Imports", state: "Available", detail: "Sidebar" },
        ...shared,
      ];
    case "reader":
      return [
        {
          area: "Reader",
          state: readerDocument ? "Loaded" : "Empty",
          detail: readerDocument ? readerDocument.id : "No document loaded",
        },
        { area: "Player", state: "Idle", detail: "Mini-player hidden" },
        ...shared,
      ];
    case "settings":
      return [
        { area: "Theme", state: "Persisted", detail: "Light, dark, system" },
        { area: "Defaults", state: "Seeded", detail: "Voice, speed, export" },
        ...shared,
      ];
    default:
      return [
        { area: "Importer", state: "Ready", detail: "Awaiting source" },
        { area: "Tokenizer", state: "Ready", detail: "Sentence offsets" },
        ...shared,
      ];
  }
}
