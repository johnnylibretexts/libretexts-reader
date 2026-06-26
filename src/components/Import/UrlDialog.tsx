import { type FormEvent, useMemo, useState } from "react";
import { Link, Loader2 } from "lucide-react";
import { api } from "../../lib/tauri";
import { displayError } from "../../lib/errors";

type ImportStage = "idle" | "Fetching..." | "Parsing..." | "Saving...";

interface UrlDialogProps {
  onImported: (documentId: string, title: string) => void;
}

export function UrlDialog({ onImported }: UrlDialogProps) {
  const [url, setUrl] = useState("");
  const [stage, setStage] = useState<ImportStage>("idle");
  const [error, setError] = useState<string | null>(null);

  const trimmedUrl = url.trim();
  const urlIsValid = useMemo(
    () => /^https?:\/\/\S+$/i.test(trimmedUrl),
    [trimmedUrl],
  );
  const submitting = stage !== "idle";
  const canSubmit = urlIsValid && !submitting;

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!canSubmit) {
      return;
    }

    setError(null);
    setStage("Fetching...");
    const timers = [
      window.setTimeout(() => setStage("Parsing..."), 500),
      window.setTimeout(() => setStage("Saving..."), 1000),
    ];

    try {
      const documentId = await api.importUrl(trimmedUrl);
      onImported(documentId, trimmedUrl);
      setUrl("");
    } catch (error) {
      setError(displayError(error));
    } finally {
      timers.forEach(window.clearTimeout);
      setStage("idle");
    }
  }

  return (
    <form
      className="flex flex-col gap-4 rounded-md border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900"
      onSubmit={handleSubmit}
    >
      <div className="flex items-center gap-3">
        <div className="grid size-9 place-items-center rounded-md bg-brand-50 text-brand-700 dark:bg-neutral-800 dark:text-brand-500">
          <Link className="size-4" aria-hidden="true" />
        </div>
        <h2 className="text-base font-semibold">Article URL</h2>
      </div>

      <label className="flex flex-col gap-2 text-sm font-medium">
        URL
        <input
          className="h-10 rounded-md border border-neutral-200 bg-white px-3 text-sm font-normal outline-none focus:border-brand-500 focus:ring-2 focus:ring-brand-500/20 dark:border-neutral-800 dark:bg-neutral-950"
          onChange={(event) => setUrl(event.target.value)}
          value={url}
        />
      </label>

      {trimmedUrl && !urlIsValid ? (
        <p className="rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-sm text-amber-800 dark:border-amber-950 dark:bg-amber-950/30 dark:text-amber-300">
          URL must start with http:// or https://.
        </p>
      ) : null}

      {error ? (
        <p className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700 dark:border-red-950 dark:bg-red-950/30 dark:text-red-300">
          {error}
        </p>
      ) : null}

      <div className="flex items-center justify-between gap-3">
        <p className="text-sm text-neutral-500 dark:text-neutral-400">
          {submitting ? stage : ""}
        </p>
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
