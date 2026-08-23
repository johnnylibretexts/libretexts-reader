import { BookOpen, Download, X } from "lucide-react";
import { useImportsStore } from "../stores/imports";
import type * as Domain from "../types/domain";

interface ImportStatusProps {
  onOpen: (documentId: string, title: string) => void;
}

const STAGE_LABELS: Record<Domain.ImportStage, string> = {
  fetching: "Fetching chapters",
  parsing: "Parsing content",
  tokenizing: "Preparing text",
  storing: "Saving to library",
  complete: "Finishing up",
  failed: "Failed",
};

export function ImportStatus({ onOpen }: ImportStatusProps) {
  const active = useImportsStore((state) => state.active);
  const completed = useImportsStore((state) => state.completed);
  const error = useImportsStore((state) => state.error);
  const dismissCompleted = useImportsStore((state) => state.dismissCompleted);
  const clearError = useImportsStore((state) => state.clearError);
  const cancel = useImportsStore((state) => state.cancel);

  if (!active && !completed && !error) {
    return null;
  }

  const percent =
    active && active.total > 0
      ? Math.min(100, Math.round((active.current / active.total) * 100))
      : null;

  return (
    <div
      aria-live="polite"
      className="border-t border-neutral-200 bg-white px-4 py-2 dark:border-neutral-800 dark:bg-neutral-900"
      role="status"
    >
      {active ? (
        <div className="flex items-center gap-3">
          <Download className="size-4 shrink-0 animate-pulse text-neutral-500" aria-hidden="true" />
          <div className="min-w-0 flex-1">
            <p className="truncate text-sm font-medium">Importing {active.title}</p>
            <p className="truncate text-xs text-neutral-500 dark:text-neutral-400">
              {STAGE_LABELS[active.stage] ?? active.stage}
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
          <button
            aria-label={`Cancel importing ${active.title}`}
            className="grid size-8 shrink-0 place-items-center rounded-md text-neutral-500 hover:bg-stone-100 focus:outline-none focus:ring-2 focus:ring-brand-500 dark:hover:bg-neutral-800"
            onClick={() => void cancel()}
            title="Cancel import"
            type="button"
          >
            <X className="size-4" aria-hidden="true" />
          </button>
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

      {error ? (
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
