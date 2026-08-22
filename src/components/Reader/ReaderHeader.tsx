import { ExternalLink, Loader2 } from "lucide-react";
import { PlaybackControls } from "./PlaybackControls";
import { ModelDownloadProgress } from "../ModelDownloadProgress";
import { usePlayerStore } from "../../stores/player";

/**
 * The attribution field as a link, or null when it is not one.
 *
 * `documents.attribution` is polymorphic by Source: OpenStax, LibreTexts and
 * article all store a URL there, while Pressbooks stores an author name
 * ("Craig DeLancey"). Rendering it one way is wrong for the other, and
 * hyperlinking a person is the worse of the two mistakes.
 */
function sourceLink(attribution: string | null) {
  if (!attribution) {
    return null;
  }
  try {
    const url = new URL(attribution);
    if (url.protocol !== "http:" && url.protocol !== "https:") {
      return null;
    }
    // The bare host and path: the full href is on the anchor, and a raw URL
    // with its scheme and query is noise on a line meant to be read.
    return { href: url.href, label: `${url.host}${url.pathname}`.replace(/\/$/, "") };
  } catch {
    return null;
  }
}

export function ReaderHeader() {
  const document = usePlayerStore((state) => state.document);
  const sections = usePlayerStore((state) => state.sections);
  const currentSectionIndex = usePlayerStore(
    (state) => state.currentSectionIndex,
  );
  const setSection = usePlayerStore((state) => state.setSection);
  const isBuffering = usePlayerStore((state) => state.isBuffering);
  const bufferingMessage = usePlayerStore((state) => state.bufferingMessage);
  const modelDownload = usePlayerStore((state) => state.modelDownload);

  if (!document) {
    return null;
  }

  const attributionLink = sourceLink(document.attribution);

  return (
    <div className="sticky top-0 z-10 -mx-4 border-b border-neutral-200 bg-stone-50/95 px-4 py-3 backdrop-blur dark:border-neutral-800 dark:bg-neutral-950/95 md:-mx-6 md:px-6">
      <div className="mx-auto flex max-w-6xl flex-col gap-3">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="min-w-0">
            <h1 className="truncate text-xl font-semibold">{document.title}</h1>
            <p className="mt-1 text-sm text-neutral-500 dark:text-neutral-400">
              {sections[currentSectionIndex]?.title ?? "Section"}
            </p>
            {/*
              CC BY 4.0 3(a)(1) attaches to the copy the reader is holding, and
              the app satisfied it nowhere after import -- the only licence on
              screen was in the pre-import catalog. Rendered on the reading
              surface, where the work is actually being consumed.

              Nothing at all when the Source supplied neither, rather than an
              empty field or a bare separator: a pasted-text import has no
              licence, and claiming otherwise is its own kind of wrong.
            */}
            {document.license || document.attribution ? (
              <p className="mt-1 flex flex-wrap items-center gap-x-1 text-xs text-neutral-500 dark:text-neutral-400">
                {document.license ? <span>{document.license}</span> : null}
                {document.license && document.attribution ? (
                  <span aria-hidden="true">·</span>
                ) : null}
                {document.attribution ? (
                  attributionLink ? (
                    <a
                      className="inline-flex items-center gap-1 text-brand-700 hover:underline dark:text-brand-500"
                      href={attributionLink.href}
                      rel="noreferrer"
                      target="_blank"
                    >
                      {attributionLink.label}
                      <ExternalLink className="size-3" aria-hidden="true" />
                    </a>
                  ) : (
                    <span>{document.attribution}</span>
                  )
                ) : null}
              </p>
            ) : null}
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
          {modelDownload ? (
            <ModelDownloadProgress />
          ) : isBuffering ? (
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
