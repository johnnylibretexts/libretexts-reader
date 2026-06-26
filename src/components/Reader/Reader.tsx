import { useEffect } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { BookOpen, Loader2 } from "lucide-react";
import { usePlayerStore } from "../../stores/player";
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

  useEffect(() => {
    if (documentId && document?.id !== documentId) {
      void loadDocument(documentId);
    }
  }, [document?.id, documentId, loadDocument]);

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
