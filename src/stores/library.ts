import { create } from "zustand";
import { api } from "../lib/tauri";
import type * as Domain from "../types/domain";

interface LibraryState {
  documents: Domain.Document[];
  loading: boolean;
  error: string | null;
  refresh: () => Promise<void>;
  remove: (id: string) => Promise<void>;
  search: (query: string) => Promise<void>;
}

export const useLibraryStore = create<LibraryState>((set) => ({
  documents: [],
  loading: false,
  error: null,
  refresh: async () => {
    set({ loading: true, error: null });
    try {
      const documents = await api.listDocuments();
      set({ documents, loading: false });
    } catch (error) {
      set({
        documents: [],
        loading: false,
        error: error instanceof Error ? error.message : String(error),
      });
    }
  },
  remove: async (id: string) => {
    await api.deleteDocument(id);
    const documents = await api.listDocuments();
    set({ documents });
  },
  search: async (query: string) => {
    set({ loading: true, error: null });
    try {
      const documents = query.trim()
        ? await api.searchDocuments(query)
        : await api.listDocuments();
      set({ documents, loading: false });
    } catch (error) {
      set({
        loading: false,
        error: error instanceof Error ? error.message : String(error),
      });
    }
  },
}));
