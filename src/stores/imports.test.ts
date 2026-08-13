import { beforeEach, describe, expect, it } from "vitest";
import type * as Domain from "../types/domain";
import { useImportsStore } from "./imports";

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
  useImportsStore.setState({ active: null, completed: null, error: null });
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
