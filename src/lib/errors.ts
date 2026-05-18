const BACKEND_ERROR_PREFIXES = [
  "tts error",
  "invalid input",
  "model error",
  "voice error",
  "openstax error",
  "libretexts error",
  "pdf error",
  "database error",
  "http error",
];

const BACKEND_ERROR_PREFIX_PATTERN = new RegExp(
  `^(${BACKEND_ERROR_PREFIXES.join("|")}):\\s*`,
  "i",
);

export function displayError(error: unknown) {
  const message = error instanceof Error ? error.message : String(error);
  return message.replace(BACKEND_ERROR_PREFIX_PATTERN, "");
}
