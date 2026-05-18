import { type FormEvent, useState } from "react";
import { Clipboard, Loader2 } from "lucide-react";
import { api } from "../../lib/tauri";

interface PasteDialogProps {
  onImported: (documentId: string, title: string) => void;
}

export function PasteDialog({ onImported }: PasteDialogProps) {
  const [title, setTitle] = useState("");
  const [text, setText] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const canSubmit =
    title.trim().length > 0 && text.trim().length > 0 && !submitting;

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!canSubmit) {
      return;
    }

    setSubmitting(true);
    setError(null);

    try {
      const documentId = await api.importPastedText(title.trim(), text);
      onImported(documentId, title.trim());
      setTitle("");
      setText("");
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error));
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <form
      className="flex flex-col gap-4 rounded-md border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900"
      onSubmit={handleSubmit}
    >
      <div className="flex items-center gap-3">
        <div className="grid size-9 place-items-center rounded-md bg-brand-50 text-brand-700 dark:bg-neutral-800 dark:text-brand-500">
          <Clipboard className="size-4" aria-hidden="true" />
        </div>
        <h2 className="text-base font-semibold">Paste Text</h2>
      </div>

      <label className="flex flex-col gap-2 text-sm font-medium">
        Title
        <input
          className="h-10 rounded-md border border-neutral-200 bg-white px-3 text-sm font-normal outline-none focus:border-brand-500 focus:ring-2 focus:ring-brand-500/20 dark:border-neutral-800 dark:bg-neutral-950"
          onChange={(event) => setTitle(event.target.value)}
          value={title}
        />
      </label>

      <label className="flex flex-col gap-2 text-sm font-medium">
        Text
        <textarea
          className="min-h-48 resize-y rounded-md border border-neutral-200 bg-white p-3 text-sm font-normal leading-6 outline-none focus:border-brand-500 focus:ring-2 focus:ring-brand-500/20 dark:border-neutral-800 dark:bg-neutral-950"
          onChange={(event) => setText(event.target.value)}
          value={text}
        />
      </label>

      {error ? (
        <p className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700 dark:border-red-950 dark:bg-red-950/30 dark:text-red-300">
          {error}
        </p>
      ) : null}

      <div className="flex justify-end">
        <button
          className="inline-flex h-10 items-center gap-2 rounded-md bg-brand-700 px-4 text-sm font-medium text-white hover:bg-brand-500 disabled:cursor-not-allowed disabled:opacity-50"
          disabled={!canSubmit}
          type="submit"
        >
          {submitting ? (
            <Loader2 className="size-4 animate-spin" aria-hidden="true" />
          ) : null}
          Import
        </button>
      </div>
    </form>
  );
}
