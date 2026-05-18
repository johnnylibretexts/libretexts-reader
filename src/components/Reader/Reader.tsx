import { useEffect } from "react";
import { BookOpen, Loader2 } from "lucide-react";
import { usePlayerStore } from "../../stores/player";
import { SupertonicChapterExport } from "./SupertonicChapterExport";
import { ParagraphView } from "./ParagraphView";
import { ReaderHeader } from "./ReaderHeader";

interface ReaderProps {
  documentId: string | null;
}

export function Reader({ documentId }: ReaderProps) {
  const document = usePlayerStore((state) => state.document);
  const paragraphs = usePlayerStore((state) => state.paragraphs);
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
            {paragraphs.map((paragraph, index) => (
              <ParagraphView
                key={paragraph.id}
                paragraph={paragraph}
                paragraphIndex={index}
              />
            ))}
          </div>
        </>
      ) : null}
    </section>
  );
}
