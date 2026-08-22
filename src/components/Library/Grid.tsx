import { useEffect, useMemo, useState, type MouseEvent } from "react";
import { BookOpen, Search, Trash2 } from "lucide-react";
import { useLibraryStore } from "../../stores/library";
import { usePlayerStore } from "../../stores/player";
import { ConfirmDialog } from "../ConfirmDialog";
import type * as Domain from "../../types/domain";
import { DocumentCard } from "./DocumentCard";
import { EmptyState } from "./EmptyState";

interface LibraryGridProps {
  onOpenDocument: (document: { id: string; title: string }) => void;
  /**
   * The book open in the Reader has just been deleted, so whatever else is
   * still pointing at it has to let go. Resetting the player is not enough on
   * its own: the route that opened it lives in AppShell, and the Reader nav
   * entry is always present -- so going back to it re-mounts the Reader with
   * the dead id and it immediately re-fetches rows the backend has removed.
   */
  onOpenDocumentDeleted?: () => void;
}

interface ContextMenuState {
  document: Domain.Document;
  x: number;
  y: number;
}

// Export was listed here permanently disabled, which offers the reader an
// action the app cannot perform. Chapter audio export lives in the Reader,
// where it belongs; the item is gone rather than greyed out.
const contextMenuItems = [
  { id: "open", label: "Open", icon: BookOpen },
  { id: "delete", label: "Delete", icon: Trash2 },
] as const;

/**
 * Whether re-importing this document means downloading it again.
 *
 * The dialog below promised every reader that a re-import re-downloads the
 * whole book. For an EPUB, a PDF, or pasted text there is nothing to download
 * -- the source was a local file they still have -- so the warning named a
 * cost that does not exist, in the one place where the copy IS the feature.
 */
function isImportedFromSource(sourceType: Domain.Document["sourceType"]) {
  // "url" is the article importer; "pasted", "epub" and "pdf" all came from
  // something the reader already has.
  return (
    sourceType === "openstax" ||
    sourceType === "libretexts" ||
    sourceType === "pressbooks" ||
    sourceType === "url"
  );
}

export function LibraryGrid({
  onOpenDocument,
  onOpenDocumentDeleted,
}: LibraryGridProps) {
  const documents = useLibraryStore((state) => state.documents);
  const loading = useLibraryStore((state) => state.loading);
  const error = useLibraryStore((state) => state.error);
  const remove = useLibraryStore((state) => state.remove);
  const [query, setQuery] = useState("");
  const [menu, setMenu] = useState<ContextMenuState | null>(null);
  // The delete the reader has asked for but not yet agreed to. Holding the
  // whole document rather than an id so the dialog can name the book: a
  // confirmation that does not say what it is about to destroy is not one.
  const [pendingDelete, setPendingDelete] = useState<Domain.Document | null>(
    null,
  );

  useEffect(() => {
    function closeMenu() {
      setMenu(null);
    }
    function closeOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setMenu(null);
      }
    }

    window.addEventListener("click", closeMenu);
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      window.removeEventListener("click", closeMenu);
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, []);

  const filteredDocuments = useMemo(() => {
    const normalizedQuery = query.trim().toLowerCase();
    if (!normalizedQuery) {
      return documents;
    }
    return documents.filter((document) =>
      document.title.toLowerCase().includes(normalizedQuery),
    );
  }, [documents, query]);

  function openDocument(document: Domain.Document) {
    setMenu(null);
    onOpenDocument({ id: document.id, title: document.title });
  }

  function requestDelete(document: Domain.Document) {
    setMenu(null);
    setPendingDelete(document);
  }

  async function confirmDelete() {
    const document = pendingDelete;
    if (!document) {
      return;
    }
    setPendingDelete(null);

    // Read before the delete, not after: `remove` drops the row locally as
    // part of the same transition, so asking afterwards races it.
    const isOpenInReader =
      usePlayerStore.getState().document?.id === document.id;

    await remove(document.id);

    // `remove` reports a failure by writing the store's `error` field, not by
    // rejecting, so this await resolves either way. The row disappearing is
    // the actual signal that the delete landed -- `remove` filters it out only
    // on success -- and reading the store's `error` instead would be
    // unreliable anyway, since a concurrent action can overwrite it.
    const stillInLibrary = useLibraryStore
      .getState()
      .documents.some((row) => row.id === document.id);

    // The Reader and MiniPlayer stay bound to whatever the player store holds.
    // Left pointing at a deleted document, the next section change runs
    // against rows the backend has already removed from disk. Only on a delete
    // that actually happened, though: closing the book someone is reading
    // because a failed delete resolved is a worse outcome than the bug.
    if (isOpenInReader && !stillInLibrary) {
      usePlayerStore.getState().reset();
      onOpenDocumentDeleted?.();
    }
  }

  function openContextMenu(
    event: MouseEvent<HTMLElement>,
    document: Domain.Document,
  ) {
    event.preventDefault();
    event.stopPropagation();
    setMenu({
      document,
      x: event.clientX,
      y: event.clientY,
    });
  }

  return (
    <section className="flex flex-col gap-4">
      <div className="rounded-md border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900">
        <label className="flex max-w-md flex-col gap-2 text-sm font-medium">
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
      </div>

      {error ? (
        <p className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700 dark:border-red-950 dark:bg-red-950/30 dark:text-red-300">
          {error}
        </p>
      ) : null}

      {loading ? (
        <p className="text-sm text-neutral-500 dark:text-neutral-400">
          Loading...
        </p>
      ) : null}

      {!loading && documents.length === 0 ? <EmptyState /> : null}

      {!loading &&
      documents.length > 0 &&
      filteredDocuments.length === 0 ? (
        <p className="text-sm text-neutral-500 dark:text-neutral-400">
          No documents match "{query.trim()}".
        </p>
      ) : null}

      {filteredDocuments.length > 0 ? (
        <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
          {filteredDocuments.map((document) => (
            <DocumentCard
              document={document}
              key={document.id}
              onContextMenu={openContextMenu}
              onDelete={requestDelete}
              onOpen={openDocument}
            />
          ))}
        </div>
      ) : null}

      {menu ? (
        <div
          className="fixed z-50 w-44 overflow-hidden rounded-md border border-neutral-200 bg-white py-1 text-sm shadow-lg dark:border-neutral-800 dark:bg-neutral-900"
          role="menu"
          style={{ left: menu.x, top: menu.y }}
        >
          {contextMenuItems.map((item) => {
            const Icon = item.icon;
            return (
              <button
                className="flex h-9 w-full items-center gap-2 px-3 text-left text-neutral-700 hover:bg-stone-100 dark:text-neutral-200 dark:hover:bg-neutral-800"
                key={item.id}
                onClick={() => {
                  if (item.id === "open") {
                    openDocument(menu.document);
                  }
                  if (item.id === "delete") {
                    requestDelete(menu.document);
                  }
                }}
                role="menuitem"
                type="button"
              >
                <Icon className="size-4" aria-hidden="true" />
                {item.label}
              </button>
            );
          })}
        </div>
      ) : null}

      <ConfirmDialog
        body={
          <>
            <p>
              <strong>{pendingDelete?.title}</strong>, its cover and every
              figure downloaded with it will be deleted from this Mac. This
              cannot be undone.
            </p>
            <p className="mt-2">
              {pendingDelete && isImportedFromSource(pendingDelete.sourceType)
                ? "You can import it again, but the whole book has to download afresh."
                : "You can import it again from the original file."}
            </p>
          </>
        }
        confirmLabel="Delete"
        onCancel={() => setPendingDelete(null)}
        onConfirm={() => void confirmDelete()}
        open={pendingDelete !== null}
        title="Delete this book?"
      />
    </section>
  );
}
