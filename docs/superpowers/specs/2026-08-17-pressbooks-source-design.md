# Pressbooks as a content source — design

Date: 2026-08-17
Status: approved, not yet planned

## Goal

Add Pressbooks as a fourth Catalog alongside OpenStax and LibreTexts, so a reader can
browse open textbooks published on Pressbooks networks and import one into the Library.

Pressbooks is the sixth importer. ADR-0002 names the arrival of a sixth importer that is a
near-copy of an existing one as its own revisit trigger. That trigger has fired, and the
first two units of work below are the response to it.

## What was verified against the live API

Everything in this section was probed directly on 2026-08-17. It is recorded because most
of it is not documented and two items invalidate the obvious design.

### The API is good

Each public Pressbooks book exposes a WordPress REST API under
`/wp-json/pressbooks/v2/`:

| Endpoint | Gives |
| --- | --- |
| `/metadata` | title, authors, publisher, license (name + URL), cover image URL |
| `/toc` | `front-matter[]`, `parts[] -> chapters[]`, `back-matter[]`, in reading order |
| `/chapters`, `/front-matter`, `/back-matter` | `content.rendered` HTML, paginated |
| `/books` (network root) | every public book on that network, with schema.org metadata |

TOC entries carry `has_post_content`, `word_count`, `export` and `menu_order`. There is no
equivalent of the LibreTexts 401/403 dual-strategy problem: the TOC is directly the chapter
tree.

Book metadata from `/books` is richer than the LibreTexts equivalent — `name`,
`alternativeHeadline` (subtitle), `image` and `thumbnailUrl`, `author[]`, `license` with
name/URL/code, `wordCount`, `network.name`, and `inCatalog` / `bookDirectoryExcluded`
flags.

### Two constraints that decide the design

**1. There is no server-side search.** `?search=biology` against eCampusOntario returns all
3,033 books, and the first two results are *Technology-Enabled Learning* and *Introduction
to French*. The parameter is accepted and ignored.

**2. `per_page` is hard-capped at 10.**

```
GET /wp-json/pressbooks/v2/books?per_page=20
400 {"code":"rest_invalid_param",
     "message":"Invalid parameter(s): per_page",
     "data":{"params":{"per_page":"per_page must be between 1 (inclusive) and 10 (inclusive)"}}}
```

Together these mean search can only exist locally, and local search requires enumerating a
whole network first. Measured: 1.55 s/page sequential, 0.30 s/page effective at concurrency
8. eCampusOntario is 304 pages — about 8 minutes sequential, about 91 seconds concurrent.
Payload is roughly 3.1 KB per book raw.

The cap is a deliberate tightening of the WordPress default, so it is a server-load choice
rather than an oversight. Do not attempt to work around it by increasing concurrency beyond
what is polite.

### Reachability

Some networks sit behind a Cloudflare managed challenge and return 403 with a "Just a
moment…" interstitial to any non-browser client. `reqwest` cannot pass these.

Verified reachable, with book counts at time of probe:

| Network | Books | Network | Books |
| --- | ---: | --- | ---: |
| ecampusontario.pressbooks.pub | 3033 | ohiostate.pressbooks.pub | 114 |
| pressbooks.online.ucf.edu | 1935 | louis.pressbooks.pub | 102 |
| boisestate.pressbooks.pub | 315 | milnepublishing.geneseo.edu | 90 |
| open.maricopa.edu | 252 | iastate.pressbooks.pub | 85 |
| uen.pressbooks.pub | 238 | viva.pressbooks.pub | 77 |
| kpu.pressbooks.pub | 188 | open.library.okstate.edu | 62 |
| | | oer.pressbooks.pub | 43 |
| | | rotel.pressbooks.pub | 38 |
| | | ncstate.pressbooks.pub | 34 |

Total: 6,806 books across 15 networks.

Verified unreachable, and the reason, so nobody re-adds them: `pressbooks.pub` and
`opentextbc.ca` (Cloudflare challenge); `fanshawe.pressbooks.pub`,
`library.achievingthedream.org`, `montana.pressbooks.net`, `oer.hawaii.edu` (no reachable
v2 API).

Counts move: OK State reported 61 and then 62 within the same session. Catalogs are live,
which is why the catalog is crawled at runtime rather than baked into the bundle.

### Math arrives as images that carry their own source

Pressbooks renders equations to QuickLaTeX PNGs. The `alt` attribute carries the LaTeX,
HTML-entity-encoded:

```html
<img src=".../quicklatex.com-9210efab…_l3.png"
     class="ql-img-displayed-equation quicklatex-auto-format"
     alt="&#92;&#091; &#92;&#84;&#104;&#101;&#116;&#97;&#61;…&#093;">
```

which decodes to `\[ \Theta=\Theta_g\frac{\rho_b}{\rho_w} \]`. Math is therefore
recoverable as LaTeX rather than lost to a mute image.

### Chapter HTML needs no new parsing

Chapter `content.rendered` uses absolute `https://` image URLs and real
`<figure>`/`<figcaption>` elements. `content/images.rs` and `content/html_section.rs` work
on it unchanged.

## Decisions

**Discovery: a bundled network list plus per-network browsing.** Not a single global
search — the Pressbooks Directory is Cloudflare-fronted and its `api.pressbooks.com`
backend is an undocumented JS application. Not paste-a-URL only, because that offers no
discovery.

**Enumeration: crawled at runtime into a SQLite cache**, not prebuilt into the bundle.
Catalogs change between releases, a prebuilt list would be stale on arrival, and the
crawl is a one-time cost per network that a progress indicator can carry.

**Duplication: extract the shared fetch/cache machinery before writing Pressbooks.**
Pressbooks reuses roughly 400 lines of `libretexts.rs` verbatim and additionally needs
concurrency that machinery does not have.

## Units of work

### 1. Characterize the fetch and cache layer

The code being extracted has no tests. `libretexts.rs` has four: three operate on HTML
strings, and the fourth is `#[ignore]`d and hits the live network. `fetch_json`,
`fetch_html`, the retry loop, `cached_page`, `store_page` and `fetch_book_pages` have zero
coverage.

Add `wiremock` as a **dev**-dependency and write tests against `libretexts.rs` as it stands
today, so they pass before the refactor and act as its regression gate:

1. Cache round-trip — store a page, read it back, assert gzip fidelity. Uses a `rusqlite`
   `:memory:` connection; needs no new dependency and no network.
2. Retry — a 503 followed by a 200 returns the body; three 503s return
   `AppError::LibreTexts` carrying the status.
3. No-retry — a 404 fails on the first attempt, asserted by request count.
4. Backoff is asserted as ordering, never as wall-clock duration.

Per the project convention for tests written against existing behaviour, each is verified
by breaking the behaviour deliberately and watching the test fail before restoring it.

### 2. Extract, behaviour-preserving

New `src-tauri/src/content/remote/`:

- `fetch.rs` — one retry/backoff path replacing both `fetch_json` and `fetch_html`, which
  are currently 43-line duplicates of each other differing only in an `accept` header and
  the body decode. Carries `error: fn(String) -> AppError` so LibreTexts keeps producing
  `AppError::LibreTexts` and Pressbooks produces `AppError::Pressbooks`; the `kind()`
  strings the frontend sees do not change.
- `cache.rs` — the gzip page cache, keyed by `(source, cache_key)`.
- `pages.rs` — concurrent fetch with a progress callback. `buffer_unordered` returns out of
  order, so results are re-sorted by index before returning, and the callback fires from
  the driving task rather than from inside the futures.

Migration `0008` creates `remote_page_cache (source, cache_key, book_key, content_gzip,
revision, fetched_at)`, copies existing `libretexts_cache` rows in with
`source = 'libretexts'`, and drops the old table. Copying rather than discarding costs four
lines of SQL and spares anyone with a cached book a full re-download.

**LibreTexts is wired to the new module at concurrency 1**, preserving its current
sequential behaviour exactly. Raising its concurrency changes the load placed on
libretexts.org and could newly trip 429s. That is a behaviour change and belongs in its own
commit where it can be measured — not inside a refactor.

Expected result: `libretexts.rs` drops from 1238 lines to roughly 850, retaining only what
is genuinely MindTouch-specific.

ADR-0002 is amended to record that its revisit trigger fired and what was extracted. Its
core claim — that the importers should not share an *entry shape* — is unchanged.

### 3. Catalog

Bundled network list at `src-tauri/resources/catalog/pressbooks-networks.json`, following
the existing `openstax.json` pattern, seeded with the 15 verified networks above. The
excluded networks and their reasons are recorded in the same file.

Crawl at concurrency 8 into two tables (migration `0009`):

- `pressbooks_network (host, name, total_books, synced_pages, total_pages, synced_at)`
- `pressbooks_book (host, book_url, title, subtitle, cover_url, thumbnail_url, authors,
  license_name, license_url, word_count)`

Search is `LIKE` over title, subtitle and author. At roughly 7,000 rows this is
sub-millisecond, so FTS5 is not used and no build flags change.

Freshness costs one request: `?per_page=1` returns `X-WP-Total` in the response headers, so
the current book count is readable without crawling. A crawl is only started when that
count differs from `pressbooks_network.total_books`.

Partial crawls are a supported state. Rows are written as pages arrive and
`synced_pages`/`total_pages` track progress, so a crawl interrupted at page 200 of 304
leaves 2,000 usable books and a resumable "2,000 of 3,033 loaded" state rather than
nothing.

Progress reaches the webview as a `catalog-sync-progress` event, mirroring the shape of the
existing `import-progress` event.

### 4. Importer

`content/pressbooks.rs`. The TOC supplies ordering; the collection endpoints supply content
in bulk:

```
/v2/metadata                       1 request
/v2/toc                            1 request
/v2/front-matter?per_page=10&…  ┐
/v2/chapters?per_page=10&…      ├  content.rendered, paginated
/v2/back-matter?per_page=10&…   ┘
```

A 40-chapter book is about 7 requests rather than 41.

Content rules, all driven by flags the TOC already provides:

1. Skip any entry with `has_post_content: false` or `word_count: 0` — title pages and empty
   placeholders, dropped without a heuristic.
2. Keep front-matter and back-matter that do have content. A glossary or appendix is
   legitimately readable and silently dropping it would surprise the reader.
3. Flatten parts, preserving TOC order. Parts are containers with no content of their own,
   and `DocumentBuilder.sections` is a flat `Vec` with nowhere to put one.
4. Images need no new code — a `PressbooksSource` implementation of `SectionSource` supplies
   the skip rule and `html_section.rs` does the rest.
5. `license` and `attribution` come from `metadata.license` (name and URL).

**Covers are downloaded.** `metadata.image` is a clean URL, and downloading it is about 20
lines against the fetch layer from unit 2. Note that this makes Pressbooks the only
*remote* source that populates `cover_image_path`: `epub.rs:101` is currently the only
place in the codebase that writes to `covers_dir()`, and OpenStax and LibreTexts both pass
`None`. The inconsistency is deliberate — it converts an invisible gap into a visible one
and produces a concrete follow-up item.

`source_metadata` keys on `book_url`, the book's canonical URL. `findImportedBook` is
already parameterised over the metadata key for exactly this reason.

### 5. Math

`PressbooksSource::should_skip_image` drops QuickLaTeX PNGs, which would otherwise import
as mute figures. The paragraph walk decodes the `alt` attribute into a new
`[[latex:<base64>]]` token alongside the existing `[[mathml:<base64>]]`.

LaTeX rather than MathML because KaTeX renders LaTeX natively; converting Rust-side would
add a dependency in order to reach a format the renderer handles less directly.

Touches `src/lib/mathContent.ts`, `src/components/Reader/MathText.tsx`, and
`src-tauri/src/content/normalize.rs` for the speech path.

Kept separate because units 3 and 4 ship without it — math arrives as skipped images, not
as breakage.

## Frontend

**Vocabulary:** "network" is Pressbooks' own word and belongs only in Pressbooks-facing code
and UI labels. The domain term for the thing it names is **Catalog** — see `CONTEXT.md`. Do
not introduce a `Network` domain type.

`src/components/PressbooksBrowser/` mirroring `LibreTextsBrowser/`, with a network picker
where LibreTexts has its library picker. Plus a sidebar entry, a route in `AppShell`, and a
`PressbooksBrowser.test.tsx` matching the two existing browser test files.

The browser must honour the global import guard in `src/stores/imports.ts`: every Add
across all catalogs is disabled while any import is active.

`SourceType::Pressbooks` in `src-tauri/src/db/models.rs` and `"pressbooks"` in
`src/types/domain.ts` **go in one commit**. The Fish Audio review found that a provider was
unselectable for exactly this reason — the frontend list was updated and the Rust list was
not, and the result returned `Ok` while silently storing the wrong value. Both lists move
together.

## Testing

`wiremock` covers the Pressbooks paths as well as the LibreTexts ones: TOC-to-section
ordering, the `has_post_content` skip, pagination assembly, partial-crawl resume, and the
QuickLaTeX `alt` decode.

One `#[ignore]`d live smoke test against Milne's 90-book network, matching the existing
LibreTexts live test.

Gate before every commit, unchanged: `npm run build`, `npm test`,
`cargo test -p libretexts-reader`, `git diff --check`.

## Out of scope

- Global directory search across all Pressbooks networks — Cloudflare-fronted and
  undocumented.
- H5P interactive activities. They are not readable text.
- Part hierarchy in the reader. The Section model is flat.
- Raising LibreTexts' fetch concurrency. Deliberately deferred to its own change so it can
  be measured.

## Follow-ups this work produces

These belong to the importer audit project, not to this one:

- OpenStax and LibreTexts imports have no local cover image.
- EPUB, PDF and URL imports drop images entirely.
- `readability` 0.3.0 was last released 2023-12-20 and is the whole of the URL importer.
- `scraper` is pinned at 0.20 against a current 0.27.0; `pdfium-render` at 0.8 against
  0.9.3.
