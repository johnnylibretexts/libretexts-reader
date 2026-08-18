-- Local cache of a Pressbooks Catalog.
--
-- Pressbooks caps `per_page` at 10 and ignores its own `search` parameter, so a
-- Catalog can only be searched after it has been enumerated locally. Even the
-- smallest network is nine requests; the largest is three hundred. Re-fetching
-- that on every visit to the browser would make opening a Catalog cost a crawl.
--
-- `total_books` is what makes the crawl skippable: one request with
-- `per_page=1` returns the live count in the `X-WP-Total` header, so staleness
-- is one request to check rather than a full re-enumeration.
--
-- `synced_pages` / `total_pages` record how far a crawl got. Today a crawl is
-- all-or-nothing -- it writes one transaction after every page has arrived, so
-- these two are always equal -- and the columns exist so that resuming a
-- partial crawl becomes a code change rather than another migration.

CREATE TABLE pressbooks_network (
    host         TEXT PRIMARY KEY,
    name         TEXT NOT NULL,
    total_books  INTEGER NOT NULL DEFAULT 0,
    synced_pages INTEGER NOT NULL DEFAULT 0,
    total_pages  INTEGER NOT NULL DEFAULT 0,
    synced_at    TEXT
);

-- book_url is the book's canonical URL, which is also the key `source_metadata`
-- carries on an imported Document. It is unique across networks on its own, but
-- host joins the primary key so a network's rows stay deletable as a unit.
CREATE TABLE pressbooks_book (
    host          TEXT NOT NULL,
    book_url      TEXT NOT NULL,
    title         TEXT NOT NULL,
    subtitle      TEXT,
    cover_url     TEXT,
    thumbnail_url TEXT,
    authors       TEXT NOT NULL DEFAULT '',
    license_name  TEXT NOT NULL DEFAULT '',
    license_url   TEXT,
    word_count    INTEGER NOT NULL DEFAULT 0,
    menu_position INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (host, book_url)
);
