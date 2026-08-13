import { create } from "zustand";
import { api } from "../lib/tauri";
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
