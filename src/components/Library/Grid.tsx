import { useEffect, useMemo, useState, type MouseEvent } from "react";
import { BookOpen, ExternalLink, Search, Trash2 } from "lucide-react";
import { useLibraryStore } from "../../stores/library";
import type * as Domain from "../../types/domain";
import { DocumentCard } from "./DocumentCard";
import { EmptyState } from "./EmptyState";

interface LibraryGridProps {
  onOpenDocument: (document: { id: string; title: string }) => void;
}

interface ContextMenuState {
  document: Domain.Document;
  x: number;
  y: number;
}

const contextMenuItems = [
  { id: "open", label: "Open", icon: BookOpen },
  { id: "export", label: "Export", icon: ExternalLink },
  { id: "delete", label: "Delete", icon: Trash2 },
] as const;

export function LibraryGrid({ onOpenDocument }: LibraryGridProps) {
  const documents = useLibraryStore((state) => state.documents);
  const loading = useLibraryStore((state) => state.loading);
  const error = useLibraryStore((state) => state.error);
  const remove = useLibraryStore((state) => state.remove);
  const [query, setQuery] = useState("");
  const [menu, setMenu] = useState<ContextMenuState | null>(null);

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

  async function deleteDocument(document: Domain.Document) {
    setMenu(null);
    await remove(document.id);
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
              onDelete={(document) => void deleteDocument(document)}
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
            const disabled = item.id === "export";
            return (
              <button
                className="flex h-9 w-full items-center gap-2 px-3 text-left text-neutral-700 hover:bg-stone-100 disabled:cursor-not-allowed disabled:opacity-50 dark:text-neutral-200 dark:hover:bg-neutral-800"
                disabled={disabled}
                key={item.id}
                onClick={() => {
                  if (item.id === "open") {
                    openDocument(menu.document);
                  }
                  if (item.id === "delete") {
                    void deleteDocument(menu.document);
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
    </section>
  );
}
