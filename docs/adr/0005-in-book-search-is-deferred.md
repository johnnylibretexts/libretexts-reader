# In-book search and bookmarks are deferred

Search matches book titles and nothing else. `Grid.tsx` filters loaded titles
client-side, `db::library` does a `title LIKE`, and there is no way to find a
phrase inside a book or mark a place to return to. For book-length material
that is a real gap, and the first-run UX audit filed it as one (#69).

It is deferred to after the public beta anyway, because the cheap version is
worse than nothing and the honest version is a feature, not a fix.

**The cheap version does not work here.** A `LIKE '%phrase%'` over
`sections.content` finds the section but not the place in it, and a section is
a whole chapter. Landing a reader at the top of the chapter their phrase
appears in is barely better than the table of contents they already have. It
also cannot match across the paragraph boundaries the importer creates, and it
cannot see through the math tokens at all: `[[mathml:<base64>]]` is opaque to
`LIKE`, so a search for a term inside an equation silently finds nothing while
appearing to work.

**The honest version needs a decision this beta has not earned yet.** Useful
in-book search means SQLite FTS5 over paragraphs, a paragraph-level ordinal to
navigate to, and a rule for what a match inside a math token means. That is a
migration, an index that grows with every import, and a new navigation target
in the player — none of it hard, all of it worth doing once against real
feedback about how readers actually lose their place.

**Bookmarks are cheaper but are the same decision.** They need the same
paragraph-level anchor that search results need. Building the anchor twice, or
building bookmarks on a weaker anchor and migrating it later, is the outcome
worth avoiding.

## Consequences

- **The gap is stated in the app rather than left to be discovered.** The
  Settings panel carries a "What this app does not do yet" note naming this,
  alongside the table and equation limitations, so a reader hits it as a known
  boundary rather than as a broken feature.
- **Resume covers part of what bookmarks would.** A book reopens where it was
  left (#59), so "get back to where I was" works; "get back to the bit about
  entropy" does not.
- **There is a dead path to clean up or adopt first.** `stores/library.ts`
  exports a `search` action and `tauri.ts` a `searchDocuments` command, and no
  component calls either — `Grid.tsx` filters client-side instead. Whoever
  picks this up should decide whether the server-side path becomes the
  foundation or is deleted; leaving both is how the two drift.
- **Revisit when there is beta feedback**, not on a schedule. The trigger to
  build it is readers saying they cannot find things, which the beta exists to
  learn.
