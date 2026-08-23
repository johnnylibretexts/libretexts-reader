import { convertFileSrc } from "@tauri-apps/api/core";
import { useEffect, useState, type MouseEvent } from "react";
import {
  BookOpen,
  Clipboard,
  FileText,
  Link,
  MoreHorizontal,
  Trash2,
} from "lucide-react";
import type * as Domain from "../../types/domain";

interface DocumentCardProps {
  document: Domain.Document;
  onContextMenu: (
    event: MouseEvent<HTMLElement>,
    document: Domain.Document,
  ) => void;
  onDelete: (document: Domain.Document) => void;
  onOpen: (document: Domain.Document) => void;
}

export function DocumentCard({
  document,
  onContextMenu,
  onDelete,
  onOpen,
}: DocumentCardProps) {
  const Icon = sourceIcon(document.sourceType);
  // Whole percent, because that is the resolution a bar this wide can show and
  // the number a screen reader has to read out. `progress` arrives clamped to
  // 0..1 from the query that derives it -- see `Document.progress`.
  const progress = Math.round(document.progress * 100);
  // A stored cover can stop rendering -- the file is gone, or is not the image
  // its name claims. Without this the card keeps a broken-image glyph; the
  // Source icon is the fallback that already exists for a Document with no
  // cover at all. Same pattern as the Pressbooks browser's thumbnail.
  const [coverFailed, setCoverFailed] = useState(false);

  useEffect(() => {
    setCoverFailed(false);
  }, [document.coverImagePath]);

  return (
    <article
      className="flex min-h-52 flex-col justify-between rounded-md border border-neutral-200 bg-white p-4 shadow-sm dark:border-neutral-800 dark:bg-neutral-900"
      onContextMenu={(event) => onContextMenu(event, document)}
    >
      <div className="flex gap-3">
        <div className="grid size-16 shrink-0 place-items-center overflow-hidden rounded-md bg-stone-100 text-brand-700 dark:bg-neutral-800 dark:text-brand-500">
          {document.coverImagePath && !coverFailed ? (
            <img
              alt=""
              className="size-full object-cover"
              onError={() => setCoverFailed(true)}
              src={convertFileSrc(document.coverImagePath)}
            />
          ) : (
            <Icon className="size-7" aria-hidden="true" />
          )}
        </div>
        <div className="min-w-0 flex-1">
          <h2 className="line-clamp-2 text-base font-semibold">
            {document.title}
          </h2>
          <p className="mt-1 text-sm capitalize text-neutral-600 dark:text-neutral-400">
            {sourceLabel(document.sourceType)}
          </p>
          <p className="mt-2 text-sm text-neutral-500 dark:text-neutral-400">
            {document.wordCount.toLocaleString()} words
          </p>
        </div>
      </div>

      <div className="mt-4 flex flex-col gap-3">
        <div>
          <div
            aria-label={`${progress}% through ${document.title}`}
            aria-valuemax={100}
            aria-valuemin={0}
            aria-valuenow={progress}
            className="h-2 overflow-hidden rounded-full bg-neutral-100 dark:bg-neutral-800"
            role="progressbar"
          >
            <div
              className="h-full bg-brand-700"
              style={{ width: `${progress}%` }}
            />
          </div>
          <p className="mt-2 text-xs text-neutral-500 dark:text-neutral-400">
            {document.lastOpenedAt
              ? `Last opened ${formatDate(document.lastOpenedAt)}`
              : `Imported ${formatDate(document.importedAt)}`}
          </p>
        </div>

        <div className="flex items-center justify-between gap-2">
          <button
            className="inline-flex h-9 items-center gap-2 rounded-md bg-brand-700 px-3 text-sm font-medium text-white hover:bg-brand-500 focus:outline-none focus:ring-2 focus:ring-brand-500 disabled:cursor-not-allowed disabled:opacity-50"
            onClick={() => onOpen(document)}
            type="button"
          >
            <BookOpen className="size-4" aria-hidden="true" />
            Open
          </button>
          <div className="flex items-center gap-2">
            <button
              aria-label={`Delete ${document.title}`}
              className="grid size-9 place-items-center rounded-md border border-neutral-200 text-neutral-600 hover:bg-stone-100 focus:outline-none focus:ring-2 focus:ring-brand-500 dark:border-neutral-800 dark:text-neutral-300 dark:hover:bg-neutral-800"
              onClick={() => onDelete(document)}
              title="Delete"
              type="button"
            >
              <Trash2 className="size-4" aria-hidden="true" />
            </button>
            <button
              aria-label={`More actions for ${document.title}`}
              className="grid size-9 place-items-center rounded-md border border-neutral-200 text-neutral-600 hover:bg-stone-100 focus:outline-none focus:ring-2 focus:ring-brand-500 dark:border-neutral-800 dark:text-neutral-300 dark:hover:bg-neutral-800"
              onClick={(event) => onContextMenu(event, document)}
              title="More"
              type="button"
            >
              <MoreHorizontal className="size-4" aria-hidden="true" />
            </button>
          </div>
        </div>
      </div>
    </article>
  );
}

function sourceIcon(sourceType: Domain.SourceType) {
  switch (sourceType) {
    case "openstax":
    case "libretexts":
    case "pressbooks":
      return BookOpen;
    case "epub":
    case "pdf":
      return FileText;
    case "pasted":
      return Clipboard;
    case "url":
      return Link;
  }
}

function sourceLabel(sourceType: Domain.SourceType) {
  switch (sourceType) {
    case "url":
      return "URL";
    case "openstax":
      return "OpenStax";
    case "libretexts":
      return "LibreTexts";
    case "pressbooks":
      return "Pressbooks";
    case "epub":
      return "EPUB";
    case "pdf":
      return "PDF";
    case "pasted":
      return "Pasted text";
    default:
      return sourceType;
  }
}

function formatDate(value: string) {
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
  }).format(new Date(value));
}
