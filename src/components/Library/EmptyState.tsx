import { BookOpen } from "lucide-react";

export function EmptyState() {
  return (
    <div className="flex min-h-56 flex-col items-center justify-center gap-3 rounded-md border border-dashed border-neutral-300 bg-white px-6 text-center dark:border-neutral-800 dark:bg-neutral-900">
      <div className="grid size-12 place-items-center rounded-md bg-brand-50 text-brand-700 dark:bg-neutral-800 dark:text-brand-500">
        <BookOpen className="size-6" aria-hidden="true" />
      </div>
      <p className="max-w-sm text-sm text-neutral-600 dark:text-neutral-400">
        Get started by importing a book from OpenStax, LibreTexts, or
        Pressbooks — or bring your own EPUB, PDF, article link, or pasted text.
      </p>
    </div>
  );
}
