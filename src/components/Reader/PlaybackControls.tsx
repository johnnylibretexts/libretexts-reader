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
import { type TtsProvider, useSettingsStore } from "../../stores/settings";

export function PlaybackControls() {
  const isPlaying = usePlayerStore((state) => state.isPlaying);
  const isBuffering = usePlayerStore((state) => state.isBuffering);
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
  const ttsProvider = useSettingsStore((state) => state.ttsProvider);
  const setTtsProvider = useSettingsStore((state) => state.setTtsProvider);
  async function changeProvider(provider: TtsProvider) {
    if (isPlaying) {
      pause();
    }
    await setTtsProvider(provider);
  }

  return (
    <div className="flex flex-wrap items-center gap-2">
      <IconButton
        disabled={isBuffering}
        label="Previous paragraph"
        onClick={() => void skipParagraphBack()}
      >
        <SkipBack className="size-4" aria-hidden="true" />
      </IconButton>
      <IconButton
        disabled={isBuffering}
        label="Previous sentence"
        onClick={() => void skipBack()}
      >
        <StepBack className="size-4" aria-hidden="true" />
      </IconButton>
      <button
        aria-label={isPlaying ? "Pause" : "Play"}
        className="inline-flex h-10 items-center gap-2 rounded-md bg-brand-700 px-4 text-sm font-medium text-white hover:bg-brand-500 focus:outline-none focus:ring-2 focus:ring-brand-500 disabled:cursor-not-allowed disabled:opacity-50"
        disabled={isBuffering}
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
        disabled={isBuffering}
        label="Next sentence"
        onClick={() => void skipForward()}
      >
        <StepForward className="size-4" aria-hidden="true" />
      </IconButton>
      <IconButton
        disabled={isBuffering}
        label="Next paragraph"
        onClick={() => void skipParagraphForward()}
      >
        <SkipForward className="size-4" aria-hidden="true" />
      </IconButton>

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

      <label className="ml-0 flex items-center gap-2 text-sm text-neutral-600 dark:text-neutral-400 md:ml-3">
        Engine
        <select
          className="h-9 rounded-md border border-neutral-200 bg-white px-2 text-sm font-normal outline-none focus:border-brand-500 focus:ring-2 focus:ring-brand-500/20 disabled:cursor-not-allowed disabled:opacity-50 dark:border-neutral-800 dark:bg-neutral-900"
          disabled={isBuffering}
          onChange={(event) =>
            void changeProvider(event.target.value as TtsProvider)
          }
          value={ttsProvider}
        >
          <option value="kokoro">Kokoro</option>
          <option value="supertonic">Supertonic</option>
        </select>
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
