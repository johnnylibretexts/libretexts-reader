# Durable Import State Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move import lifecycle state out of route-scoped component state into a store that outlives navigation, so import progress stays visible, a second concurrent import cannot start, and the catalog shows which books are already in the library.

**Architecture:** A new Zustand store owns import state and exposes a pure `applyProgress` action plus an `attachImportListener` IO adapter, so the state logic is testable without mocking Tauri events. A persistent `ImportStatus` strip renders beside `MiniPlayer`, outside the route switch. Both catalog browsers become stateless views over the store. No Rust changes — `commands/content.rs` already emits everything needed.

**Tech Stack:** React 19, Zustand 5, TypeScript (strict via `tsc`), Tailwind v4, vitest + jsdom, Tauri 2 event API.

**Source spec:** `docs/superpowers/specs/2026-08-13-durable-import-state-design.md`

## Global Constraints

- **Node 22.x required** (last verified 22.20.0 / npm 10.9.3). Node 24 hangs on Vite/Rollup native addons. Do not run `nvm` or change Node versions; the environment is already correct.
- **Do not revert the rollup alias** `"rollup": "npm:@rollup/wasm-node@^4.60.2"` in `package.json`.
- **No Rust changes in this plan.** `src-tauri/` is untouched. If a task seems to need a backend change, stop and report — it means the plan is wrong.
- **No new dependencies.** Zustand, lucide-react and the Tauri event API are already present.
- **No React Testing Library.** It is not configured. Components are verified by running the app; only pure logic gets vitest coverage. Do not add RTL.
- **Full gate for every task:**
  ```sh
  npm run build
  npm test
  git diff --check
  ```
- **Exact strings, copied verbatim:**
  - guard message: `An import is already running.`
  - catalog label when a book is present: `In library`
  - LibreTexts identity: `source_metadata.book_id` ↔ `LibreTextsBook.bookId`
  - OpenStax identity: `source_metadata.book_uuid` ↔ `OpenStaxBook.uuid`

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `src/stores/imports.ts` | **new** — import lifecycle state, concurrency guard, progress reducer, listener adapter | 1 |
| `src/stores/imports.test.ts` | **new** — store logic tests | 1 |
| `src/lib/importedBooks.ts` | **new** — pure matcher: is this catalog book already a document? | 2 |
| `src/lib/importedBooks.test.ts` | **new** — matcher tests | 2 |
| `src/components/ImportStatus.tsx` | **new** — persistent progress strip | 3 |
| `src/components/AppShell.tsx` | render the strip; attach the listener; stop auto-navigating for catalog imports | 3 |
| `src/components/LibreTextsBrowser/LibreTextsBrowser.tsx` | drop local import state + listener; use the store; show `In library` | 4 |
| `src/components/OpenStaxBrowser/OpenStaxBrowser.tsx` | same, keyed on `book_uuid` | 5 |

---

### Task 1: The imports store

**Files:**
- Create: `src/stores/imports.ts`
- Test: `src/stores/imports.test.ts`

**Interfaces:**
- Consumes: `displayError` from `src/lib/errors.ts`; `Domain.ImportProgress` from `src/types/domain.ts`
- Produces:
  - `useImportsStore` — Zustand hook
  - state: `active: ActiveImport | null`, `completed: CompletedImport | null`, `error: string | null`
  - `ActiveImport = { bookId: string; title: string; stage: Domain.ImportStage; current: number; total: number }`
  - `CompletedImport = { documentId: string; title: string }`
  - actions: `start(input: { bookId: string; title: string; run: () => Promise<string> }): Promise<void>`, `applyProgress(payload: Domain.ImportProgress): void`, `dismissCompleted(): void`, `clearError(): void`
  - `attachImportListener(): () => void` — subscribes to the Tauri `import-progress` event, returns a disposer

**Background the implementer needs:** `run` is injected rather than called directly so the store never imports the Tauri API, which keeps every test below free of mocking. `applyProgress` ignores events whose `documentId` does not match the active `bookId`. That matters because Rust emits fetch progress keyed on the **book id** (`src-tauri/src/commands/content.rs:19`) but emits the final `stage: "complete"` event keyed on the newly minted **document id** (`:34`). The completion event is therefore ignored by design — completion is observed by `run()` resolving, not by the event.

- [ ] **Step 1: Write the failing tests**

Create `src/stores/imports.test.ts`:

```ts
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
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
npm test -- src/stores/imports.test.ts
```

Expected: FAIL — cannot resolve `./imports`.

- [ ] **Step 3: Write the store**

Create `src/stores/imports.ts`:

```ts
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
      set({ active: null, completed: { documentId, title } });
    } catch (error) {
      set({ active: null, error: displayError(error) });
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
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
npm test -- src/stores/imports.test.ts
```

Expected: PASS, 7 tests.

- [ ] **Step 5: Run the full gate**

```bash
npm run build && npm test && git diff --check
```

Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add src/stores/imports.ts src/stores/imports.test.ts
git commit -m "feat: add imports store with a route-independent concurrency guard"
```

---

### Task 2: The "already imported" matcher

**Files:**
- Create: `src/lib/importedBooks.ts`
- Test: `src/lib/importedBooks.test.ts`

**Interfaces:**
- Consumes: `Domain.Document` from `src/types/domain.ts`
- Produces: `findImportedBook(documents: Domain.Document[], source: Domain.SourceType, metadataKey: string, catalogId: string): Domain.Document | null`

**Background:** `Document.sourceMetadata` is already exposed to the frontend as `JsonValue` and carries the source identifier — `book_id` for LibreTexts (`src-tauri/src/content/libretexts.rs:686`) and `book_uuid` for OpenStax (`src-tauri/src/content/openstax.rs:360`). The keys differ, so this helper is parameterised rather than duplicated per browser. It must tolerate `sourceMetadata` being `null` or a non-object, because EPUB/PDF/paste imports write different shapes.

- [ ] **Step 1: Write the failing tests**

Create `src/lib/importedBooks.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import type * as Domain from "../types/domain";
import { findImportedBook } from "./importedBooks";

function doc(
  id: string,
  sourceType: Domain.SourceType,
  sourceMetadata: Domain.Document["sourceMetadata"],
): Domain.Document {
  return {
    id,
    title: "General Biology",
    sourceType,
    sourceMetadata,
    coverImagePath: null,
    license: null,
    attribution: null,
    wordCount: 1,
    importedAt: "2026-08-13T00:00:00Z",
    lastOpenedAt: null,
  };
}

describe("findImportedBook", () => {
  it("finds a LibreTexts document by book_id", () => {
    const documents = [doc("d1", "libretexts", { book_id: "bio-1764" })];
    expect(findImportedBook(documents, "libretexts", "book_id", "bio-1764")?.id).toBe("d1");
  });

  it("finds an OpenStax document by book_uuid", () => {
    const documents = [doc("d1", "openstax", { book_uuid: "uuid-42" })];
    expect(findImportedBook(documents, "openstax", "book_uuid", "uuid-42")?.id).toBe("d1");
  });

  it("returns null for a book that is not imported", () => {
    const documents = [doc("d1", "libretexts", { book_id: "bio-1764" })];
    expect(findImportedBook(documents, "libretexts", "book_id", "chem-999")).toBeNull();
  });

  it("does not match across source types even when the id string collides", () => {
    const documents = [doc("d1", "openstax", { book_id: "bio-1764" })];
    expect(findImportedBook(documents, "libretexts", "book_id", "bio-1764")).toBeNull();
  });

  it("tolerates null and non-object sourceMetadata", () => {
    const documents = [
      doc("d1", "libretexts", null),
      doc("d2", "libretexts", "not-an-object" as unknown as Domain.Document["sourceMetadata"]),
      doc("d3", "libretexts", { book_id: "bio-1764" }),
    ];
    expect(findImportedBook(documents, "libretexts", "book_id", "bio-1764")?.id).toBe("d3");
  });

  it("returns the first match when a book was imported twice", () => {
    const documents = [
      doc("d1", "libretexts", { book_id: "bio-1764" }),
      doc("d2", "libretexts", { book_id: "bio-1764" }),
    ];
    expect(findImportedBook(documents, "libretexts", "book_id", "bio-1764")?.id).toBe("d1");
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
npm test -- src/lib/importedBooks.test.ts
```

Expected: FAIL — cannot resolve `./importedBooks`.

- [ ] **Step 3: Write the matcher**

Create `src/lib/importedBooks.ts`:

```ts
import type * as Domain from "../types/domain";

/**
 * Find the library document a catalog entry was imported as, if any.
 *
 * The two catalogs identify books differently — LibreTexts writes `book_id`
 * into source_metadata, OpenStax writes `book_uuid` — so the key is a
 * parameter rather than baked in. sourceMetadata is JsonValue and may be null
 * or a non-object for other import kinds, hence the shape check.
 */
export function findImportedBook(
  documents: Domain.Document[],
  source: Domain.SourceType,
  metadataKey: string,
  catalogId: string,
): Domain.Document | null {
  const match = documents.find((document) => {
    if (document.sourceType !== source) {
      return false;
    }
    const metadata = document.sourceMetadata;
    if (typeof metadata !== "object" || metadata === null || Array.isArray(metadata)) {
      return false;
    }
    return (metadata as Record<string, unknown>)[metadataKey] === catalogId;
  });

  return match ?? null;
}
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
npm test -- src/lib/importedBooks.test.ts
```

Expected: PASS, 6 tests.

- [ ] **Step 5: Run the full gate**

```bash
npm run build && npm test && git diff --check
```

Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add src/lib/importedBooks.ts src/lib/importedBooks.test.ts
git commit -m "feat: add a source-keyed matcher for already-imported catalog books"
```

---

### Task 3: The persistent progress strip

**Files:**
- Create: `src/components/ImportStatus.tsx`
- Modify: `src/components/AppShell.tsx`

**Interfaces:**
- Consumes: `useImportsStore`, `attachImportListener` from `src/stores/imports.ts` (Task 1)
- Produces: `<ImportStatus onOpen={(documentId, title) => void} />`, rendered by `AppShell` immediately above `<MiniPlayer />`

**Background:** `AppShell.tsx:196` renders `<MiniPlayer onClose={resetPlayer} />` outside the route switch — that is the existing pattern for persistent chrome and where the strip belongs. Today `handleLibreTextsImported` / `handleOpenStaxImported` call `openReader`, which yanks the user to the new book. Per the spec, completion must notify instead; the strip's **Open** action becomes the only way a finished catalog import changes the route.

There is no React Testing Library in this project, so this task has no automated test. It is verified by running the app in Step 5.

- [ ] **Step 1: Create the strip component**

Create `src/components/ImportStatus.tsx`:

```tsx
import { BookOpen, Download, X } from "lucide-react";
import { useImportsStore } from "../stores/imports";

interface ImportStatusProps {
  onOpen: (documentId: string, title: string) => void;
}

export function ImportStatus({ onOpen }: ImportStatusProps) {
  const active = useImportsStore((state) => state.active);
  const completed = useImportsStore((state) => state.completed);
  const error = useImportsStore((state) => state.error);
  const dismissCompleted = useImportsStore((state) => state.dismissCompleted);
  const clearError = useImportsStore((state) => state.clearError);

  if (!active && !completed && !error) {
    return null;
  }

  const percent =
    active && active.total > 0
      ? Math.min(100, Math.round((active.current / active.total) * 100))
      : null;

  return (
    <div className="border-t border-neutral-200 bg-white px-4 py-2 dark:border-neutral-800 dark:bg-neutral-900">
      {active ? (
        <div className="flex items-center gap-3">
          <Download className="size-4 shrink-0 animate-pulse text-neutral-500" aria-hidden="true" />
          <div className="min-w-0 flex-1">
            <p className="truncate text-sm font-medium">Importing {active.title}</p>
            <p className="truncate text-xs text-neutral-500 dark:text-neutral-400">
              {active.stage}
              {active.total > 0 ? ` ${active.current}/${active.total}` : null}
            </p>
          </div>
          <div
            aria-label={`Import progress for ${active.title}`}
            aria-valuemax={100}
            aria-valuemin={0}
            aria-valuenow={percent ?? undefined}
            className="h-1.5 w-32 shrink-0 overflow-hidden rounded-full bg-stone-200 dark:bg-neutral-800"
            role="progressbar"
          >
            <div
              className="h-full rounded-full bg-brand-700 transition-[width]"
              style={{ width: percent === null ? "25%" : `${percent}%` }}
            />
          </div>
        </div>
      ) : null}

      {!active && completed ? (
        <div className="flex items-center gap-3">
          <BookOpen className="size-4 shrink-0 text-neutral-500" aria-hidden="true" />
          <p className="min-w-0 flex-1 truncate text-sm">
            <span className="font-medium">{completed.title}</span> imported
          </p>
          <button
            className="inline-flex h-8 items-center gap-2 rounded-md bg-brand-700 px-3 text-sm font-medium text-white hover:bg-brand-500"
            onClick={() => {
              onOpen(completed.documentId, completed.title);
              dismissCompleted();
            }}
            type="button"
          >
            Open
          </button>
          <button
            aria-label="Dismiss"
            className="grid size-8 place-items-center rounded-md text-neutral-500 hover:bg-stone-100 dark:hover:bg-neutral-800"
            onClick={dismissCompleted}
            type="button"
          >
            <X className="size-4" aria-hidden="true" />
          </button>
        </div>
      ) : null}

      {!active && error ? (
        <div className="flex items-center gap-3">
          <p className="min-w-0 flex-1 truncate text-sm text-red-700 dark:text-red-300">{error}</p>
          <button
            aria-label="Dismiss import error"
            className="grid size-8 place-items-center rounded-md text-neutral-500 hover:bg-stone-100 dark:hover:bg-neutral-800"
            onClick={clearError}
            type="button"
          >
            <X className="size-4" aria-hidden="true" />
          </button>
        </div>
      ) : null}
    </div>
  );
}
```

- [ ] **Step 2: Attach the listener once, at app level**

In `src/components/AppShell.tsx`, add the import alongside the existing component imports:

```tsx
import { ImportStatus } from "./ImportStatus";
import { attachImportListener } from "../stores/imports";
```

Then add this effect next to the existing `library-changed` effect (which begins at line 76):

```tsx
  useEffect(() => attachImportListener(), []);
```

- [ ] **Step 3: Render the strip above MiniPlayer**

Replace the existing line `<MiniPlayer onClose={resetPlayer} />` with:

```tsx
        <ImportStatus
          onOpen={(documentId, title) => {
            void refreshLibrary();
            openReader({ id: documentId, title });
          }}
        />
        <MiniPlayer onClose={resetPlayer} />
```

- [ ] **Step 4: Stop auto-navigating on catalog imports**

Delete the `handleOpenStaxImported` and `handleLibreTextsImported` functions (`AppShell.tsx:106-115`), remove the `onOpenStaxImported` / `onLibreTextsImported` props from the `RoutePlaceholder` call site (`:178-183`), from its destructured parameter list (`:208-209`), and from its props interface (`:219-220`). The browsers are rendered at `:249` and `:252`.

Leave `handlePasteImported`, `handleEpubImported`, `handlePdfImported` and `handleUrlImported` alone — those dialogs are fast and out of scope.

The browsers still take an `onImported` prop at this point; Tasks 4 and 5 remove it. To keep this task's tree compiling, pass a no-op for now:

```tsx
{route.id === "openstax" ? <OpenStaxBrowser onImported={() => {}} /> : null}
{route.id === "libretexts" ? <LibreTextsBrowser onImported={() => {}} /> : null}
```

- [ ] **Step 5: Run the full gate and verify by hand**

```bash
npm run build && npm test && git diff --check
```

Expected: all PASS. Then:

```bash
npm run tauri -- build --debug --no-bundle
./target/debug/libretexts-reader
```

Run the binary **directly** — `open` on a `--no-bundle` binary exits 0 without starting anything. Confirm the app launches and that no strip is visible while nothing is importing (the component returns `null` in that state).

- [ ] **Step 6: Commit**

```bash
git add src/components/ImportStatus.tsx src/components/AppShell.tsx
git commit -m "feat: show import progress in persistent chrome instead of navigating on completion"
```

---

### Task 4: Rewire the LibreTexts browser

**Files:**
- Modify: `src/components/LibreTextsBrowser/LibreTextsBrowser.tsx`
- Modify: `src/components/AppShell.tsx` (drop the no-op prop added in Task 3)

**Interfaces:**
- Consumes: `useImportsStore` from `src/stores/imports.ts` (Task 1); `findImportedBook` from `src/lib/importedBooks.ts` (Task 2); `useLibraryStore` from `src/stores/library.ts`
- Produces: a `LibreTextsBrowser` component taking **no props**

**Background:** the component currently owns `importingBookId`, `progress`, an `import-progress` listener keyed on `importingBookId` (`:73-88`), and a component-local guard `if (importingBookId) return` (`:100`). All four move to the store. The card's Add button lives at `:220-235`.

- [ ] **Step 1: Replace the component's import state with store state**

In `src/components/LibreTextsBrowser/LibreTextsBrowser.tsx`:

Update the imports — drop `listen`, add the store and matcher, and drop `Loader2`/`Plus` only if they become unused (the Add button still uses `Plus`):

```tsx
import { useEffect, useMemo, useState } from "react";
import { BookOpen, ExternalLink, Loader2, Plus, Search } from "lucide-react";
import { api } from "../../lib/tauri";
import type * as Domain from "../../types/domain";
import { displayError } from "../../lib/errors";
import { useImportsStore } from "../../stores/imports";
import { useLibraryStore } from "../../stores/library";
import { findImportedBook } from "../../lib/importedBooks";
```

Change the signature to take no props:

```tsx
export function LibreTextsBrowser() {
```

Delete these three state declarations (`:17-19`):

```tsx
  const [importingBookId, setImportingBookId] = useState<string | null>(null);
  const [progress, setProgress] = useState<Domain.ImportProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
```

and replace them with store reads:

```tsx
  const activeImport = useImportsStore((state) => state.active);
  const startImport = useImportsStore((state) => state.start);
  const documents = useLibraryStore((state) => state.documents);
```

- [ ] **Step 2: Delete the component's import-progress listener**

Remove the entire `useEffect` that calls `listen<Domain.ImportProgress>("import-progress", …)` (`:73-88`). `AppShell` now owns that subscription; leaving this one would double-handle every event.

- [ ] **Step 3: Replace `importBook` with a store call**

Replace the whole `importBook` function with:

```tsx
  async function importBook(book: Domain.LibreTextsBook) {
    await startImport({
      bookId: book.bookId,
      title: book.title,
      run: () => api.importLibreTexts(book.bookId),
    });
  }
```

The guard, the error handling and the progress bookkeeping all live in the store now.

- [ ] **Step 4: Show "In library" instead of "Add" for imported books**

Inside the card's `.map((book) => …)`, before the returned JSX, derive the imported document:

```tsx
          const imported = findImportedBook(documents, "libretexts", "book_id", book.bookId);
```

Then replace the Add button block with:

```tsx
                {imported ? (
                  <span className="inline-flex h-9 items-center gap-2 rounded-md border border-neutral-200 px-3 text-sm font-medium text-neutral-600 dark:border-neutral-800 dark:text-neutral-300">
                    <BookOpen className="size-4" aria-hidden="true" />
                    In library
                  </span>
                ) : (
                  <button
                    className="inline-flex h-9 items-center gap-2 rounded-md bg-brand-700 px-3 text-sm font-medium text-white hover:bg-brand-500 disabled:cursor-not-allowed disabled:opacity-50"
                    disabled={Boolean(activeImport)}
                    onClick={() => void importBook(book)}
                    type="button"
                  >
                    {activeImport?.bookId === book.bookId ? (
                      <Loader2 className="size-4 animate-spin" aria-hidden="true" />
                    ) : (
                      <Plus className="size-4" aria-hidden="true" />
                    )}
                    Add
                  </button>
                )}
```

If any error banner in this component referenced the deleted `error` state, delete that banner — errors now surface in the persistent strip.

- [ ] **Step 5: Do not fetch the library here**

The catalog needs `documents` populated to decide "In library", and `AppShell` already
supplies it: `refreshLibrary()` runs on mount (`AppShell.tsx:73`) and again on every
`library-changed` event (`:79`). Add **no** fetching effect to this component — a second
fetcher would race the shared store for no benefit. Read `documents` and nothing else.

- [ ] **Step 6: Drop the no-op prop at the call site**

In `src/components/AppShell.tsx`, change:

```tsx
{route.id === "libretexts" ? <LibreTextsBrowser onImported={() => {}} /> : null}
```

to:

```tsx
{route.id === "libretexts" ? <LibreTextsBrowser /> : null}
```

Also remove `onLibreTextsImported` from the `RoutePlaceholder` props interface if it is still declared there.

- [ ] **Step 7: Run the full gate**

```bash
npm run build && npm test && git diff --check
```

Expected: all PASS. `tsc` is the real check here — it will catch any leftover reference to the deleted state.

- [ ] **Step 8: Verify by hand**

```bash
npm run tauri -- build --debug --no-bundle
./target/debug/libretexts-reader
```

Confirm all four behaviours: (a) a General Biology card reads **In library**, not **+ Add**; (b) starting an import shows the strip; (c) navigating to Reader keeps the strip visible and advancing; (d) returning to the catalog shows the spinner still on that card.

- [ ] **Step 9: Commit**

```bash
git add src/components/LibreTextsBrowser/LibreTextsBrowser.tsx src/components/AppShell.tsx
git commit -m "feat: drive LibreTexts imports from the store and mark imported books"
```

---

### Task 5: Rewire the OpenStax browser

**Files:**
- Modify: `src/components/OpenStaxBrowser/OpenStaxBrowser.tsx`
- Modify: `src/components/AppShell.tsx` (drop the no-op prop added in Task 3)

**Interfaces:**
- Consumes: `useImportsStore` from `src/stores/imports.ts` (Task 1); `findImportedBook` from `src/lib/importedBooks.ts` (Task 2); `useLibraryStore` from `src/stores/library.ts`
- Produces: an `OpenStaxBrowser` component taking **no props**

**Background:** this component mirrors the LibreTexts one but keys on a different identifier. Its state variable is `importingUuid` (not `importingBookId`), its listener sits at `:46-61`, its catalog id is `book.uuid`, and its metadata key is `book_uuid`.

**The API method is `api.importOpenstax` — lowercase `s` in "Openstax"** (`src/lib/tauri.ts:107`). It does not match the `OpenStax` casing used everywhere else in the codebase. Copy it exactly or the build fails.

- [ ] **Step 1: Replace the component's import state with store state**

In `src/components/OpenStaxBrowser/OpenStaxBrowser.tsx`, add these imports:

```tsx
import { useImportsStore } from "../../stores/imports";
import { useLibraryStore } from "../../stores/library";
import { findImportedBook } from "../../lib/importedBooks";
```

and remove the `listen` import from `@tauri-apps/api/event`.

Change the signature to take no props:

```tsx
export function OpenStaxBrowser() {
```

Delete the `importingUuid`, `progress` and `error` state declarations and replace with:

```tsx
  const activeImport = useImportsStore((state) => state.active);
  const startImport = useImportsStore((state) => state.start);
  const documents = useLibraryStore((state) => state.documents);
```

- [ ] **Step 2: Delete the component's import-progress listener**

Remove the entire `useEffect` calling `listen<Domain.ImportProgress>("import-progress", …)` (`:46-61`). `AppShell` owns it now.

- [ ] **Step 3: Replace the import handler with a store call**

Replace the component's import function body with:

```tsx
  async function importBook(book: Domain.OpenStaxBook) {
    await startImport({
      bookId: book.uuid,
      title: book.title,
      run: () => api.importOpenstax(book.uuid),
    });
  }
```

- [ ] **Step 4: Show "In library" instead of "Add" for imported books**

Inside the card `.map((book) => …)`, derive:

```tsx
          const imported = findImportedBook(documents, "openstax", "book_uuid", book.uuid);
```

Then replace the Add button block with:

```tsx
                {imported ? (
                  <span className="inline-flex h-9 items-center gap-2 rounded-md border border-neutral-200 px-3 text-sm font-medium text-neutral-600 dark:border-neutral-800 dark:text-neutral-300">
                    <BookOpen className="size-4" aria-hidden="true" />
                    In library
                  </span>
                ) : (
                  <button
                    className="inline-flex h-9 items-center gap-2 rounded-md bg-brand-700 px-3 text-sm font-medium text-white hover:bg-brand-500 disabled:cursor-not-allowed disabled:opacity-50"
                    disabled={Boolean(activeImport)}
                    onClick={() => void importBook(book)}
                    type="button"
                  >
                    {activeImport?.bookId === book.uuid ? (
                      <Loader2 className="size-4 animate-spin" aria-hidden="true" />
                    ) : (
                      <Plus className="size-4" aria-hidden="true" />
                    )}
                    Add
                  </button>
                )}
```

Ensure `BookOpen` is imported from `lucide-react` in this file; add it to the existing import if absent. Delete any error banner that referenced the removed `error` state.

- [ ] **Step 5: Drop the no-op prop at the call site**

In `src/components/AppShell.tsx`:

```tsx
{route.id === "openstax" ? <OpenStaxBrowser /> : null}
```

Remove `onOpenStaxImported` from the `RoutePlaceholder` props interface if still declared.

- [ ] **Step 6: Confirm no import-progress listener remains outside the store**

```bash
grep -rn "import-progress" src/
```

Expected: exactly one hit, in `src/stores/imports.ts`.

- [ ] **Step 7: Run the full gate**

```bash
npm run build && npm test && git diff --check
```

Expected: all PASS.

- [ ] **Step 8: Verify by hand**

```bash
npm run tauri -- build --debug --no-bundle
./target/debug/libretexts-reader
```

Confirm: an OpenStax import shows the strip; starting a second import while one runs is refused with `An import is already running.` in the strip; completion shows **Open** and does **not** navigate on its own.

- [ ] **Step 9: Commit**

```bash
git add src/components/OpenStaxBrowser/OpenStaxBrowser.tsx src/components/AppShell.tsx
git commit -m "feat: drive OpenStax imports from the store and mark imported books"
```

---

## Rollback

Every task is an independent commit and revertible with `git revert`. Nothing touches Rust, the database, or on-disk state, so a revert is complete — there is no data migration to undo.

## Out of scope

Recorded in the spec, not built here: cancellation, resume/checkpointing, deterministic image filenames, and deduplicating the two General Biology documents already in the library (delete one in the Library UI — `db::library::delete_document` removes its image files too).
