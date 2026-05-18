import { Loader2 } from "lucide-react";
import { PlaybackControls } from "./PlaybackControls";
import { usePlayerStore } from "../../stores/player";

export function ReaderHeader() {
  const document = usePlayerStore((state) => state.document);
  const sections = usePlayerStore((state) => state.sections);
  const currentSectionIndex = usePlayerStore(
    (state) => state.currentSectionIndex,
  );
  const setSection = usePlayerStore((state) => state.setSection);
  const isBuffering = usePlayerStore((state) => state.isBuffering);
  const bufferingMessage = usePlayerStore((state) => state.bufferingMessage);

  if (!document) {
    return null;
  }

  return (
    <div className="sticky top-0 z-10 -mx-4 border-b border-neutral-200 bg-stone-50/95 px-4 py-3 backdrop-blur dark:border-neutral-800 dark:bg-neutral-950/95 md:-mx-6 md:px-6">
      <div className="mx-auto flex max-w-6xl flex-col gap-3">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="min-w-0">
            <h1 className="truncate text-xl font-semibold">{document.title}</h1>
            <p className="mt-1 text-sm text-neutral-500 dark:text-neutral-400">
              {sections[currentSectionIndex]?.title ?? "Section"}
            </p>
          </div>
          <label className="flex min-w-56 flex-col gap-1 text-sm font-medium">
            Section
            <select
              className="h-10 rounded-md border border-neutral-200 bg-white px-3 text-sm font-normal outline-none focus:border-brand-500 focus:ring-2 focus:ring-brand-500/20 dark:border-neutral-800 dark:bg-neutral-900"
              disabled={isBuffering}
              onChange={(event) => void setSection(Number(event.target.value))}
              value={currentSectionIndex}
            >
              {sections.map((section, index) => (
                <option key={section.id} value={index}>
                  {section.title}
                </option>
              ))}
            </select>
          </label>
        </div>
        <div className="flex items-center justify-between gap-3">
          <PlaybackControls />
          {isBuffering ? (
            <span className="inline-flex items-center gap-2 text-sm text-neutral-500 dark:text-neutral-400">
              <Loader2 className="size-4 animate-spin" aria-hidden="true" />
              {bufferingMessage || "Loading"}
            </span>
          ) : null}
        </div>
      </div>
    </div>
  );
}
