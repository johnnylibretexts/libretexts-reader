# Durable import state — design

**Date:** 2026-08-13
**Status:** approved, not yet implemented

Import progress currently lives in component state that is destroyed when the user
navigates away from the import screen. The import itself keeps running, but it becomes
invisible and — worse — its concurrency guard resets, so a second import of the same book
can start. Move import lifecycle state into a store, surface it in persistent chrome, and
make the catalog reflect what is already in the library.

## The bug, precisely

`AppShell.tsx:251` renders the browser conditionally:

```tsx
{route.id === "libretexts" ? <LibreTextsBrowser … /> : null}
```

Navigating away unmounts it. Nothing cancels the import — **there is no cancellation path
in this codebase**: no `AbortController`, no cancel command, no abort signal reaching Rust.
The awaited promise's closure survives unmounting and `import_libretexts` runs to
completion. What dies is the ability to observe it:

| Lost on unmount | Consequence |
|---|---|
| `progress` / `importingBookId` state (`LibreTextsBrowser.tsx:99-118`) | Progress UI disappears |
| `listen("import-progress")` subscription (`:75`) | Rust keeps emitting into nothing |
| The `if (importingBookId) return` guard (`:100`) | Resets to `null` — a second import can start |

### Confirmed consequence

Observed on 2026-08-13. Two complete imports of the same book both persisted:

| Document | `book_id` | Sections | Images | Imported |
|---|---|---|---|---|
| `e428f843…` | `bio-1764` | 358 | 1190 | 18:54:16Z |
| `7f4433cc…` | `bio-1764` | 358 | 1190 | 18:59:29Z |

2380 image rows, 2380 files, **616 MB where ~308 MB would do**. Every image was downloaded
twice under a different random UUID (`content/images.rs:143`), so nothing could recognise
the second copy as identical. There are **no orphaned files** — every file has a row.

The catalog compounds it: cards render `+ Add` without consulting library state, so the UI
invites a duplicate rather than preventing it.

## Scope

**In:** import lifecycle state in a store · persistent progress strip · global concurrency
guard · notify-instead-of-navigate on completion · catalog "In library" state.

**Out:** cancellation · resume/checkpointing · deterministic image naming · deduplicating
documents already imported (the existing pair is a manual delete). Resume and dedup pivot
on URL-derived image filenames and get their own design cycle; see "Follow-on work".

**No Rust changes.** `commands/content.rs:19` already emits `import-progress` with
`stage`/`current`/`total`, and `:34` emits `library-changed`. The backend is sufficient.

## Architecture

A new Zustand store owns import lifecycle state and subscribes to `import-progress` **once,
at app level**. The browsers become stateless views over it.

A separate store rather than an extension of `library.ts`: that file is CRUD over persisted
documents, this is transient job state with a different lifecycle. Separating them keeps
both focused, and matches the existing `library` / `player` / `settings` split.

### Components

| File | Change |
|---|---|
| `src/stores/imports.ts` | **new** — the store |
| `src/components/ImportStatus.tsx` | **new** — persistent strip, rendered beside `MiniPlayer` |
| `src/components/LibreTextsBrowser/LibreTextsBrowser.tsx` | remove local progress state and `listen`; read the store; card shows "In library" |
| `src/components/OpenStaxBrowser/OpenStaxBrowser.tsx` | same — removes a duplicated listener |
| `src/components/AppShell.tsx` | render `<ImportStatus/>`; stop auto-navigating on completion |

### Store shape

```ts
interface ImportState {
  active: { bookId: string; title: string; stage: string;
            current: number; total: number } | null;
  completed: { documentId: string; title: string } | null;
  error: string | null;

  start: (bookId: string, title: string,
          run: () => Promise<string>) => Promise<void>;
  dismissCompleted: () => void;
  clearError: () => void;
}
```

`start()` is where the guard lives: if `active` is non-null it returns immediately without
invoking `run`, and sets `error` to "An import is already running." Because the store
outlives every route, unmounting cannot reset it.

## Data flow

```
Rust import_libretexts
  ├─ emit "import-progress" {stage,current,total} ──► imports store ──► ImportStatus strip
  │                                                       └───────────► browser card state
  └─ persist() → emit "library-changed" ────────────► library.refresh()
                                                          └──► store.completed = {id,title}
                                                               strip shows "Imported — Open"
```

On completion the strip offers **Open**; it does **not** call `openReader()`. Today
completion yanks the user to the new book (`AppShell.tsx:93`), which interrupts reading or
playback when the import finishes while they are elsewhere.

The `import-progress` listener is registered once and disposed only at app teardown, so
events can never fire into a disposed subscription.

## "In library" detection

`Document.sourceMetadata` is already exposed to the frontend (`types/domain.ts:5`) and
carries `book_id`. No backend change is needed:

```ts
const imported = documents.find(
  (d) => d.sourceType === "libretexts" &&
         (d.sourceMetadata as { book_id?: string } | null)?.book_id === book.bookId,
);
```

When `imported` is set, the card renders **In library** with an **Open** action in place of
**+ Add**.

The two sources use different identifiers, so the matcher is per-source rather than shared:

| Source | `source_metadata` key | Catalog field | Written at |
|---|---|---|---|
| LibreTexts | `book_id` | `LibreTextsBook.bookId` | `content/libretexts.rs:686` |
| OpenStax | `book_uuid` | `OpenStaxBook.uuid` | `content/openstax.rs:360` |

Implement one helper taking `(sourceType, metadataKey, catalogId)` so both browsers call the
same logic with their own key, rather than duplicating the lookup a third time.

Both existing duplicates carry `book_id: "bio-1764"`, so this check would have caught them.

## Error handling

- Import failure sets `store.error`; the strip surfaces it and it is dismissible. The
  browser no longer owns error state.
- A second import attempted while one is active is a no-op with a message, not a silent
  drop — the user must learn why nothing happened.
- Progress events for a `bookId` that is not `active` are ignored rather than adopted, so a
  late event from a previous import cannot resurrect a finished strip.

## Testing

Store-level vitest only, matching the existing posture (`errors`, `mathContent`, `player`
are pure-logic suites; no React Testing Library is configured, so components are verified by
running the app).

1. **`start()` while `active` is non-null is rejected** and leaves the first import
   untouched — this is the regression test for the duplicate above.
2. A progress event updates `active` with stage/current/total.
3. Completion clears `active`, sets `completed`, and does not navigate.
4. A progress event whose `bookId` does not match `active` is ignored.
5. The "in library" matcher identifies an imported book by `book_id` and rejects a
   different one.

Manual verification, since components are untested by machine: start an import, navigate to
the Reader, confirm the strip persists and advances; confirm the catalog card for an
imported book reads "In library"; confirm completion does not steal focus from the reader.

## Follow-on work

Not in this spec, recorded so the sequence is clear:

1. **Deterministic image filenames** (URL- or content-derived instead of
   `Uuid::new_v4()` at `content/images.rs:143`). This is the pivot: it makes re-import
   idempotent, enables resume, and would have made the duplicate cost ~0 MB rather than
   308 MB.
2. **Resume / checkpointing.** Blocked today because `import_book` builds a whole
   `DocumentBuilder` in memory and `persist()` writes it in one transaction with a
   `document_id` minted at write time (`content/document.rs:52`) — an interrupted import
   leaves nothing to resume from.
3. **Cancellation.** No path exists into the running import.
4. **Deduplicating documents already imported**, once (1) makes identity cheap.
