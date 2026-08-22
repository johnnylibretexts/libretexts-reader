import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import { api, isTauriRuntime } from "../lib/tauri";
import type * as Domain from "../types/domain";
import { displayError } from "../lib/errors";

interface LibraryState {
  documents: Domain.Document[];
  loading: boolean;
  error: string | null;
  refresh: () => Promise<void>;
  remove: (id: string) => Promise<void>;
  search: (query: string) => Promise<void>;
}

// Monotonic token so an earlier in-flight search/refresh cannot overwrite the
// results of a newer one when responses resolve out of order.
let activeListRequest = 0;

export const useLibraryStore = create<LibraryState>((set) => ({
  documents: [],
  loading: false,
  error: null,
  refresh: async () => {
    const requestId = ++activeListRequest;
    set({ loading: true, error: null });
    try {
      const documents = await api.listDocuments();
      if (requestId !== activeListRequest) {
        return;
      }
      set({ documents, loading: false });
    } catch (error) {
      if (requestId !== activeListRequest) {
        return;
      }
      set({
        documents: [],
        loading: false,
        error: displayError(error),
      });
    }
  },
  remove: async (id: string) => {
    set({ loading: true, error: null });
    try {
      await api.deleteDocument(id);
      // Invalidate any list/search started before this delete committed, so a
      // late response cannot reinsert the document we just removed.
      activeListRequest += 1;
      // Treat the deletion as a single state transition: once the backend
      // delete succeeds, drop the row locally so a failing refresh cannot
      // leave the just-deleted document visible.
      set((state) => ({
        documents: state.documents.filter((document) => document.id !== id),
        loading: false,
      }));
    } catch (error) {
      set({
        loading: false,
        error: displayError(error),
      });
    }
  },
  search: async (query: string) => {
    const requestId = ++activeListRequest;
    set({ loading: true, error: null });
    try {
      const documents = query.trim()
        ? await api.searchDocuments(query)
        : await api.listDocuments();
      if (requestId !== activeListRequest) {
        return;
      }
      set({ documents, loading: false });
    } catch (error) {
      if (requestId !== activeListRequest) {
        return;
      }
      set({
        loading: false,
        error: displayError(error),
      });
    }
  },
}));

/**
 * Keep the Library in step with imports and deletions happening elsewhere.
 *
 * Lives here rather than in `AppShell` for the same reason
 * `attachImportListener` lives beside the imports store: the subscription
 * exists to write this store, and as a component effect it had no seam a test
 * could reach. Returns a disposer for unmount.
 */
export function attachLibraryListener(): () => void {
  let unlisten: (() => void) | undefined;
  let disposed = false;

  // `listen` rejects wherever there is no Tauri event bus -- every jsdom test,
  // any `vite` browser preview. A missing runtime is not a failure.
  if (!isTauriRuntime()) {
    return () => undefined;
  }

  void listen("library-changed", () => {
    void useLibraryStore.getState().refresh();
  })
    .then((dispose) => {
      if (disposed) {
        dispose();
        return;
      }
      unlisten = dispose;
    })
    .catch((failure) => {
      // Nothing else refreshes the Library while the app is open, so a
      // swallowed failure here means a finished import never appears on the
      // shelf -- indistinguishable from an import that silently did nothing.
      useLibraryStore.setState({
        error: `The library will not refresh on its own this session; restart the app to restore it. (${displayError(failure)})`,
      });
    });

  return () => {
    disposed = true;
    unlisten?.();
  };
}
