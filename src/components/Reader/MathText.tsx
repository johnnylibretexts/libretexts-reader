import katex from "katex";
import { parseMathContent, sanitizeMathml } from "../../lib/mathContent";

interface MathTextProps {
  text: string;
}

export function MathText({ text }: MathTextProps) {
  return (
    <>
      {parseMathContent(text).map((segment, index) => {
        if (segment.kind === "text") {
          return <span key={index}>{segment.text}</span>;
        }

        if (segment.kind === "mathml") {
          const safeMarkup = sanitizeMathml(segment.markup);
          if (!safeMarkup) {
            return <span key={index}>equation</span>;
          }

          return (
            <span
              className={`reader-math ${
                segment.display ? "reader-math-block" : "reader-math-inline"
              }`}
              dangerouslySetInnerHTML={{ __html: safeMarkup }}
              key={index}
            />
          );
        }

        return (
          <span
            className={`reader-math ${
              segment.display ? "reader-math-block" : "reader-math-inline"
            }`}
            dangerouslySetInnerHTML={{
              __html: katex.renderToString(segment.text, {
                displayMode: segment.display,
                output: "htmlAndMathml",
                strict: "ignore",
                throwOnError: false,
                trust: false,
              }),
            }}
            key={index}
          />
        );
      })}
    </>
  );
}
