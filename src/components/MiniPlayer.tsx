import {
  Pause,
  Play,
  SkipBack,
  SkipForward,
  X,
  type LucideIcon,
} from "lucide-react";
import { usePlayerStore } from "../stores/player";

interface MiniPlayerProps {
  onClose: () => void;
}

export function MiniPlayer({ onClose }: MiniPlayerProps) {
  const document = usePlayerStore((state) => state.document);
  const sections = usePlayerStore((state) => state.sections);
  const currentSectionIndex = usePlayerStore(
    (state) => state.currentSectionIndex,
  );
  const currentSentenceIndex = usePlayerStore(
    (state) => state.currentSentenceIndex,
  );
  const isPlaying = usePlayerStore((state) => state.isPlaying);
  const isBuffering = usePlayerStore((state) => state.isBuffering);
  const error = usePlayerStore((state) => state.error);
  const canSwitchToSupertonic = usePlayerStore(
    (state) => state.canSwitchToSupertonic,
  );
  const play = usePlayerStore((state) => state.play);
  const pause = usePlayerStore((state) => state.pause);
  const skipBack = usePlayerStore((state) => state.skipBack);
  const skipForward = usePlayerStore((state) => state.skipForward);
  const switchToSupertonic = usePlayerStore(
    (state) => state.switchToSupertonic,
  );

  if (!document) {
    return null;
  }

  return (
    <footer className="flex flex-col gap-2 border-t border-neutral-200 bg-white px-5 py-2 dark:border-neutral-800 dark:bg-neutral-950">
      {error ? (
        <div className="flex items-center justify-between gap-3 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700 dark:border-red-950 dark:bg-red-950/30 dark:text-red-300">
          <span className="truncate">{error}</span>
          {canSwitchToSupertonic ? (
            <button
              className="shrink-0 rounded-md border border-red-300 px-2 py-1 text-xs font-medium text-red-700 hover:bg-red-100 focus:outline-none focus:ring-2 focus:ring-brand-500 dark:border-red-800 dark:text-red-200 dark:hover:bg-red-900/40"
              type="button"
              onClick={() => void switchToSupertonic()}
            >
              Switch to Supertonic
            </button>
          ) : null}
        </div>
      ) : null}

      <div className="flex min-h-16 items-center justify-between gap-4">
        <div className="min-w-0">
          <p className="truncate text-sm font-medium">{document.title}</p>
          <p className="truncate text-xs text-neutral-500 dark:text-neutral-400">
            {sections[currentSectionIndex]?.title ?? "Section"} · Sentence{" "}
            {currentSentenceIndex + 1}
          </p>
        </div>

        <div className="flex items-center gap-2">
          <IconButton
            disabled={isBuffering}
            label="Back"
            icon={SkipBack}
            onClick={() => void skipBack()}
          />
          <IconButton
            disabled={isBuffering}
            label={isPlaying ? "Pause" : "Play"}
            icon={isPlaying ? Pause : Play}
            onClick={() => (isPlaying ? pause() : void play())}
            primary
          />
          <IconButton
            disabled={isBuffering}
            label="Forward"
            icon={SkipForward}
            onClick={() => void skipForward()}
          />
        </div>

        <button
          className="grid size-9 shrink-0 place-items-center rounded-md text-neutral-500 hover:bg-stone-100 focus:outline-none focus:ring-2 focus:ring-brand-500 dark:text-neutral-400 dark:hover:bg-neutral-900"
          type="button"
          onClick={onClose}
          aria-label="Close mini-player"
          title="Close"
        >
          <X className="size-4" aria-hidden="true" />
        </button>
      </div>
    </footer>
  );
}

function IconButton({
  icon: Icon,
  disabled,
  label,
  onClick,
  primary = false,
}: {
  disabled: boolean;
  icon: LucideIcon;
  label: string;
  onClick: () => void;
  primary?: boolean;
}) {
  return (
    <button
      className={`grid size-9 place-items-center rounded-md focus:outline-none focus:ring-2 focus:ring-brand-500 ${
        primary
          ? "bg-brand-700 text-white hover:bg-brand-500"
          : "border border-neutral-200 text-neutral-600 hover:bg-stone-100 dark:border-neutral-800 dark:text-neutral-300 dark:hover:bg-neutral-900"
      }`}
      disabled={disabled}
      onClick={onClick}
      type="button"
      aria-label={label}
      title={label}
    >
      <Icon className="size-4" aria-hidden="true" />
    </button>
  );
}
