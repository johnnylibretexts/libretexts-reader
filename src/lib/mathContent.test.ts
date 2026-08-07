import { describe, expect, it } from "vitest";
import { parseMathContent, sanitizeMathml } from "./mathContent";

/** `<math><mi>x</mi></math>` as the import pipeline encodes it. */
function mathmlToken(markup: string) {
  return `[[mathml:${btoa(markup)}]]`;
}

describe("sanitizeMathml", () => {
  it("keeps legitimate MathML intact", () => {
    const sanitized = sanitizeMathml("<math><mrow><mi>x</mi><mo>=</mo><mn>2</mn></mrow></math>");

    expect(sanitized).toContain("<mi>x</mi>");
    expect(sanitized).toContain("<mn>2</mn>");
  });

  it("rejects markup whose root is not math", () => {
    expect(sanitizeMathml("<div><mi>x</mi></div>")).toBeNull();
  });

  it("drops a script smuggled inside math", () => {
    const sanitized = sanitizeMathml(
      "<math><mi>x</mi><script>alert(1)</script></math>",
    );

    expect(sanitized).not.toBeNull();
    expect(sanitized).not.toContain("script");
    expect(sanitized).not.toContain("alert");
  });

  it("strips event-handler attributes while keeping the element", () => {
    const sanitized = sanitizeMathml(
      '<math><mi onclick="alert(1)" onmouseover="steal()">x</mi></math>',
    );

    expect(sanitized).toContain("<mi>x</mi>");
    expect(sanitized).not.toContain("onclick");
    expect(sanitized).not.toContain("onmouseover");
  });

  it("strips an href that could carry javascript:", () => {
    const sanitized = sanitizeMathml(
      '<math><mi href="javascript:alert(1)">x</mi></math>',
    );

    expect(sanitized).not.toContain("javascript:");
  });

  it("sanitizes deeply nested children, not just the top level", () => {
    const sanitized = sanitizeMathml(
      '<math><mrow><mrow><mi onclick="alert(1)">x</mi><iframe src="evil"></iframe></mrow></mrow></math>',
    );

    expect(sanitized).not.toBeNull();
    expect(sanitized).not.toContain("onclick");
    expect(sanitized).not.toContain("iframe");
    expect(sanitized).toContain("<mi>x</mi>");
  });

  it("returns null for markup that does not parse", () => {
    expect(sanitizeMathml("<math><mi>x</mi>")).toBeNull();
  });
});

describe("parseMathContent", () => {
  it("returns plain text as a single segment", () => {
    const segments = parseMathContent("No mathematics here.");

    expect(segments).toHaveLength(1);
    expect(segments[0].kind).toBe("text");
  });

  it("splits a mathml token out of the surrounding sentence", () => {
    const segments = parseMathContent(
      `Given ${mathmlToken("<math><mi>x</mi></math>")} we continue.`,
    );

    expect(segments.map((segment) => segment.kind)).toEqual(["text", "mathml", "text"]);
  });

  it("recognises LaTeX delimiters, which is how LibreTexts serves math", () => {
    const inline = parseMathContent("Given \\(x = 2\\) we continue.");
    expect(inline.map((segment) => segment.kind)).toEqual(["text", "tex", "text"]);

    const display = parseMathContent("Result: \\[E = mc^2\\]");
    expect(display.some((segment) => segment.kind === "tex")).toBe(true);
  });

  it("leaves a lone dollar sign alone", () => {
    const segments = parseMathContent("It cost $5 to make.");

    expect(segments.every((segment) => segment.kind === "text")).toBe(true);
  });
});
