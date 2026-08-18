import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { MathText } from "./MathText";

/** LaTeX as the import pipeline encodes it, recovered from a Pressbooks equation. */
function latexToken(latex: string) {
  return `[[latex:${btoa(latex)}]]`;
}

describe("MathText", () => {
  it("typesets an equation a Pressbooks import recovered from a picture", () => {
    const { container } = render(
      <MathText text={`The ratio is ${latexToken("\\(\\Theta=2\\)")} exactly.`} />,
    );

    expect(container.querySelector(".katex")).not.toBeNull();
    expect(container.textContent).not.toContain("[[latex:");
    expect(container.textContent).toContain("The ratio is");
    expect(container.textContent).toContain("exactly.");
  });

  it("sets a displayed equation on its own line and an inline one in the sentence", () => {
    const { container: displayed } = render(
      <MathText text={latexToken("\\[ E = mc^2 \\]")} />,
    );
    expect(displayed.querySelector(".reader-math-block")).not.toBeNull();

    const { container: inline } = render(<MathText text={latexToken("\\(y\\)")} />);
    expect(inline.querySelector(".reader-math-inline")).not.toBeNull();
  });

  it("shows a word rather than markup when the token will not decode", () => {
    render(<MathText text="Given [[latex:A]]." />);

    expect(screen.getByText(/Given equation\./)).toBeInTheDocument();
  });

  it("still renders LaTeX a source served inline, which is how LibreTexts sends it", () => {
    // An expression container, not a quoted attribute: JSX passes attribute
    // strings through verbatim, so `\\(` would reach the component as two
    // characters and read as an escaped delimiter.
    const { container } = render(
      <MathText text={"Given \\(x = 2\\) we continue."} />,
    );

    expect(container.querySelector(".katex")).not.toBeNull();
  });
});
