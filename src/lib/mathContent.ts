export type MathSegment =
  | { kind: "text"; text: string }
  | { kind: "tex"; text: string; display: boolean }
  | { kind: "mathml"; markup: string; display: boolean };

const MATHML_TOKEN_RE = /\[\[mathml:([A-Za-z0-9+/=]+)\]\]/g;

export function parseMathContent(text: string): MathSegment[] {
  const segments: MathSegment[] = [];
  let cursor = 0;

  while (cursor < text.length) {
    const mathml = findNextMathmlToken(text, cursor);
    const tex = findNextTexSegment(text, cursor);
    const next =
      mathml && tex
        ? mathml.start <= tex.start
          ? mathml
          : tex
        : mathml ?? tex;

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
