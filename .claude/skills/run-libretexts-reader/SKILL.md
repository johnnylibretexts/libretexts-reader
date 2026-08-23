---
name: run-libretexts-reader
description: Build, launch, seed and screenshot the LibreTexts Reader desktop app on macOS. Use when asked to run or start the app, take a screenshot of it, see a change working in the real UI, or check playback/library behaviour outside the test suite.
---

# Running LibreTexts Reader

A Tauri 2 desktop app, so there is no URL to hit and no headless mode. It is
driven by `.claude/skills/run-libretexts-reader/driver.mjs`, which builds it,
gives it a **scratch library** to work against, launches it, and screenshots
its window.

Paths below are relative to the repo root. macOS only — this is the only
platform the app bundles for.

The scratch library matters: `paths.rs` creates every directory it resolves, so
an unguarded run reads and writes the reader's real books at
`~/Library/Application Support/dev.johnnylibretexts.reader`. Every driver
command sets `LIBRETEXTS_READER_APP_DATA_DIR` to a throwaway directory under
`$TMPDIR` instead, so seeding a book or moving a playback cursor never touches
real data. Override it with `RUN_SCRATCH=/some/path`.

## Prerequisites

Already present on this machine; no `brew install` was needed.

- Node 22.x (verified on v22.20.0). **Not Node 24** — see `CLAUDE.md`.
- Rust stable via rustup, pinned by `rust-toolchain.toml`.
- `/usr/bin/sqlite3`, which ships with macOS. The driver uses this absolute
  path deliberately: a conda or homebrew `sqlite3` earlier on `PATH` is not
  guaranteed to exist.
- Network on the **first** build only, for `build.rs` to fetch PDFium.

## Build

```bash
node .claude/skills/run-libretexts-reader/driver.mjs build
```

Wraps `npm run tauri -- build --debug --no-bundle`. About 50s warm, minutes
cold. `--no-bundle` is deliberate: it skips the DMG step, which is the step
that needs a GUI session and `CI=true` (see `CLAUDE.md`).

## Run — the agent path

```bash
# seed a book with a resume cursor, launch, and screenshot, in one go
node .claude/skills/run-libretexts-reader/driver.mjs demo
```

Prints the screenshot path. **Open it and look at it.** A capture of your
terminal instead of the app means the window moved displays — see Gotchas.

Individual commands:

| Command | What it does |
| --- | --- |
| `build` | debug binary, no installers |
| `seed` | fresh scratch library + one fixture book |
| `launch` | start the app against the scratch library, wait for its window |
| `bounds` | print the window rectangle, e.g. `{"x":640,"y":-1257,...}` |
| `shot <file>` | screenshot **the window**, not the screen |
| `quit` | stop the app |

```bash
node .claude/skills/run-libretexts-reader/driver.mjs seed
node .claude/skills/run-libretexts-reader/driver.mjs launch
node .claude/skills/run-libretexts-reader/driver.mjs shot /tmp/library.png
node .claude/skills/run-libretexts-reader/driver.mjs quit
```

`seed` wipes and rebuilds the scratch library; `launch` reuses whatever is
already there, so seed once and relaunch as often as you like.

### What the fixture proves

`seed` writes one book — *The Resume Demonstration* — with numbers chosen so
the expected state is checkable by eye rather than by trusting the app:

- Three sections of four paragraphs, six words each: 24 words a section, 72 in
  the book.
- A resume cursor on the **third paragraph of chapter two** (ordinal 2 of 4),
  sentence 1, speed 1.25.
- So progress is 24 finished words plus half of the 24 in progress, over 72 —
  exactly **50%**, and the Library card's bar should be visibly half full.

Opening the book should land on *"This is the third paragraph of chapter
two"*, not chapter one.

To check the derivation without the GUI at all, query the scratch database
directly — `$TMPDIR/libretexts-reader-run/appdata/library.sqlite` — with
`/usr/bin/sqlite3`.

## Run — the human path

```bash
npm run tauri:dev
```

Vite plus the Rust backend with hot reload, against the **real** library. Use
this when a human is going to drive; use the driver when an agent is.

## Test

```bash
npm run build && npm test && cargo test -p libretexts-reader && git diff --check
```

The repo's full gate, from `CLAUDE.md`.

## Gotchas

- **The window is often not on the main display.** On this machine it opens at
  `y = -1257`, i.e. on the display *above* the main one. A plain
  `screencapture -x shot.png` then succeeds, exits 0, and silently photographs
  whatever is on the main display — a terminal, usually. That is why `shot`
  reads `bounds` every time and captures `-R<x,y,w,h>`. Never hardcode the
  rectangle.
- **Clicks cannot be automated here.** All three routes are dead ends:
  `osascript` `click at {x, y}` fails with `System Events got an error … (-25204)`;
  the WKWebView exposes no named buttons, so `click button "Open"` fails with
  `-1700`; `cliclick` is not installed. `set frontmost` *does* work, which is
  what `shot` relies on. **A flow that needs a click has to be handed to a
  human** — say which control to click and what they should see. (`-25204` is
  an Accessibility-permission symptom; granting the terminal Accessibility
  rights may lift it, but that is the human's call to make, not something to
  work around.)
- **The app has to create its own database.** `seed` launches the app, waits
  for `library.sqlite`, kills it, and only then writes rows. Replaying the
  files in `src-tauri/resources/migrations/` by hand skips the `_migrations`
  bookkeeping in `db::migrations`, and the next real launch tries to apply
  them all over again.
- **Killing the app reports exit code 144.** `quit`, or any `pkill` against
  it, makes a backgrounded launch report failure. That is the SIGTERM you
  asked for, not a crash.
- **`seed` is destructive to the scratch library only.** It `rm -rf`s the
  scratch directory first. It cannot reach the real one.

## Troubleshooting

| Symptom | Fix |
| --- | --- |
| `no binary at target/debug/libretexts-reader` | `driver.mjs build` |
| `no scratch library -- run seed first` | `driver.mjs seed` |
| `timed out waiting for the app window` | The process died on launch. Run the binary in the foreground with `LIBRETEXTS_READER_APP_DATA_DIR` set and read stderr. |
| Screenshot shows a terminal | The window moved. Re-run `bounds`; do not reuse an old rectangle. |
| `System Events got an error … (-25204)` | Expected. Clicking is not automatable here — hand the click to a human. |
