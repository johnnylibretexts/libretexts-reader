import {
  Pause,
  Play,
  SkipBack,
  SkipForward,
  StepBack,
  StepForward,
} from "lucide-react";
import type { ReactNode } from "react";
import { usePlayerStore } from "../../stores/player";
import { useSettingsStore } from "../../stores/settings";
import { SPEECH_ENGINE_BILLS } from "../../lib/speech";
import { useTranslationStore } from "../../stores/translation";
import { ListenInControl } from "./ListenInControl";
import { ImageDescriptionPreferenceControl } from "../ImageDescriptionPreferenceControl";

export function PlaybackControls() {
  const isPlaying = usePlayerStore((state) => state.isPlaying);
  const isBuffering = usePlayerStore((state) => state.isBuffering);
  const modelDownload = usePlayerStore((state) => state.modelDownload);
  const translationRunning = useTranslationStore(
    (state) => state.sectionState.status === "running",
  );
  const engineBills =
    SPEECH_ENGINE_BILLS[useSettingsStore((state) => state.ttsProvider)];
  /**
   * Pause stays live through the one-time voice download, where it is the one
   * control that can do anything: it stops the download. Disabling it with
   * everything else is what made a ~383MB fetch read as a frozen app.
   *
   * It stays live for a billing engine too, and for the same reason turned
   * around: buffering is precisely when a burst of charged requests is in
   * flight, and Pause is what stops the queue. A disabled Pause makes "stop
   * spending" unclickable at the only moment it matters.
   *
   * Skips stay disabled either way -- there is no audio yet to skip through.
   */
  const playDisabled =
    translationRunning || (isBuffering && !modelDownload && !engineBills);
  const play = usePlayerStore((state) => state.play);
  const pause = usePlayerStore((state) => state.pause);
  const skipBack = usePlayerStore((state) => state.skipBack);
  const skipForward = usePlayerStore((state) => state.skipForward);
  const skipParagraphBack = usePlayerStore((state) => state.skipParagraphBack);
  const skipParagraphForward = usePlayerStore(
    (state) => state.skipParagraphForward,
  );
  const speed = usePlayerStore((state) => state.speed);
  const setSpeed = usePlayerStore((state) => state.setSpeed);

  return (
    <div className="flex flex-wrap items-center gap-2">
      <IconButton
        disabled={isBuffering || translationRunning}
        label="Previous paragraph"
        onClick={() => void skipParagraphBack()}
      >
        <SkipBack className="size-4" aria-hidden="true" />
      </IconButton>
      <IconButton
        disabled={isBuffering || translationRunning}
        label="Previous sentence"
        onClick={() => void skipBack()}
      >
        <StepBack className="size-4" aria-hidden="true" />
      </IconButton>
      <button
        aria-label={isPlaying ? "Pause" : "Play"}
        className="inline-flex h-10 items-center gap-2 rounded-md bg-brand-700 px-4 text-sm font-medium text-white hover:bg-brand-500 focus:outline-none focus:ring-2 focus:ring-brand-500 disabled:cursor-not-allowed disabled:opacity-50"
        disabled={playDisabled}
        onClick={() => (isPlaying ? pause() : void play())}
        title={isPlaying ? "Pause" : "Play"}
        type="button"
      >
        {isPlaying ? (
          <Pause className="size-4" aria-hidden="true" />
        ) : (
          <Play className="size-4" aria-hidden="true" />
        )}
        {isPlaying ? "Pause" : "Play"}
      </button>
      <IconButton
        disabled={isBuffering || translationRunning}
        label="Next sentence"
        onClick={() => void skipForward()}
      >
        <StepForward className="size-4" aria-hidden="true" />
      </IconButton>
      <IconButton
        disabled={isBuffering || translationRunning}
        label="Next paragraph"
        onClick={() => void skipParagraphForward()}
      >
        <SkipForward className="size-4" aria-hidden="true" />
      </IconButton>

      <div className="ml-0 md:ml-3">
        <ListenInControl />
      </div>

      <div className="ml-0 md:ml-1">
        <ImageDescriptionPreferenceControl compact />
      </div>

      <label className="ml-0 flex items-center gap-2 text-sm text-neutral-600 dark:text-neutral-400 md:ml-3">
        Speed
        <input
          className="h-2 w-28 accent-brand-700"
          max={2}
          min={0.5}
          onChange={(event) => setSpeed(Number(event.target.value))}
          step={0.1}
          type="range"
          value={speed}
        />
        <span className="w-9 tabular-nums">{speed.toFixed(1)}x</span>
      </label>
    </div>
  );
}

function IconButton({
  children,
  disabled,
  label,
  onClick,
}: {
  children: ReactNode;
  disabled: boolean;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      aria-label={label}
      className="grid size-10 place-items-center rounded-md border border-neutral-200 text-neutral-600 hover:bg-stone-100 focus:outline-none focus:ring-2 focus:ring-brand-500 disabled:cursor-not-allowed disabled:opacity-50 dark:border-neutral-800 dark:text-neutral-300 dark:hover:bg-neutral-800"
      disabled={disabled}
      onClick={onClick}
      title={label}
      type="button"
    >
      {children}
    </button>
  );
}
