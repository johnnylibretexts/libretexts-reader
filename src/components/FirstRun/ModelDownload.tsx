import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { CheckCircle2, Download, Loader2 } from "lucide-react";
import { api, isTauriRuntime } from "../../lib/tauri";
import { type ModelPrecision, useSettingsStore } from "../../stores/settings";

interface ModelProgress {
  downloaded: number;
  total: number;
}

const MODEL_OPTIONS: Array<{
  precision: ModelPrecision;
  label: string;
  size: string;
  detail: string;
}> = [
  {
    precision: "q8",
    label: "Recommended",
    size: "92 MB",
    detail: "Best default for responsive local playback.",
  },
  {
    precision: "fp32",
    label: "Full quality",
    size: "326 MB",
    detail: "Higher fidelity, but slower to start and generate.",
  },
];

export function ModelDownload() {
  const hydrated = useSettingsStore((state) => state.hydrated);
  const modelDownloaded = useSettingsStore((state) => state.modelDownloaded);
  const modelPrecision = useSettingsStore((state) => state.modelPrecision);
  const markModelDownloaded = useSettingsStore(
    (state) => state.markModelDownloaded,
  );
  const [selectedPrecision, setSelectedPrecision] =
    useState<ModelPrecision>(modelPrecision);
  const [progress, setProgress] = useState<ModelProgress | null>(null);
  const [startedAt, setStartedAt] = useState<number | null>(null);
  const [downloading, setDownloading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let unlisten: (() => void) | undefined;

    void listen<ModelProgress>("model-download-progress", (event) => {
      setProgress(event.payload);
    })
      .then((dispose) => {
        unlisten = dispose;
      })
      .catch(() => undefined);

    return () => {
      unlisten?.();
    };
  }, []);

  const percent = useMemo(() => {
    if (!progress || progress.total === 0) {
      return 0;
    }

    return Math.min(
      100,
      Math.round((progress.downloaded / progress.total) * 100),
    );
  }, [progress]);

  const eta = useMemo(() => {
    if (!progress || !startedAt || progress.downloaded === 0) {
      return "Calculating";
    }

    const elapsedSeconds = (Date.now() - startedAt) / 1000;
    const bytesPerSecond = progress.downloaded / Math.max(elapsedSeconds, 1);
    const remainingSeconds = Math.max(
      0,
      (progress.total - progress.downloaded) / bytesPerSecond,
    );
    return formatDuration(remainingSeconds);
  }, [progress, startedAt]);

  if (!hydrated || modelDownloaded || !isTauriRuntime()) {
    return null;
  }

  async function downloadModel(precision: ModelPrecision) {
    if (downloading) {
      return;
    }

    setSelectedPrecision(precision);
    setProgress(null);
    setStartedAt(Date.now());
    setDownloading(true);
    setError(null);

    try {
      await api.ensureModelDownloaded(precision);
      markModelDownloaded(precision);
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error));
    } finally {
      setDownloading(false);
    }
  }

  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-neutral-950/45 p-4 backdrop-blur-sm">
      <section
        aria-labelledby="model-download-title"
        aria-modal="true"
        className="w-full max-w-xl rounded-md border border-neutral-200 bg-white p-5 shadow-xl dark:border-neutral-800 dark:bg-neutral-950"
        role="dialog"
      >
        <div className="flex items-start gap-3">
          <div className="grid size-11 shrink-0 place-items-center rounded-md bg-brand-50 text-brand-700 dark:bg-neutral-900 dark:text-brand-500">
            {downloading ? (
              <Loader2 className="size-5 animate-spin" aria-hidden="true" />
            ) : (
              <Download className="size-5" aria-hidden="true" />
            )}
          </div>
          <div className="min-w-0">
            <h2 id="model-download-title" className="text-xl font-semibold">
              Download Speech Model
            </h2>
            <p className="mt-1 text-sm text-neutral-600 dark:text-neutral-400">
              Johnny Reader stores the model locally for offline playback.
            </p>
          </div>
        </div>

        <div className="mt-5 grid gap-3 sm:grid-cols-2">
          {MODEL_OPTIONS.map((option) => {
            const selected = selectedPrecision === option.precision;
            const disabled = downloading && !selected;
            return (
              <button
                className="flex min-h-32 flex-col justify-between rounded-md border border-neutral-200 p-4 text-left hover:border-brand-500 focus:outline-none focus:ring-2 focus:ring-brand-500 disabled:cursor-not-allowed disabled:opacity-50 dark:border-neutral-800 dark:hover:border-brand-500"
                disabled={disabled}
                key={option.precision}
                onClick={() => void downloadModel(option.precision)}
                type="button"
              >
                <span className="flex items-start justify-between gap-3">
                  <span>
                    <span className="block text-sm font-semibold">
                      {option.label}
                    </span>
                    <span className="mt-1 block text-sm text-neutral-500 dark:text-neutral-400">
                      {option.detail}
                    </span>
                  </span>
                  {selected && downloading ? (
                    <Loader2
                      className="size-4 shrink-0 animate-spin text-brand-700"
                      aria-hidden="true"
                    />
                  ) : selected ? (
                    <CheckCircle2
                      className="size-4 shrink-0 text-brand-700"
                      aria-hidden="true"
                    />
                  ) : null}
                </span>
                <span className="mt-4 text-xs font-medium uppercase text-neutral-500 dark:text-neutral-400">
                  {option.precision} · {option.size}
                </span>
              </button>
            );
          })}
        </div>

        {downloading ? (
          <div className="mt-5">
            <div className="h-2 overflow-hidden rounded-full bg-neutral-100 dark:bg-neutral-800">
              <div
                className="h-full bg-brand-700 transition-[width]"
                style={{ width: `${percent}%` }}
              />
            </div>
            <div className="mt-2 flex flex-wrap items-center justify-between gap-2 text-sm text-neutral-600 dark:text-neutral-400">
              <span>
                {progress
                  ? `${formatBytes(progress.downloaded)} / ${formatBytes(progress.total)}`
                  : "Connecting"}
              </span>
              <span>{progress ? `${percent}% · ${eta}` : "Preparing"}</span>
            </div>
          </div>
        ) : null}

        {error ? (
          <p className="mt-4 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700 dark:border-red-950 dark:bg-red-950/30 dark:text-red-300">
            {error}
          </p>
        ) : null}
      </section>
    </div>
  );
}

function formatBytes(value: number) {
  if (value <= 0) {
    return "0 MB";
  }

  return `${(value / 1_000_000).toFixed(1)} MB`;
}

function formatDuration(seconds: number) {
  if (!Number.isFinite(seconds)) {
    return "Calculating";
  }

  if (seconds < 60) {
    return `${Math.max(1, Math.round(seconds))}s`;
  }

  const minutes = Math.floor(seconds / 60);
  const remainder = Math.round(seconds % 60);
  return `${minutes}m ${remainder}s`;
}
