import { sentenceText, usePlayerStore } from "../../stores/player";
import type * as Domain from "../../types/domain";

interface ParagraphViewProps {
  paragraph: Domain.Paragraph;
  paragraphIndex: number;
}

export function ParagraphView({
  paragraph,
  paragraphIndex,
}: ParagraphViewProps) {
  const currentParagraphIndex = usePlayerStore(
    (state) => state.currentParagraphIndex,
  );
  const currentSentenceIndex = usePlayerStore(
    (state) => state.currentSentenceIndex,
  );
  const seekToSentence = usePlayerStore((state) => state.seekToSentence);
  const sentenceIndexes =
    paragraph.sentenceOffsets.length > 0
      ? paragraph.sentenceOffsets.map((_, index) => index)
      : [0];

  return (
    <p className="text-lg leading-8 text-neutral-800 dark:text-neutral-200">
      {sentenceIndexes.map((sentenceIndex) => {
        const active =
          currentParagraphIndex === paragraphIndex &&
          currentSentenceIndex === sentenceIndex;
        const handleSelect = () =>
          void seekToSentence(paragraphIndex, sentenceIndex);
        return (
          <span
            className={`box-decoration-clone rounded-sm align-baseline underline-offset-4 focus:outline-none focus:ring-2 focus:ring-brand-500 ${
              active
                ? "bg-brand-50 text-brand-700 underline dark:bg-brand-950/50 dark:text-brand-300"
                : "cursor-pointer hover:bg-stone-100 dark:hover:bg-neutral-800"
            }`}
            key={sentenceIndex}
            onClick={handleSelect}
            onKeyDown={(event) => {
              if (event.key === "Enter" || event.key === " ") {
                event.preventDefault();
                handleSelect();
              }
            }}
            role="button"
            tabIndex={0}
          >
            {sentenceText(paragraph, sentenceIndex)}{" "}
          </span>
        );
      })}
    </p>
  );
}
