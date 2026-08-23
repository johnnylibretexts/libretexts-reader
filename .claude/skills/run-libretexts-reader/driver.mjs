#!/usr/bin/env node
// Drive the LibreTexts Reader desktop app without touching the reader's real
// library.
//
// Every command works against a scratch app-data directory, so seeding a book,
// moving a playback cursor or deleting a document never reaches
// ~/Library/Application Support/dev.johnnylibretexts.reader.
//
// Usage: node .claude/skills/run-libretexts-reader/driver.mjs <command>
//
//   build          debug binary, no installers (~50s warm, minutes cold)
//   seed           create + migrate the scratch library, then seed one book
//   launch         start the app against the scratch library
//   bounds         print the app window's screen rectangle
//   shot <file>    screenshot the app window (not the whole screen)
//   quit           stop the app
//   demo           seed + launch + shot, end to end
//
// Clicking is NOT here on purpose -- see the Gotchas in SKILL.md. A flow that
// needs a click has to be handed to a human.

import { execFile, execFileSync, spawn } from "node:child_process";
import { existsSync, mkdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SKILL_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(SKILL_DIR, "../../..");
const BINARY = path.join(REPO, "target/debug/libretexts-reader");
const SCRATCH =
  process.env.RUN_SCRATCH ?? path.join(tmpdir(), "libretexts-reader-run");
const APP_DATA = path.join(SCRATCH, "appdata");
const DB = path.join(APP_DATA, "library.sqlite");
// `pkill -f` matches on this, so it must stay specific enough to miss a real
// installed copy of the app the reader may have running.
const PROCESS_MATCH = "target/debug/libretexts-reader";

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function sh(command, args, options = {}) {
  return execFileSync(command, args, {
    encoding: "utf8",
    stdio: options.quiet ? ["ignore", "pipe", "pipe"] : "inherit",
    cwd: REPO,
    ...options,
  });
}

function osascript(script) {
  return execFileSync("osascript", ["-e", script], {
    encoding: "utf8",
  }).trim();
}

function sqlite(sql) {
  return execFileSync("/usr/bin/sqlite3", [DB, sql], { encoding: "utf8" });
}

function appIsRunning() {
  try {
    execFileSync("pgrep", ["-f", PROCESS_MATCH], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}

function build() {
  console.log("building the debug binary (no bundle)...");
  sh("npm", ["run", "tauri", "--", "build", "--debug", "--no-bundle"]);
  console.log(`built: ${BINARY}`);
}

function startApp() {
  if (!existsSync(BINARY)) {
    throw new Error(`no binary at ${BINARY} -- run \`driver.mjs build\` first`);
  }
  // Detached, with stdio ignored: the app runs for the rest of the session and
  // must not hold this process open.
  const child = spawn(BINARY, [], {
    cwd: REPO,
    detached: true,
    stdio: "ignore",
    env: { ...process.env, LIBRETEXTS_READER_APP_DATA_DIR: APP_DATA },
  });
  child.unref();
  return child;
}

function quit() {
  if (!appIsRunning()) {
    console.log("nothing to stop");
    return;
  }
  execFile("pkill", ["-f", PROCESS_MATCH], () => {});
  console.log("stopped");
}

async function waitFor(check, { label, timeoutMs = 60_000 }) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await check()) {
      return true;
    }
    await sleep(500);
  }
  throw new Error(`timed out waiting for ${label}`);
}

/**
 * A migrated, empty scratch library.
 *
 * The app itself has to create it: `_migrations` bookkeeping is written by
 * `db::migrations`, so a database built by replaying the .sql files by hand
 * makes the next real launch try to apply them all again.
 */
async function migrateScratchLibrary() {
  rmSync(SCRATCH, { recursive: true, force: true });
  mkdirSync(APP_DATA, { recursive: true });
  console.log(`creating a scratch library at ${APP_DATA}`);
  startApp();
  await waitFor(async () => existsSync(DB), { label: "the database" });
  // The migrations run inside `setup`, before the window; give the batch a
  // moment to commit before killing the process that is writing it.
  await sleep(2500);
  quit();
  await sleep(1500);
}

/**
 * One book with a resume cursor exactly half way in.
 *
 * The word counts are chosen so the expected progress is checkable by hand:
 * three sections of four paragraphs, six words each -- 24 words a section, 72
 * in the book. A cursor on the third paragraph (ordinal 2 of 4) of the second
 * section is 24 finished words plus half of the 24 in progress, over 72.
 * Fifty percent, and the Library card's bar should be visibly half full.
 */
function seedBook() {
  const DOC = "doc-resume-demo";
  const STAMP = "2026-08-20T10:00:00+00:00";
  const chapters = ["one", "two", "three"];
  const ordinals = ["first", "second", "third", "fourth"];

  const text = (chapter, index) =>
    `This is the ${ordinals[index]} paragraph of chapter ${chapter}. ` +
    `Every paragraph here holds two plain sentences.`;

  // Byte spans of each sentence, the shape `sentence_boundaries` records.
  const offsets = (value) => {
    const spans = [];
    let start = 0;
    for (const piece of value.split(". ")) {
      let end = start + piece.length;
      if (!piece.endsWith(".")) {
        end += 1; // the period the split removed
      }
      spans.push([start, Math.min(end, value.length)]);
      start = end + 1; // skip the space
    }
    return spans;
  };

  const quote = (value) => `'${String(value).replaceAll("'", "''")}'`;
  const statements = [
    `DELETE FROM documents WHERE id = ${quote(DOC)};`,
    `INSERT INTO documents (id, title, source_type, source_metadata,
        cover_image_path, license, attribution, word_count, imported_at,
        last_opened_at)
     VALUES (${quote(DOC)}, 'The Resume Demonstration', 'openstax', '{}',
        NULL, NULL, NULL, 72, ${quote(STAMP)}, NULL);`,
  ];

  chapters.forEach((chapter, ordinal) => {
    statements.push(
      `INSERT INTO sections (id, document_id, ordinal, title, word_count)
       VALUES ('sec-${ordinal}', ${quote(DOC)}, ${ordinal},
         'Chapter ${chapter}', 24);`,
    );
    ordinals.forEach((_, index) => {
      const body = text(chapter, index);
      statements.push(
        `INSERT INTO paragraphs (id, section_id, ordinal, text, sentence_offsets)
         VALUES ('para-${ordinal}-${index}', 'sec-${ordinal}', ${index},
           ${quote(body)}, ${quote(JSON.stringify(offsets(body)))});`,
      );
    });
  });

  statements.push(
    `DELETE FROM playback_state WHERE document_id = ${quote(DOC)};`,
    `INSERT INTO playback_state (document_id, section_id, paragraph_id,
        sentence_index, sentence_offset_ms, voice_id, speed, updated_at)
     VALUES (${quote(DOC)}, 'sec-1', 'para-1-2', 1, 0, 'M1', 1.25,
        ${quote(STAMP)});`,
  );

  sqlite(statements.join("\n"));

  const cursor = sqlite(
    "SELECT text FROM paragraphs WHERE id = 'para-1-2';",
  ).trim();
  console.log(`seeded. the cursor points at: ${cursor}`);
  console.log("the Library card's bar should be half full (50%)");
}

/**
 * The app window's rectangle in screen coordinates.
 *
 * Read every time rather than cached: the window is not always on the main
 * display, and on a display above it the Y is negative -- which is exactly the
 * case a plain full-screen `screencapture` gets wrong, silently grabbing
 * whatever is on the main display instead.
 */
function bounds() {
  const raw = osascript(
    'tell application "System Events" to tell process "libretexts-reader" ' +
      "to get {position, size} of window 1",
  );
  const [x, y, width, height] = raw.split(", ").map(Number);
  return { x, y, width, height };
}

function shot(file) {
  const target = path.resolve(file ?? path.join(SCRATCH, "shot.png"));
  const { x, y, width, height } = bounds();
  // Bring it forward first: `screencapture -R` grabs whatever is on screen in
  // that rectangle, so a window behind the terminal photographs the terminal.
  osascript(
    'tell application "System Events" to set frontmost of process ' +
      '"libretexts-reader" to true',
  );
  execFileSync("screencapture", [
    "-x",
    `-R${x},${y},${width},${height}`,
    target,
  ]);
  console.log(target);
  return target;
}

async function launch() {
  if (!existsSync(DB)) {
    throw new Error("no scratch library -- run `driver.mjs seed` first");
  }
  startApp();
  await waitFor(
    async () => {
      try {
        bounds();
        return true;
      } catch {
        return false;
      }
    },
    { label: "the app window" },
  );
  // The window exists before React has painted the Library.
  await sleep(2000);
  console.log("running");
}

async function main() {
  const [command, ...rest] = process.argv.slice(2);
  switch (command) {
    case "build":
      return build();
    case "seed":
      await migrateScratchLibrary();
      return seedBook();
    case "launch":
      return launch();
    case "bounds":
      return console.log(JSON.stringify(bounds()));
    case "shot":
      return void shot(rest[0]);
    case "quit":
      return quit();
    case "demo": {
      await migrateScratchLibrary();
      seedBook();
      await launch();
      shot(rest[0] ?? path.join(SCRATCH, "library.png"));
      console.log("\nlook at that screenshot. the bar should be half full.");
      return;
    }
    default:
      console.error(
        "commands: build | seed | launch | bounds | shot <file> | quit | demo",
      );
      process.exitCode = 1;
  }
}

await main();
