import { useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { BookOpen, Loader2 } from "lucide-react";
import { usePlayerStore } from "../../stores/player";
import { useSettingsStore } from "../../stores/settings";
import { useTranslationStore } from "../../stores/translation";
import { SUPERTONIC_LANGUAGES } from "../../lib/supertonic";
import { SupertonicChapterExport } from "./SupertonicChapterExport";
import { ParagraphView } from "./ParagraphView";
import { ReaderHeader } from "./ReaderHeader";
import type * as Domain from "../../types/domain";

interface ReaderProps {
  documentId: string | null;
}

export function Reader({ documentId }: ReaderProps) {
  const document = usePlayerStore((state) => state.document);
  const paragraphs = usePlayerStore((state) => state.paragraphs);
  const sectionImages = usePlayerStore((state) => state.sectionImages);
  const loading = usePlayerStore((state) => state.loading);
  const error = usePlayerStore((state) => state.error);
  const loadDocument = usePlayerStore((state) => state.loadDocument);
  const translation = useTranslationStore((state) => state.sectionState);
  const cancelTranslation = useTranslationStore((state) => state.cancel);
  const translationTargetLanguage = useSettingsStore(
    (state) => state.translationTargetLang,
  );
  const [translationCancelling, setTranslationCancelling] = useState(false);
  const currentSectionId = usePlayerStore(
    (state) => state.sections[state.currentSectionIndex]?.id ?? null,
  );
  const previousSectionId = useRef(currentSectionId);

  useEffect(() => {
    if (documentId && document?.id !== documentId) {
      void loadDocument(documentId);
    }
  }, [document?.id, documentId, loadDocument]);

  // Translation status describes one chapter. Keep the initial value on mount
  // (component tests and a route return both depend on it), but never carry a
  // completed chapter's fallback warning onto the next section.
  useEffect(() => {
    if (previousSectionId.current !== currentSectionId) {
      previousSectionId.current = currentSectionId;
      useTranslationStore.setState({
        sectionState: {
          status: "idle",
          done: 0,
          total: 0,
          fallbackCount: 0,
          sentenceCount: 0,
          error: null,
        },
      });
    }
  }, [currentSectionId]);

  useEffect(() => {
    if (translation.status !== "running") {
      setTranslationCancelling(false);
    }
  }, [translation.status]);

  const translationTotal = Math.max(translation.total, 1);
  const translationDone = Math.min(
    Math.max(translation.done, 0),
    translationTotal,
  );
  const translationPercent = Math.round(
    (translationDone / translationTotal) * 100,
  );

  if (!documentId) {
    return (
      <div className="mx-auto grid min-h-80 max-w-3xl place-items-center rounded-md border border-dashed border-neutral-300 px-6 text-center dark:border-neutral-700">
        <div>
          <BookOpen
            className="mx-auto size-8 text-brand-700 dark:text-brand-500"
            aria-hidden="true"
          />
          <p className="mt-3 text-sm text-neutral-500 dark:text-neutral-400">
            Open a document from the library.
          </p>
        </div>
      </div>
    );
  }

  return (
    <section className="mx-auto flex max-w-6xl flex-col gap-6">
      <ReaderHeader />

      {loading ? (
        <p className="inline-flex items-center gap-2 text-sm text-neutral-500 dark:text-neutral-400">
          <Loader2 className="size-4 animate-spin" aria-hidden="true" />
          Loading document
        </p>
      ) : null}

      {error ? (
        <p className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700 dark:border-red-950 dark:bg-red-950/30 dark:text-red-300">
          {error}
        </p>
      ) : null}

      {!loading && document ? (
        <>
          {translation.status === "running" ? (
            <div className="rounded-md border border-neutral-200 bg-white px-4 py-3 text-neutral-950 shadow-sm dark:border-neutral-700 dark:bg-neutral-900 dark:text-neutral-100">
              <div className="flex flex-wrap items-start justify-between gap-3">
                <div>
                  <p className="text-sm font-semibold">
                    Preparing{" "}
                    {languageName(
                      translationTargetLanguage ?? document.sourceLanguage,
                    )}{" "}
                    narration
                  </p>
                  <p className="mt-1 text-xs text-neutral-600 dark:text-neutral-300">
                    {translation.done} of {translation.total} sentences
                  </p>
                </div>
                <button
                  className="rounded-md border border-neutral-300 px-3 py-1.5 text-sm font-medium text-neutral-700 hover:bg-neutral-100 focus:outline-none focus:ring-2 focus:ring-brand-500 disabled:cursor-wait disabled:opacity-60 dark:border-neutral-700 dark:text-neutral-200 dark:hover:bg-neutral-800"
                  disabled={translationCancelling}
                  onClick={() => {
                    setTranslationCancelling(true);
                    void cancelTranslation();
                  }}
                  type="button"
                >
                  {translationCancelling ? "Stopping…" : "Cancel"}
                </button>
              </div>
              <div className="mt-3 flex items-center gap-3">
                <div
                  aria-label="Chapter translation progress"
                  aria-valuemax={translationTotal}
                  aria-valuemin={0}
                  aria-valuenow={translationDone}
                  aria-valuetext={`${translation.done} of ${translation.total} sentences, ${translationPercent} percent`}
                  className="h-2 flex-1 overflow-hidden rounded-full bg-neutral-200 dark:bg-neutral-700"
                  role="progressbar"
                >
                  <div
                    className="h-full rounded-full bg-brand-700 transition-[width] dark:bg-brand-400"
                    style={{ width: `${translationPercent}%` }}
                  />
                </div>
                <span className="w-10 text-right text-xs font-semibold tabular-nums">
                  {translationPercent}%
                </span>
              </div>
            </div>
          ) : null}

          {translation.status === "complete" &&
          translation.fallbackCount > 0 ? (
            <p className="rounded-md border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-800 dark:border-amber-950 dark:bg-amber-950/30 dark:text-amber-300">
              {translation.fallbackCount} of {translation.sentenceCount} sentences
              are read in {languageName(document.sourceLanguage)} — translation
              check failed.
            </p>
          ) : null}

          {translation.status === "failed" && translation.error ? (
            <p className="rounded-md border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700 dark:border-red-950 dark:bg-red-950/30 dark:text-red-300">
              {translation.error}
            </p>
          ) : null}

          <SupertonicChapterExport />
          <div className="space-y-5 pb-24 font-reader">
            <SectionImages
              images={imagesBeforeFirstParagraph(sectionImages)}
            />
            {paragraphs.map((paragraph, index) => (
              <div className="space-y-5" key={paragraph.id}>
                <ParagraphView
                  paragraph={paragraph}
                  paragraphIndex={index}
                />
                <SectionImages
                  images={imagesAfterParagraph(sectionImages, paragraph.ordinal)}
                />
              </div>
            ))}
            <SectionImages
              images={imagesAfterLastParagraph(sectionImages, paragraphs)}
            />
          </div>
        </>
      ) : null}
    </section>
  );
}

function languageName(code: string) {
  return (
    SUPERTONIC_LANGUAGES.find((language) => language.id === code)?.name ??
    code.toUpperCase()
  );
}

function SectionImages({ images }: { images: Domain.SectionImage[] }) {
  if (images.length === 0) {
    return null;
  }

  return (
    <div className="space-y-6">
      {images.map((image) => {
        const caption = image.caption ?? image.altText;

        return (
          <figure
            className="reader-figure"
            key={image.id}
          >
            <img
              alt={image.altText ?? image.caption ?? ""}
              className="mx-auto max-h-[34rem] max-w-full object-contain"
              loading="lazy"
              src={convertFileSrc(image.localPath)}
            />
            {caption ? (
              <figcaption className="mx-auto mt-2 max-w-3xl text-sm leading-6 text-neutral-600 dark:text-neutral-300">
                {caption}
              </figcaption>
            ) : null}
          </figure>
        );
      })}
    </div>
  );
}

function imagesBeforeFirstParagraph(images: Domain.SectionImage[]) {
  return images.filter((image) => image.anchorParagraphOrdinal === null);
}

function imagesAfterParagraph(
  images: Domain.SectionImage[],
  paragraphOrdinal: number,
) {
  return images.filter(
    (image) => image.anchorParagraphOrdinal === paragraphOrdinal,
  );
}

function imagesAfterLastParagraph(
  images: Domain.SectionImage[],
  paragraphs: Domain.Paragraph[],
) {
  // Anchors that point past the last rendered paragraph fall through to the end
  // so they are never silently dropped. Compare against the stored ordinal of
  // the last paragraph rather than the paragraph count.
  const lastOrdinal =
    paragraphs.length > 0 ? paragraphs[paragraphs.length - 1].ordinal : -1;
  return images.filter(
    (image) =>
      image.anchorParagraphOrdinal !== null &&
      image.anchorParagraphOrdinal > lastOrdinal,
  );
}
