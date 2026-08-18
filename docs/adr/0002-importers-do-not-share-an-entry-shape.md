# Importers do not share a common entry shape

The five importers — OpenStax, LibreTexts, EPUB, PDF, article — have five different entry signatures: different names, different arities, two different progress-callback types, three async and two sync. They converge only at `DocumentBuilder`. This reads like an obvious refactor waiting to happen, and it is deliberately left alone.

The formats differ in kind, not in detail. PDF reconstructs headings from font-size ratios and rebuilds paragraphs from line geometry, with no HTML anywhere in its path. LibreTexts needs two entirely separate table-of-contents strategies depending on whether the Deki API returns 401/403 for a given book. A trait wide enough to cover all five would be almost entirely optional parameters, and would explain nothing about what any importer actually does.

What genuinely *is* shared — walking HTML into paragraphs, images, anchors and captions — is factored into one module that takes a per-source skip rule. That is where the duplication was, and that is the only place it's worth removing.

Revisit if a sixth importer arrives that is a near-copy of an existing one, or if progress reporting is unified across all six import paths and the callback shapes have to agree anyway.

## Amendment: the revisit trigger fired, and the decision stands

Pressbooks arrives as a sixth importer that is a near-copy of LibreTexts, which is the
first of the two triggers above. What that revealed was not one duplicate but three: the
retry-with-backoff loop existed in OpenStax, in LibreTexts' JSON path, and again in
LibreTexts' HTML path, the last two differing only in a request header and how the body was
decoded. Adding Pressbooks would have made four. A fix to backoff could land in one Source
and silently miss the others.

The response is to share the **machinery**, not the entry shape. `content::remote` now holds
the retry loop, a page cache keyed by Source, and a concurrent fetcher with a progress hook.
The three separate retry loops became one. Each Source still names its own failures — the
shared loop reports what happened and the Source turns that into `AppError::OpenStax` or
`AppError::LibreTexts`, so the kind strings the webview receives are unchanged.

**The entry-shape decision is unchanged.** The six importers still have six signatures, still
converge only at `DocumentBuilder`, and there is still no trait over them. The reasons hold
exactly as written: PDF has no HTML in its path, and LibreTexts still needs two entirely
separate table-of-contents strategies depending on whether the Deki API returns 401/403.
Extracting the machinery did nothing to change that, which is the point — the duplication
worth removing was never in the entry shape.

Two things were deliberately left alone. `openstax_cache` keeps its own table: it carries an
`archive_release` column it actually reads for invalidation, which the shared cache's
`content_revision` does not yet do, so folding it in would be a behaviour change rather than
a move. And LibreTexts fetches at concurrency 1, exactly as it did before; the shared
fetcher's concurrency exists for Pressbooks, and raising it for LibreTexts is a decision
about how hard to lean on someone else's servers, not a refactor.

Revisit again if a Source needs a cache policy the shared one cannot express, or if
`openstax_cache` is still separate once a third Source needs revalidation.
