import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import type * as Domain from "../types/domain";
import { displayError } from "../lib/errors";

export interface ActiveImport {
  bookId: string;
  title: string;
  stage: Domain.ImportStage;
  current: number;
  total: number;
}

export interface CompletedImport {
  documentId: string;
  title: string;
}

interface ImportsState {
  active: ActiveImport | null;
  completed: CompletedImport | null;
  error: string | null;
  start: (input: {
    bookId: string;
    title: string;
    run: () => Promise<string>;
  }) => Promise<void>;
  applyProgress: (payload: Domain.ImportProgress) => void;
  dismissCompleted: () => void;
  clearError: () => void;
}

export const useImportsStore = create<ImportsState>((set, get) => ({
  active: null,
  completed: null,
  error: null,

  // The guard lives here rather than in a component: this store outlives every
  // route, so unmounting the catalog cannot reset it and let a duplicate start.
  start: async ({ bookId, title, run }) => {
    if (get().active) {
      set({ error: "An import is already running." });
      return;
    }

    set({
      active: { bookId, title, stage: "fetching", current: 0, total: 0 },
      completed: null,
      error: null,
    });

    try {
      const documentId = await run();
      set({ active: null, completed: { documentId, title }, error: null });
    } catch (error) {
      set({ active: null, error: displayError(error), completed: null });
    }
  },

  // Rust keys fetch progress on the book id but keys the final "complete" event
  // on the freshly minted document id, so that last event never matches and is
  // ignored. Completion is observed by `run()` resolving instead.
  applyProgress: (payload) => {
    const active = get().active;
    if (!active || payload.documentId !== active.bookId) {
      return;
    }
    set({
      active: {
        ...active,
        stage: payload.stage,
        current: payload.current,
        total: payload.total,
      },
    });
  },

  dismissCompleted: () => set({ completed: null }),
  clearError: () => set({ error: null }),
}));

/**
 * Subscribe to Rust's import-progress events. Call once, at app level — never
 * from a route-scoped component, or the subscription dies on navigation.
 * Returns a disposer for app teardown.
 */
export function attachImportListener(): () => void {
  let unlisten: (() => void) | undefined;
  let disposed = false;

  void listen<Domain.ImportProgress>("import-progress", (event) => {
    useImportsStore.getState().applyProgress(event.payload);
  })
    .then((dispose) => {
      if (disposed) {
        dispose();
        return;
      }
      unlisten = dispose;
    })
    .catch(() => undefined);

  return () => {
    disposed = true;
    unlisten?.();
  };
}
