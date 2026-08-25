import { useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { BookOpen, Loader2 } from "lucide-react";
import { usePlayerStore } from "../../stores/player";
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
  const setDocumentSourceLanguage = usePlayerStore(
    (state) => state.setDocumentSourceLanguage,
  );
  const translation = useTranslationStore((state) => state.sectionState);
  const cancelTranslation = useTranslationStore((state) => state.cancel);
  const [sourceLanguageSaving, setSourceLanguageSaving] = useState(false);
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
          <div className="flex flex-wrap items-end justify-between gap-3 rounded-md border border-neutral-200 bg-white px-4 py-3 dark:border-neutral-800 dark:bg-neutral-900">
            <label className="flex min-w-48 flex-col gap-1 text-sm font-medium">
              Written in
              <select
                className="h-10 rounded-md border border-neutral-200 bg-white px-3 text-sm font-normal outline-none focus:border-brand-500 focus:ring-2 focus:ring-brand-500/20 disabled:cursor-not-allowed disabled:opacity-60 dark:border-neutral-700 dark:bg-neutral-950"
                disabled={sourceLanguageSaving || translation.status === "running"}
                onChange={(event) => {
                  setSourceLanguageSaving(true);
                  void setDocumentSourceLanguage(event.target.value).finally(
                    () => setSourceLanguageSaving(false),
                  );
                }}
                value={document.sourceLanguage}
              >
                {SUPERTONIC_LANGUAGES.some(
                  (language) => language.id === document.sourceLanguage,
                ) ? null : (
                  <option value={document.sourceLanguage}>
                    {languageName(document.sourceLanguage)}
                  </option>
                )}
                {SUPERTONIC_LANGUAGES.map((language) => (
                  <option key={language.id} value={language.id}>
                    {language.name}
                  </option>
                ))}
              </select>
            </label>
            <p className="max-w-2xl text-xs leading-5 text-neutral-500 dark:text-neutral-400">
              This identifies the book&apos;s original language. The page stays
              in this language even when spoken narration is translated.
            </p>
          </div>

          {translation.status === "running" ? (
            <div className="rounded-md border border-brand-200 bg-brand-50 px-4 py-3 text-sm dark:border-brand-950 dark:bg-brand-950/30">
              <div className="flex flex-wrap items-center justify-between gap-3">
                <span>
                  Translating chapter: {translation.done} of {translation.total}
                  {" sentences"}
                </span>
                <button
                  className="rounded-md border border-brand-300 px-3 py-1.5 text-sm font-medium hover:bg-brand-100 focus:outline-none focus:ring-2 focus:ring-brand-500 dark:border-brand-800 dark:hover:bg-brand-950"
                  onClick={() => void cancelTranslation()}
                  type="button"
                >
                  Cancel
                </button>
              </div>
              <progress
                aria-label="Chapter translation progress"
                className="mt-3 h-2 w-full"
                max={Math.max(translation.total, 1)}
                value={translation.done}
              />
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
