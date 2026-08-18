export type MathSegment =
  | { kind: "text"; text: string }
  | { kind: "tex"; text: string; display: boolean }
  | { kind: "mathml"; markup: string; display: boolean };

const MATHML_TOKEN_RE = /\[\[mathml:([A-Za-z0-9+/=]+)\]\]/g;
const LATEX_TOKEN_RE = /\[\[latex:([A-Za-z0-9+/=]+)\]\]/g;
const LATEX_ENVIRONMENT_RE = /\\begin\s*\{/;

/**
 * A stretch of `text` that is mathematics rather than prose, and how it is
 * written: as a MathML token, as a LaTeX token, or as LaTeX the source served
 * inline between delimiters.
 */
type MathCandidate =
  | { kind: "mathml"; start: number; end: number; markup: string }
  | { kind: "latex"; start: number; end: number; latex: string | null }
  | { kind: "tex"; start: number; end: number; text: string; display: boolean };

export function parseMathContent(text: string): MathSegment[] {
  const segments: MathSegment[] = [];
  let cursor = 0;

  while (cursor < text.length) {
    const next = nextMathCandidate(text, cursor);

    if (!next) {
      pushTextSegment(segments, text.slice(cursor));
      break;
    }

    if (next.start > cursor) {
      pushTextSegment(segments, text.slice(cursor, next.start));
    }

    if (next.kind === "mathml") {
      segments.push({
        kind: "mathml",
        markup: next.markup,
        display: mathmlDisplayMode(next.markup),
      });
    } else if (next.kind === "latex") {
      const math = next.latex === null ? null : texFromSource(next.latex);
      if (math) {
        segments.push({ kind: "tex", text: math.text, display: math.display });
      } else {
        // The token is consumed either way. A payload that will not decode is
        // a defect in the import, and showing the reader its markup makes a
        // silent one loud without making it any more legible.
        pushTextSegment(segments, "equation");
      }
    } else {
      segments.push({ kind: "tex", text: next.text, display: next.display });
    }
    cursor = next.end;
  }

  return segments.length > 0 ? segments : [{ kind: "text", text }];
}

export function sanitizeMathml(markup: string): string | null {
  if (typeof DOMParser === "undefined") {
    return null;
  }

  const document = new DOMParser().parseFromString(markup, "application/xml");
  if (document.querySelector("parsererror")) {
    return null;
  }

  const root = document.documentElement;
  if (!root || root.localName.toLowerCase() !== "math") {
    return null;
  }

  if (!sanitizeMathmlElement(root)) {
    return null;
  }

  return new XMLSerializer().serializeToString(root);
}

/**
 * The earliest mathematics at or after `start`. Ties go to the token forms,
 * whose payloads are opaque and so cannot be scanned for delimiters.
 */
function nextMathCandidate(text: string, start: number): MathCandidate | null {
  const candidates = [
    findNextMathmlToken(text, start),
    findNextLatexToken(text, start),
    findNextTexSegment(text, start),
  ].filter((candidate): candidate is MathCandidate => candidate !== null);

  return candidates.reduce<MathCandidate | null>(
    (earliest, candidate) =>
      earliest === null || candidate.start < earliest.start ? candidate : earliest,
    null,
  );
}

function findNextMathmlToken(text: string, start: number) {
  MATHML_TOKEN_RE.lastIndex = start;
  const match = MATHML_TOKEN_RE.exec(text);
  if (!match) {
    return null;
  }

  const markup = decodeBase64(match[1]);
  if (!markup) {
    return null;
  }

  return {
    kind: "mathml" as const,
    start: match.index,
    end: match.index + match[0].length,
    markup,
  };
}

/**
 * Unlike the MathML token, this is returned even when its payload will not
 * decode, so that the token is always consumed rather than left in the prose.
 */
function findNextLatexToken(text: string, start: number) {
  LATEX_TOKEN_RE.lastIndex = start;
  const match = LATEX_TOKEN_RE.exec(text);
  if (!match) {
    return null;
  }

  return {
    kind: "latex" as const,
    start: match.index,
    end: match.index + match[0].length,
    latex: decodeBase64(match[1]),
  };
}

/**
 * LaTeX as the publisher wrote it, read for the mode it asks to be set in.
 */
function texFromSource(latex: string) {
  const source = latex.trim();
  if (!source) {
    return null;
  }

  const delimited = texSegmentAt(source, 0);
  if (delimited && delimited.end === source.length) {
    return { text: delimited.text, display: delimited.display };
  }

  // Nothing delimits it, so nothing states the mode. A named environment is
  // displayed mathematics; anything else is a fragment sitting in a line of
  // prose, and setting it on its own line would break the sentence in half.
  return { text: source, display: LATEX_ENVIRONMENT_RE.test(source) };
}

function findNextTexSegment(text: string, start: number) {
  for (let index = start; index < text.length; index += 1) {
    const candidate = texSegmentAt(text, index);
    if (candidate) {
      return candidate;
    }
  }

  return null;
}

function texSegmentAt(text: string, start: number) {
  const delimiters = [
    { open: "\\[", close: "\\]", display: true },
    { open: "\\(", close: "\\)", display: false },
    { open: "$$", close: "$$", display: true },
    { open: "$", close: "$", display: false },
  ];

  for (const delimiter of delimiters) {
    if (!text.startsWith(delimiter.open, start)) {
      continue;
    }
    if (
      delimiter.open === "$" &&
      (isEscaped(text, start) || !isSingleDollarMathStart(text, start))
    ) {
      continue;
    }

    const contentStart = start + delimiter.open.length;
    const close = findClosingDelimiter(text, delimiter.close, contentStart);
    if (close < 0) {
      continue;
    }

    const content = text.slice(contentStart, close).trim();
    if (!content || (delimiter.open === "$" && !looksLikeMath(content))) {
      continue;
    }

    return {
      kind: "tex" as const,
      start,
      end: close + delimiter.close.length,
      text: content,
      display: delimiter.display,
    };
  }

  return null;
}

function findClosingDelimiter(text: string, delimiter: string, start: number) {
  for (let index = start; index < text.length; index += 1) {
    if (text.startsWith(delimiter, index) && !isEscaped(text, index)) {
      return index;
    }
  }
  return -1;
}

function isEscaped(text: string, index: number) {
  let slashCount = 0;
  for (let cursor = index - 1; cursor >= 0 && text[cursor] === "\\"; cursor -= 1) {
    slashCount += 1;
  }
  return slashCount % 2 === 1;
}

function isSingleDollarMathStart(text: string, index: number) {
  const before = text[index - 1] ?? " ";
  const after = text[index + 1] ?? " ";
  return !/\d/.test(after) && !/\w/.test(before);
}

function looksLikeMath(text: string) {
  return /\\[A-Za-z]+|[_^=<>+\-*/]|[A-Za-z]\d|\d[A-Za-z]/.test(text);
}

function pushTextSegment(segments: MathSegment[], text: string) {
  if (!text) {
    return;
  }

  const previous = segments[segments.length - 1];
  if (previous?.kind === "text") {
    previous.text += text;
  } else {
    segments.push({ kind: "text", text });
  }
}

function decodeBase64(value: string) {
  try {
    if (typeof atob === "function") {
      const binary = atob(value);
      const bytes = Uint8Array.from(binary, (character) =>
        character.charCodeAt(0),
      );
      return new TextDecoder().decode(bytes);
    }
  } catch {
    return null;
  }
  return null;
}

function mathmlDisplayMode(markup: string) {
  return /\bdisplay\s*=\s*["']block["']/i.test(markup);
}

const ALLOWED_MATHML_TAGS = new Set([
  "annotation",
  "annotation-xml",
  "maligngroup",
  "malignmark",
  "math",
  "menclose",
  "merror",
  "mfenced",
  "mfrac",
  "mglyph",
  "mi",
  "mlabeledtr",
  "mmultiscripts",
  "mn",
  "mo",
  "mover",
  "mpadded",
  "mphantom",
  "mroot",
  "mrow",
  "ms",
  "mspace",
  "msqrt",
  "mstyle",
  "msub",
  "msubsup",
  "msup",
  "mtable",
  "mtd",
  "mtext",
  "mtr",
  "munder",
  "munderover",
  "none",
  "semantics",
]);

const ALLOWED_MATHML_ATTRIBUTES = new Set([
  "accent",
  "accentunder",
  "align",
  "columnalign",
  "columnspan",
  "depth",
  "display",
  "encoding",
  "fence",
  "form",
  "height",
  "largeop",
  "linethickness",
  "lspace",
  "mathvariant",
  "movablelimits",
  "notation",
  "rowalign",
  "rowspan",
  "rspace",
  "separator",
  "stretchy",
  "width",
  "xmlns",
]);

function sanitizeMathmlElement(element: Element): boolean {
  if (!ALLOWED_MATHML_TAGS.has(element.localName.toLowerCase())) {
    return false;
  }

  for (const attribute of Array.from(element.attributes)) {
    const name = attribute.name.toLowerCase();
    if (!ALLOWED_MATHML_ATTRIBUTES.has(name)) {
      element.removeAttribute(attribute.name);
    }
  }

  for (const child of Array.from(element.children)) {
    if (!sanitizeMathmlElement(child)) {
      child.remove();
    }
  }

  return true;
}
