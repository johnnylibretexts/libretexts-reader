import {
  Download,
  Loader2,
  Play,
  Trash2,
  Volume2,
  VolumeX,
} from "lucide-react";
import type * as Domain from "../../types/domain";

interface VoiceProgress {
  downloaded: number;
  total: number;
}

interface VoiceCardProps {
  disabled: boolean;
  onDelete: (voice: Domain.Voice) => void;
  onDownload: (voice: Domain.Voice) => void;
  onPreview: (voice: Domain.Voice) => void;
  previewing: boolean;
  progress: VoiceProgress | null;
  voice: Domain.Voice;
}

export function VoiceCard({
  disabled,
  onDelete,
  onDownload,
  onPreview,
  previewing,
  progress,
  voice,
}: VoiceCardProps) {
  const installed = voice.isBundled || voice.isDownloaded;
  const downloading = Boolean(progress);
  const percent =
    progress && progress.total > 0
      ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100))
      : 0;

  return (
    <article className="flex min-h-44 flex-col justify-between rounded-md border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900">
      <div className="flex items-start gap-3">
        <div className="grid size-11 shrink-0 place-items-center rounded-md bg-brand-50 text-brand-700 dark:bg-neutral-800 dark:text-brand-500">
          {installed ? (
            <Volume2 className="size-5" aria-hidden="true" />
          ) : (
            <VolumeX className="size-5" aria-hidden="true" />
          )}
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 flex-wrap items-start justify-between gap-2">
            <h2 className="min-w-0 truncate text-base font-semibold">
              {voice.displayName}
            </h2>
            {installed ? (
              <span className="rounded-md bg-emerald-50 px-2 py-1 text-xs font-medium text-emerald-700 dark:bg-emerald-950/40 dark:text-emerald-300">
                Installed
              </span>
            ) : null}
          </div>
          <p className="mt-1 text-sm text-neutral-600 dark:text-neutral-400">
            {voice.language} · {voice.gender}
          </p>
          <p className="mt-2 text-sm text-neutral-500 dark:text-neutral-400">
            {formatBytes(voice.sizeBytes)}
          </p>
        </div>
      </div>

      {progress ? (
        <div className="mt-4">
          <div className="h-2 overflow-hidden rounded-full bg-neutral-100 dark:bg-neutral-800">
            <div
              className="h-full bg-brand-700 transition-[width]"
              style={{ width: `${percent}%` }}
            />
          </div>
          <p className="mt-2 text-xs text-neutral-500 dark:text-neutral-400">
            {formatBytes(progress.downloaded)} / {formatBytes(progress.total)}
          </p>
        </div>
      ) : null}

      <div className="mt-4 flex items-center justify-between gap-2">
        <button
          aria-label={`Preview ${voice.displayName}`}
          className="inline-flex h-9 items-center gap-2 rounded-md border border-neutral-200 px-3 text-sm font-medium text-neutral-700 hover:bg-stone-100 focus:outline-none focus:ring-2 focus:ring-brand-500 disabled:cursor-not-allowed disabled:opacity-50 dark:border-neutral-800 dark:text-neutral-200 dark:hover:bg-neutral-800"
          disabled={!installed || disabled || downloading || previewing}
          onClick={() => onPreview(voice)}
          title="Preview"
          type="button"
        >
          {previewing ? (
            <Loader2 className="size-4 animate-spin" aria-hidden="true" />
          ) : (
            <Play className="size-4" aria-hidden="true" />
          )}
          Preview
        </button>

        {installed && !voice.isBundled ? (
          <button
            aria-label={`Delete ${voice.displayName}`}
            className="grid size-9 place-items-center rounded-md border border-neutral-200 text-neutral-600 hover:bg-stone-100 focus:outline-none focus:ring-2 focus:ring-brand-500 disabled:cursor-not-allowed disabled:opacity-50 dark:border-neutral-800 dark:text-neutral-300 dark:hover:bg-neutral-800"
            disabled={disabled || downloading}
            onClick={() => onDelete(voice)}
            title="Delete"
            type="button"
          >
            <Trash2 className="size-4" aria-hidden="true" />
          </button>
        ) : !installed ? (
          <button
            className="inline-flex h-9 items-center gap-2 rounded-md bg-brand-700 px-3 text-sm font-medium text-white hover:bg-brand-500 focus:outline-none focus:ring-2 focus:ring-brand-500 disabled:cursor-not-allowed disabled:opacity-50"
            disabled={disabled || downloading}
            onClick={() => onDownload(voice)}
            type="button"
          >
            {downloading ? (
              <Loader2 className="size-4 animate-spin" aria-hidden="true" />
            ) : (
              <Download className="size-4" aria-hidden="true" />
            )}
            Download
          </button>
        ) : (
          <span className="h-9" />
        )}
      </div>
    </article>
  );
}

function formatBytes(value: number) {
  if (value <= 0) {
    return "0 MB";
  }

  return `${(value / 1_000_000).toFixed(1)} MB`;
}
