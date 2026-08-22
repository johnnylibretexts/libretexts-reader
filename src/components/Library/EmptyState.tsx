import { BookOpen, Download } from "lucide-react";
import { useEffect, useState } from "react";
import { formatBytes } from "../../lib/format";
import { api } from "../../lib/tauri";
import { useSettingsStore } from "../../stores/settings";

export function EmptyState() {
  const ttsProvider = useSettingsStore((state) => state.ttsProvider);
  /**
   * The size of the fetch the reader's first Play will trigger, or null when
   * there is nothing to warn about.
   *
   * Read from the model status rather than written into the copy so it cannot
   * drift from the manifest, which is the only place the real figure lives
   * (`src-tauri/src/tts/supertonic/model.rs`).
   */
  const [pendingDownloadBytes, setPendingDownloadBytes] = useState<
    number | null
  >(null);

  useEffect(() => {
    // Fish synthesizes in the cloud and downloads no model, so this warning
    // would describe something that will never happen to that reader.
    if (ttsProvider !== "supertonic") {
      setPendingDownloadBytes(null);
      return;
    }

    let cancelled = false;
    void api
      .getSupertonicModelStatus()
      .then((status) => {
        if (!cancelled) {
          setPendingDownloadBytes(status.downloaded ? null : status.totalBytes);
        }
      })
      // Silently: this is a heads-up on a screen with nothing wrong with it,
      // and an error banner about a voice model on an empty library would be
      // more alarming than the thing it failed to say.
      .catch(() => undefined);

    return () => {
      cancelled = true;
    };
  }, [ttsProvider]);

  return (
    <div className="flex min-h-56 flex-col items-center justify-center gap-3 rounded-md border border-dashed border-neutral-300 bg-white px-6 text-center dark:border-neutral-800 dark:bg-neutral-900">
      <div className="grid size-12 place-items-center rounded-md bg-brand-50 text-brand-700 dark:bg-neutral-800 dark:text-brand-500">
        <BookOpen className="size-6" aria-hidden="true" />
      </div>
      <p className="max-w-sm text-sm text-neutral-600 dark:text-neutral-400">
        Get started by importing a book from OpenStax, LibreTexts, or
        Pressbooks — or bring your own EPUB, PDF, article link, or pasted text.
      </p>
      {pendingDownloadBytes ? (
        <p className="flex max-w-sm items-start gap-2 text-sm text-neutral-500 dark:text-neutral-400">
          <Download className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
          <span>
            Reading aloud uses an on-device voice. The first time you press
            Play it downloads once — about{" "}
            {formatBytes(pendingDownloadBytes)} — and then works offline.
          </span>
        </p>
      ) : null}
    </div>
  );
}
