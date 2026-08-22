import { formatBytes } from "../lib/format";
import { usePlayerStore } from "../stores/player";

/**
 * The one-time voice-model fetch, as something a reader can watch and stop.
 *
 * Rendered instead of the buffering spinner, never alongside it: the spinner
 * is right for the seconds a sentence takes and wrong for the minutes ~383MB
 * takes, and showing both would say the app is doing two things when it is
 * doing one. Returns null when nothing is downloading, so callers can drop it
 * in without repeating the check.
 */
export function ModelDownloadProgress() {
  const modelDownload = usePlayerStore((state) => state.modelDownload);
  const bufferingMessage = usePlayerStore((state) => state.bufferingMessage);
  const cancelModelDownload = usePlayerStore(
    (state) => state.cancelModelDownload,
  );

  if (!modelDownload) {
    return null;
  }

  const { downloadedBytes, totalBytes } = modelDownload;
  // Clamped because Rust widens `total` to whatever has already arrived when a
  // file turns out larger than its manifest says, and a bar past 100% reads as
  // a bug in the app rather than a rounding artefact.
  const percent =
    totalBytes > 0
      ? Math.min(100, Math.round((downloadedBytes / totalBytes) * 100))
      : 0;

  return (
    <div className="flex min-w-0 flex-1 items-center gap-3">
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline justify-between gap-2">
          <span className="truncate text-sm text-neutral-600 dark:text-neutral-300">
            {bufferingMessage || "Downloading the on-device voice"}
          </span>
          <span className="shrink-0 text-xs tabular-nums text-neutral-500 dark:text-neutral-400">
            {formatBytes(downloadedBytes)} of {formatBytes(totalBytes)} ·{" "}
            {percent}%
          </span>
        </div>
        <div
          aria-label="Voice model download"
          aria-valuemax={100}
          aria-valuemin={0}
          aria-valuenow={percent}
          className="mt-1 h-1.5 w-full overflow-hidden rounded-full bg-neutral-200 dark:bg-neutral-800"
          role="progressbar"
        >
          <div
            className="h-full rounded-full bg-brand-700 transition-[width] duration-300"
            style={{ width: `${percent}%` }}
          />
        </div>
      </div>
      <button
        className="h-8 shrink-0 rounded-md border border-neutral-200 px-3 text-sm font-medium text-neutral-700 hover:bg-stone-100 focus:outline-none focus:ring-2 focus:ring-brand-500 dark:border-neutral-800 dark:text-neutral-200 dark:hover:bg-neutral-800"
        onClick={cancelModelDownload}
        type="button"
      >
        Cancel
      </button>
    </div>
  );
}
