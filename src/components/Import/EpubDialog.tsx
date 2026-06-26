import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { BookOpen, Loader2, Upload } from "lucide-react";
import { api } from "../../lib/tauri";

type ImportStage = "idle" | "Parsing..." | "Saving...";

interface EpubDialogProps {
  onImported: (documentId: string, title: string) => void;
}

export function EpubDialog({ onImported }: EpubDialogProps) {
  const [filePath, setFilePath] = useState("");
  const [stage, setStage] = useState<ImportStage>("idle");
  const [error, setError] = useState<string | null>(null);

  const submitting = stage !== "idle";

  async function chooseFile() {
    setError(null);
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: "EPUB", extensions: ["epub"] }],
      });
      if (typeof selected === "string") {
        setFilePath(selected);
      }
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error));
    }
  }

  async function importFile() {
    if (!filePath || submitting) {
      return;
    }

    setError(null);
    setStage("Parsing...");
    const timer = window.setTimeout(() => setStage("Saving..."), 700);

    try {
      const documentId = await api.importEpub(filePath);
      onImported(documentId, fileName(filePath));
      setFilePath("");
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error));
    } finally {
      window.clearTimeout(timer);
      setStage("idle");
    }
  }

  return (
    <section className="flex flex-col gap-4 rounded-md border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900">
      <div className="flex items-center gap-3">
        <div className="grid size-9 place-items-center rounded-md bg-brand-50 text-brand-700 dark:bg-neutral-800 dark:text-brand-500">
          <BookOpen className="size-4" aria-hidden="true" />
        </div>
        <h2 className="text-base font-semibold">EPUB Import</h2>
      </div>

      <div className="flex flex-col gap-3 sm:flex-row">
        <button
          className="inline-flex h-10 items-center justify-center gap-2 rounded-md border border-neutral-200 px-4 text-sm font-medium text-neutral-700 hover:bg-stone-100 focus:outline-none focus:ring-2 focus:ring-brand-500 disabled:cursor-not-allowed disabled:opacity-50 dark:border-neutral-800 dark:text-neutral-300 dark:hover:bg-neutral-800"
          disabled={submitting}
          onClick={() => void chooseFile()}
          type="button"
        >
          <Upload className="size-4" aria-hidden="true" />
          Choose
        </button>
        <div className="flex min-h-10 min-w-0 flex-1 items-center rounded-md border border-neutral-200 px-3 text-sm text-neutral-600 dark:border-neutral-800 dark:text-neutral-300">
          <span className="truncate">{filePath || "No file selected"}</span>
        </div>
      </div>

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
          disabled={!filePath || submitting}
          onClick={() => void importFile()}
          type="button"
        >
          {submitting ? (
            <Loader2 className="size-4 animate-spin" aria-hidden="true" />
          ) : null}
          Import
        </button>
      </div>
    </section>
  );
}

function fileName(path: string): string {
  return (
    path
      .split(/[\\/]/)
      .pop()
      ?.replace(/\.epub$/i, "") || "EPUB"
  );
}
