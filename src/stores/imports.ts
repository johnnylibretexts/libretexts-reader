import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import type * as Domain from "../types/domain";
import { displayError } from "../lib/errors";
import { api, isTauriRuntime } from "../lib/tauri";

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
  /**
   * Abandon the import in flight and let the reader start another.
   *
   * Releases the guard on the click rather than when the backend agrees.
   * Cancellation in Rust is cooperative -- it takes effect at the next page
   * boundary -- and an import that never settles at all used to lock Add
   * across every catalog for the rest of the session with nothing to press.
   */
  cancel: () => Promise<void>;
  dismissCompleted: () => void;
  clearError: () => void;
}

/**
 * Which import the store is still listening to.
 *
 * Bumped on every start and on every cancel, so a run whose result arrives
 * after the reader gave up on it can be recognised and ignored -- the same
 * device the player and library stores use for out-of-order responses.
 */
let activeRun = 0;

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

    const runId = ++activeRun;

    try {
      const documentId = await run();
      if (runId !== activeRun) {
        // Cancelled, but the fetch was already past its last check point. The
        // book the reader gave up on must not appear on the shelf, and the
        // whole document lands in one transaction -- so removing it is the
        // whole of the cleanup.
        await api.deleteDocument(documentId).catch(() => undefined);
        return;
      }
      set({ active: null, completed: { documentId, title }, error: null });
    } catch (error) {
      // A cancelled import fails, because the cancel is what failed it.
      // Reporting that would dress the reader's own click up as a fault.
      if (runId !== activeRun) {
        return;
      }
      set({ active: null, error: displayError(error), completed: null });
    }
  },

  cancel: async () => {
    if (!get().active) {
      return;
    }

    // Orphan the run first: everything below can suspend, and the result must
    // already be ignorable by the time it does.
    activeRun += 1;
    set({ active: null, completed: null, error: null });

    try {
      await api.cancelImport();
    } catch (error) {
      // The guard is released either way. But the fetch is still running, and
      // a strip that just cleared would be claiming otherwise.
      set({
        error: `The import may still be running -- it could not be told to stop. (${displayError(error)})`,
      });
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

  // `listen` rejects wherever there is no Tauri event bus -- every jsdom test,
  // any `vite` browser preview. That is a missing runtime, not a failure, and
  // reporting it would put an error in front of readers who have no problem.
  // The same guard is why `SettingsPanel` checks before subscribing.
  if (!isTauriRuntime()) {
    return () => undefined;
  }

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
    .catch((failure) => {
      // Nothing retries this and nothing else will notice. Swallowed, the
      // progress strip stays dead for the whole session, and an import that is
      // running looks like one that never started -- so the message names the
      // only thing that fixes it.
      useImportsStore.setState({
        error: `Import progress updates are unavailable for this session; restart the app to restore them. (${displayError(failure)})`,
      });
    });

  return () => {
    disposed = true;
    unlisten?.();
  };
}
