import { beforeEach, describe, expect, it, vi } from "vitest";

const listen = vi.fn();
const isTauriRuntime = vi.fn(() => true);
const listDocuments = vi.fn();

vi.mock("@tauri-apps/api/event", () => ({
  listen: (...args: unknown[]) => listen(...args),
}));

vi.mock("../lib/tauri", () => ({
  isTauriRuntime: () => isTauriRuntime(),
  api: {
    get listDocuments() {
      return listDocuments;
    },
  },
}));

const { useLibraryStore, attachLibraryListener } = await import("./library");

beforeEach(() => {
  vi.clearAllMocks();
  isTauriRuntime.mockReturnValue(true);
  listen.mockResolvedValue(() => undefined);
  listDocuments.mockResolvedValue([]);
  useLibraryStore.setState({ documents: [], loading: false, error: null });
});

describe("library-changed subscription", () => {
  it("reports a subscription that failed instead of leaving the library stale", async () => {
    // Nothing retries this and nothing else refreshes the Library while the
    // app is open, so an import that finishes leaves a shelf that never shows
    // the book -- looking like the import silently failed.
    listen.mockRejectedValue(new Error("event bus unavailable"));

    attachLibraryListener();

    await vi.waitFor(() =>
      expect(useLibraryStore.getState().error).toMatch(/refresh/i),
    );
    expect(useLibraryStore.getState().error).toMatch(/restart/i);
  });

  it("says nothing when there is no desktop event bus to subscribe to", async () => {
    // Outside Tauri `listen` always rejects. That is a missing runtime, not a
    // failure, and it must not put an error in front of anyone.
    isTauriRuntime.mockReturnValue(false);

    attachLibraryListener();

    await Promise.resolve();
    expect(listen).not.toHaveBeenCalled();
    expect(useLibraryStore.getState().error).toBeNull();
  });

  it("refreshes the library when the event arrives", async () => {
    // The subscription exists to do this; a test that only covered its failure
    // would pass just as well against one that never listened.
    let deliver: (() => void) | undefined;
    listen.mockImplementation(async (_name: string, handler: () => void) => {
      deliver = handler;
      return () => undefined;
    });

    attachLibraryListener();
    await vi.waitFor(() => expect(deliver).toBeDefined());
    deliver?.();

    await vi.waitFor(() => expect(listDocuments).toHaveBeenCalled());
  });

  it("stops refreshing once disposed", async () => {
    // `AppShell` disposes on unmount. A handler left attached would refresh
    // into a store the next mount is already refreshing, racing its results.
    const dispose = vi.fn();
    listen.mockResolvedValue(dispose);

    const stop = attachLibraryListener();
    await vi.waitFor(() => expect(listen).toHaveBeenCalled());
    stop();

    expect(dispose).toHaveBeenCalled();
  });
});
