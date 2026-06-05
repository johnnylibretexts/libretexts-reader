export type MathSegment =
  | { kind: "text"; text: string }
  | { kind: "tex"; text: string; display: boolean }
  | { kind: "mathml"; markup: string; display: boolean };

const MATHML_TOKEN_RE = /\[\[mathml:([A-Za-z0-9+/=]+)\]\]/g;

const GREEK_COMMANDS: Record<string, string> = {
  alpha: "alpha",
  beta: "beta",
  gamma: "gamma",
  delta: "delta",
  epsilon: "epsilon",
  zeta: "zeta",
  eta: "eta",
  theta: "theta",
  iota: "iota",
  kappa: "kappa",
  lambda: "lambda",
  mu: "mu",
  nu: "nu",
  xi: "xi",
  pi: "pi",
  rho: "rho",
  sigma: "sigma",
  tau: "tau",
  upsilon: "upsilon",
  phi: "phi",
  chi: "chi",
  psi: "psi",
  omega: "omega",
  Gamma: "capital gamma",
  Delta: "capital delta",
  Theta: "capital theta",
  Lambda: "capital lambda",
  Xi: "capital xi",
  Pi: "capital pi",
  Sigma: "capital sigma",
  Phi: "capital phi",
  Psi: "capital psi",
  Omega: "capital omega",
};

const COMMAND_WORDS: Record<string, string> = {
  cdot: ", times,",
  circ: "circle",
  div: "divided by",
  ge: ", greater than or equal to,",
  geq: ", greater than or equal to,",
  in: "in",
  infty: "infinity",
  le: ", less than or equal to,",
  leq: ", less than or equal to,",
  left: "",
  ln: "natural log",
  log: "log",
  neq: ", not equal to,",
  pm: ", plus or minus,",
  right: "",
  sin: "sine",
  cos: "cosine",
  tan: "tangent",
  times: ", times,",
  to: "to",
};

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

export function mathContentToSpeech(text: string): string {
  return normalizeSpeech(
    parseMathContent(text)
      .map((segment) => {
        if (segment.kind === "text") {
          return segment.text;
        }
        if (segment.kind === "mathml") {
          return withMathPauses(mathmlToSpeech(segment.markup));
        }
        return withMathPauses(latexToSpeech(segment.text));
      })
      .join(" "),
  );
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

function latexToSpeech(source: string): string {
  let text = source
    .replace(/\\label\s*\{[^{}]*\}/g, " ")
    .replace(/\\(?:,|;|:|!|quad|qquad)/g, " ")
    .replace(/\\left\b/g, " ")
    .replace(/\\right\b/g, " ");

  text = replaceLatexCommandGroups(text, "frac", 2, ([top, bottom]) => {
    return `${latexToSpeech(top)}, over, ${latexToSpeech(bottom)}`;
  });
  text = replaceLatexCommandGroups(text, "sqrt", 1, ([value]) => {
    return `square root of, ${latexToSpeech(value)}`;
  });
  text = replaceLatexCommandGroups(text, "vec", 1, ([value]) => {
    return `vector ${latexToSpeech(value)}`;
  });
  text = replaceLatexCommandGroups(text, "bar", 1, ([value]) => {
    return `${latexToSpeech(value)} bar`;
  });
  text = replaceLatexCommandGroups(text, "hat", 1, ([value]) => {
    return `${latexToSpeech(value)} hat`;
  });

  return normalizeSpeech(readLatexCharacters(text));
}

function replaceLatexCommandGroups(
  text: string,
  command: string,
  groupCount: number,
  replacement: (groups: string[]) => string,
) {
  let cursor = 0;
  let output = "";
  const pattern = `\\${command}`;

  while (cursor < text.length) {
    const index = text.indexOf(pattern, cursor);
    if (index < 0) {
      output += text.slice(cursor);
      break;
    }

    const groups: string[] = [];
    let groupCursor = index + pattern.length;
    let matched = true;
    for (let groupIndex = 0; groupIndex < groupCount; groupIndex += 1) {
      groupCursor = skipWhitespace(text, groupCursor);
      const group = readLatexGroup(text, groupCursor);
      if (!group) {
        matched = false;
        break;
      }
      groups.push(group.value);
      groupCursor = group.end;
    }

    if (!matched) {
      output += text.slice(cursor, index + pattern.length);
      cursor = index + pattern.length;
      continue;
    }

    output += text.slice(cursor, index);
    output += ` ${replacement(groups)} `;
    cursor = groupCursor;
  }

  return output;
}

function readLatexCharacters(text: string) {
  const words: string[] = [];
  let cursor = 0;

  while (cursor < text.length) {
    const character = text[cursor];

    if (/\s/.test(character)) {
      cursor += 1;
      continue;
    }

    if (character === "\\") {
      const command = readCommand(text, cursor + 1);
      if (command) {
        words.push(commandToSpeech(command.name));
        cursor = command.end;
        continue;
      }
      cursor += 1;
      continue;
    }

    if (character === "_" || character === "^") {
      const script = readScript(text, cursor + 1);
      const scriptSpeech = latexToSpeech(script.value);
      words.push(
        character === "_"
          ? `sub ${scriptSpeech},`
          : `${exponentToSpeech(scriptSpeech)},`,
      );
      cursor = script.end;
      continue;
    }

    if (character === "{") {
      const group = readLatexGroup(text, cursor);
      if (group) {
        words.push(latexToSpeech(group.value));
        cursor = group.end;
        continue;
      }
    }

    if (/[A-Za-z0-9]/.test(character)) {
      const token = readAlphanumericToken(text, cursor);
      words.push(mathIdentifierToSpeech(token.value));
      cursor = token.end;
      continue;
    }

    words.push(symbolToSpeech(character));
    cursor += 1;
  }

  return words.join(" ");
}

function readAlphanumericToken(text: string, start: number) {
  let end = start;
  while (end < text.length && /[A-Za-z0-9]/.test(text[end])) {
    end += 1;
  }
  return { value: text.slice(start, end), end };
}

function mathIdentifierToSpeech(value: string) {
  if (/^[A-Z]{2,}$/.test(value)) {
    return value.split("").join(" ");
  }
  return value;
}

function readCommand(text: string, start: number) {
  const match = /^[A-Za-z]+/.exec(text.slice(start));
  if (!match) {
    return null;
  }

  return {
    name: match[0],
    end: start + match[0].length,
  };
}

function readScript(text: string, start: number) {
  const cursor = skipWhitespace(text, start);
  const group = readLatexGroup(text, cursor);
  if (group) {
    return group;
  }

  const command = text[cursor] === "\\" ? readCommand(text, cursor + 1) : null;
  if (command) {
    return { value: `\\${command.name}`, end: command.end };
  }

  return { value: text[cursor] ?? "", end: Math.min(text.length, cursor + 1) };
}

function readLatexGroup(text: string, start: number) {
  if (text[start] !== "{") {
    return null;
  }

  let depth = 0;
  for (let cursor = start; cursor < text.length; cursor += 1) {
    if (text[cursor] === "{" && !isEscaped(text, cursor)) {
      depth += 1;
    } else if (text[cursor] === "}" && !isEscaped(text, cursor)) {
      depth -= 1;
      if (depth === 0) {
        return {
          value: text.slice(start + 1, cursor),
          end: cursor + 1,
        };
      }
    }
  }

  return null;
}

function skipWhitespace(text: string, start: number) {
  let cursor = start;
  while (cursor < text.length && /\s/.test(text[cursor])) {
    cursor += 1;
  }
  return cursor;
}

function commandToSpeech(command: string) {
  return GREEK_COMMANDS[command] ?? COMMAND_WORDS[command] ?? command;
}

function symbolToSpeech(symbol: string) {
  if (/[\w]/.test(symbol)) {
    return symbol;
  }

  const symbols: Record<string, string> = {
    "=": ", equals,",
    "+": ", plus,",
    "-": "minus",
    "*": ", times,",
    "/": "over",
    "<": ", less than,",
    ">": ", greater than,",
    "(": "open parenthesis",
    ")": "close parenthesis",
    "[": "open bracket",
    "]": "close bracket",
    ",": "comma",
    ".": "point",
    "'": "prime,",
    "|": "vertical bar",
  };

  return symbols[symbol] ?? symbol;
}

function mathmlToSpeech(markup: string): string {
  if (typeof DOMParser === "undefined") {
    return "equation";
  }

  const document = new DOMParser().parseFromString(markup, "application/xml");
  if (document.querySelector("parsererror")) {
    return "equation";
  }

  return normalizeSpeech(readMathmlNode(document.documentElement));
}

function readMathmlNode(node: Node | null): string {
  if (!node) {
    return "";
  }

  if (node.nodeType === Node.TEXT_NODE) {
    return node.textContent ?? "";
  }

  if (node.nodeType !== Node.ELEMENT_NODE) {
    return "";
  }

  const element = node as Element;
  const children = Array.from(element.childNodes);
  const childSpeech = () => children.map(readMathmlNode).join(" ");

  switch (element.localName.toLowerCase()) {
    case "math":
    case "mrow":
    case "semantics":
      return childSpeech();
    case "annotation":
    case "annotation-xml":
      return "";
    case "mi":
    case "mn":
    case "mtext":
      return element.textContent ?? "";
    case "mo":
      return symbolToSpeech((element.textContent ?? "").trim());
    case "msub":
      return `${readMathmlNode(children[0])} sub ${readMathmlNode(children[1])}`;
    case "msup":
      return `${readMathmlNode(children[0])} ${exponentToSpeech(
        readMathmlNode(children[1]),
      )}`;
    case "msubsup":
      return `${readMathmlNode(children[0])} sub ${readMathmlNode(
        children[1],
      )} ${exponentToSpeech(readMathmlNode(children[2]))}`;
    case "mfrac":
      return `${readMathmlNode(children[0])} over ${readMathmlNode(children[1])}`;
    case "msqrt":
      return `square root of ${childSpeech()}`;
    case "mroot":
      return `${readMathmlNode(children[1])} root of ${readMathmlNode(
        children[0],
      )}`;
    case "mover":
      return `${readMathmlNode(children[0])} over ${readMathmlNode(children[1])}`;
    case "munder":
      return `${readMathmlNode(children[0])} under ${readMathmlNode(children[1])}`;
    case "munderover":
      return `${readMathmlNode(children[0])} from ${readMathmlNode(
        children[1],
      )} to ${readMathmlNode(children[2])}`;
    case "mtable":
      return children.map(readMathmlNode).join("; ");
    case "mtr":
      return children.map(readMathmlNode).join(", ");
    case "mtd":
      return childSpeech();
    default:
      return childSpeech();
  }
}

function exponentToSpeech(value: string) {
  const normalized = normalizeSpeech(value);
  if (normalized === "2" || normalized === "two") {
    return "squared";
  }
  if (normalized === "3" || normalized === "three") {
    return "cubed";
  }
  if (normalized.startsWith("minus ")) {
    return `to the negative ${normalized.slice("minus ".length)} power`;
  }
  return `to the ${normalized} power`;
}

function normalizeSpeech(text: string) {
  return text
    .replace(/\s+/g, " ")
    .replace(/\s+([,.;:!?])/g, "$1")
    .replace(/,+/g, ",")
    .replace(/,\s*([.!?])/g, "$1")
    .replace(/^,\s*/g, "")
    .trim();
}

function withMathPauses(text: string) {
  const normalized = normalizeSpeech(text);
  return normalized ? `, ${normalized},` : "";
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
