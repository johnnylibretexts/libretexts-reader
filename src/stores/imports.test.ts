import { beforeEach, describe, expect, it, vi } from "vitest";
import type * as Domain from "../types/domain";

const listen = vi.fn();
const isTauriRuntime = vi.fn(() => true);

vi.mock("@tauri-apps/api/event", () => ({
  listen: (...args: unknown[]) => listen(...args),
}));

const cancelImport = vi.fn(async () => undefined);
const deleteDocument = vi.fn(async (_id: string) => undefined);

vi.mock("../lib/tauri", () => ({
  isTauriRuntime: () => isTauriRuntime(),
  api: {
    cancelImport: () => cancelImport(),
    deleteDocument: (id: string) => deleteDocument(id),
  },
}));

const { useImportsStore, attachImportListener } = await import("./imports");

function progress(
  documentId: string,
  overrides: Partial<Domain.ImportProgress> = {},
): Domain.ImportProgress {
  return {
    documentId,
    stage: "fetching",
    current: 3,
    total: 10,
    message: null,
    ...overrides,
  };
}

/** A promise plus the handles to settle it, so tests control when an import ends. */
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

beforeEach(() => {
  vi.clearAllMocks();
  isTauriRuntime.mockReturnValue(true);
  listen.mockResolvedValue(() => undefined);
  useImportsStore.setState({ active: null, completed: null, error: null });
});

describe("import progress subscription", () => {
  it("reports a subscription that failed instead of leaving the strip dead", async () => {
    // Nothing retries this. Swallowed, the progress strip is dead for the
    // whole session and an import that is running looks like one that never
    // started -- so the message has to say the session is what needs ending.
    listen.mockRejectedValue(new Error("event bus unavailable"));

    attachImportListener();

    await vi.waitFor(() =>
      expect(useImportsStore.getState().error).toMatch(/progress/i),
    );
    expect(useImportsStore.getState().error).toMatch(/restart/i);
  });

  it("says nothing when there is no desktop event bus to subscribe to", async () => {
    // `listen` rejects outside Tauri -- in every jsdom test and any browser
    // preview. That is the absence of a runtime, not a failure, and reporting
    // it would put an error in front of readers who have no problem.
    isTauriRuntime.mockReturnValue(false);

    attachImportListener();

    await Promise.resolve();
    expect(listen).not.toHaveBeenCalled();
    expect(useImportsStore.getState().error).toBeNull();
  });
});

describe("imports store", () => {
  it("rejects a second import while one is active and leaves the first untouched", async () => {
    const first = deferred<string>();
    let secondRan = false;

    void useImportsStore.getState().start({
      bookId: "bio-1764",
      title: "General Biology",
      run: () => first.promise,
    });

    await useImportsStore.getState().start({
      bookId: "chem-999",
      title: "Chemistry",
      run: async () => {
        secondRan = true;
        return "doc-2";
      },
    });

    expect(secondRan).toBe(false);
    expect(useImportsStore.getState().active?.bookId).toBe("bio-1764");
    expect(useImportsStore.getState().error).toBe("An import is already running.");

    first.resolve("doc-1");
  });

  it("applies a progress event for the active import", () => {
    useImportsStore.setState({
      active: {
        bookId: "bio-1764",
        title: "General Biology",
        stage: "fetching",
        current: 0,
        total: 0,
      },
    });

    useImportsStore.getState().applyProgress(progress("bio-1764", { current: 214, total: 358 }));

    expect(useImportsStore.getState().active).toMatchObject({
      bookId: "bio-1764",
      current: 214,
      total: 358,
      stage: "fetching",
    });
  });

  it("ignores a progress event whose documentId is not the active import", () => {
    useImportsStore.setState({
      active: {
        bookId: "bio-1764",
        title: "General Biology",
        stage: "fetching",
        current: 5,
        total: 358,
      },
    });

    useImportsStore.getState().applyProgress(progress("some-other-id", { current: 999 }));

    expect(useImportsStore.getState().active?.current).toBe(5);
  });

  it("ignores a progress event when nothing is active", () => {
    useImportsStore.getState().applyProgress(progress("bio-1764"));
    expect(useImportsStore.getState().active).toBeNull();
  });

  it("records completion and clears the active import", async () => {
    await useImportsStore.getState().start({
      bookId: "bio-1764",
      title: "General Biology",
      run: async () => "doc-1",
    });

    expect(useImportsStore.getState().active).toBeNull();
    expect(useImportsStore.getState().completed).toEqual({
      documentId: "doc-1",
      title: "General Biology",
    });
    expect(useImportsStore.getState().error).toBeNull();
  });

  it("records a failure and clears the active import", async () => {
    await useImportsStore.getState().start({
      bookId: "bio-1764",
      title: "General Biology",
      run: async () => {
        throw new Error("network died");
      },
    });

    expect(useImportsStore.getState().active).toBeNull();
    expect(useImportsStore.getState().completed).toBeNull();
    expect(useImportsStore.getState().error).toContain("network died");
  });

  // The four tests below exist to kill specific mutants. `start` clears stale
  // status in three places — on entry, on success, and on failure — and every
  // one of those clears survives deletion unless a test seeds a non-null value
  // first. `beforeEach` resets the store to all-null, so asserting `toBeNull()`
  // after a fresh success asserts against a value that was already null.

  it("clears a stale error the moment a new import starts", () => {
    const pending = deferred<string>();
    useImportsStore.setState({ error: "Import failed: network died" });

    void useImportsStore.getState().start({
      bookId: "bio-1764",
      title: "General Biology",
      run: () => pending.promise,
    });

    // Asserted while the import is still in flight: this pins the clear to the
    // entry `set`, not the one on the success path.
    expect(useImportsStore.getState().active?.bookId).toBe("bio-1764");
    expect(useImportsStore.getState().error).toBeNull();

    pending.resolve("doc-1");
  });

  it("clears a stale completion the moment a new import starts", () => {
    const pending = deferred<string>();
    useImportsStore.setState({ completed: { documentId: "doc-0", title: "Chemistry" } });

    void useImportsStore.getState().start({
      bookId: "bio-1764",
      title: "General Biology",
      run: () => pending.promise,
    });

    expect(useImportsStore.getState().active?.bookId).toBe("bio-1764");
    expect(useImportsStore.getState().completed).toBeNull();

    pending.resolve("doc-1");
  });

  it("clears a stale error when an import succeeds", async () => {
    await useImportsStore.getState().start({
      bookId: "bio-1764",
      title: "General Biology",
      run: async () => {
        // Seeded mid-flight so the entry clear cannot be what satisfies the
        // assertion — only the success path can clear this one.
        useImportsStore.setState({ error: "Import failed: network died" });
        return "doc-1";
      },
    });

    expect(useImportsStore.getState().completed?.documentId).toBe("doc-1");
    expect(useImportsStore.getState().error).toBeNull();
  });

  it("clears a stale completion when an import fails", async () => {
    await useImportsStore.getState().start({
      bookId: "bio-1764",
      title: "General Biology",
      run: async () => {
        useImportsStore.setState({ completed: { documentId: "doc-0", title: "Chemistry" } });
        throw new Error("network died");
      },
    });

    expect(useImportsStore.getState().error).toContain("network died");
    expect(useImportsStore.getState().completed).toBeNull();
  });

  it("allows a new import after the previous one finishes", async () => {
    await useImportsStore.getState().start({
      bookId: "bio-1764",
      title: "General Biology",
      run: async () => "doc-1",
    });

    await useImportsStore.getState().start({
      bookId: "chem-999",
      title: "Chemistry",
      run: async () => "doc-2",
    });

    expect(useImportsStore.getState().completed?.documentId).toBe("doc-2");
  });
});

describe("cancelling an import", () => {
  /** An import in flight, with the handles to settle it late. */
  function startImport() {
    const run = deferred<string>();
    const started = useImportsStore.getState().start({
      bookId: "book-1",
      title: "A Big Textbook",
      run: () => run.promise,
    });
    return { run, started };
  }

  it("releases the guard without waiting for the backend", async () => {
    // The whole complaint: an import that never settles locks Add across every
    // catalog for the rest of the session, with nothing to press. The guard has
    // to come back on the click, not on the backend agreeing.
    const { run } = startImport();
    expect(useImportsStore.getState().active).not.toBeNull();

    await useImportsStore.getState().cancel();

    expect(useImportsStore.getState().active).toBeNull();
    // Deliberately still unsettled -- this is the hung-import case.
    void run;
  });

  it("asks the backend to stop fetching", async () => {
    startImport();

    await useImportsStore.getState().cancel();

    expect(cancelImport).toHaveBeenCalled();
  });

  it("lets the reader start a different book straight away", async () => {
    startImport();
    await useImportsStore.getState().cancel();

    // Not awaited: `start` only resolves when the import does, and this one is
    // deliberately left in flight. The guard is claimed synchronously.
    const second = deferred<string>();
    void useImportsStore.getState().start({
      bookId: "book-2",
      title: "Another Book",
      run: () => second.promise,
    });

    expect(useImportsStore.getState().active?.bookId).toBe("book-2");
    expect(useImportsStore.getState().error).toBeNull();
  });

  it("throws away a cancelled import that finished anyway", async () => {
    // Cancellation is cooperative: the fetch can be past its last check point
    // when the click lands, and persist happens in one transaction at the end.
    // A book the reader cancelled must not appear on the shelf regardless.
    const { run, started } = startImport();
    await useImportsStore.getState().cancel();

    run.resolve("doc-late");
    await started;

    expect(deleteDocument).toHaveBeenCalledWith("doc-late");
    expect(useImportsStore.getState().completed).toBeNull();
    expect(useImportsStore.getState().active).toBeNull();
  });

  it("shows no error when the cancelled import reports its own failure", async () => {
    // The cancel is what failed it. Reporting that back to the reader would
    // dress their own click up as something going wrong.
    const { run, started } = startImport();
    await useImportsStore.getState().cancel();

    run.reject(new Error("import cancelled"));
    await started;

    expect(useImportsStore.getState().error).toBeNull();
  });

  it("says so when the request to stop could not be delivered", async () => {
    // The guard is released either way -- but the fetch is still running, and
    // a strip that simply cleared would be claiming otherwise.
    cancelImport.mockRejectedValueOnce(new Error("the bridge is gone"));
    startImport();

    await useImportsStore.getState().cancel();

    expect(useImportsStore.getState().active).toBeNull();
    expect(useImportsStore.getState().error).toMatch(/still/i);
  });

  it("does nothing when no import is running", async () => {
    await useImportsStore.getState().cancel();

    expect(cancelImport).not.toHaveBeenCalled();
    expect(useImportsStore.getState().error).toBeNull();
  });
});
