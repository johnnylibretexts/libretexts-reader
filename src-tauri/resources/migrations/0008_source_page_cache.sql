-- Replace the LibreTexts-specific page cache with one keyed by Source.
--
-- A second remote Source (Pressbooks) needs the same cache, and page ids are
-- only unique within a Source -- two Sources numbering a page the same way
-- would otherwise read each other's rows. The Source name joins the primary
-- key rather than being smuggled into cache_key, so a Source's rows stay
-- selectable on their own.
--
-- Existing rows are carried across rather than dropped: they are pages a reader
-- has already waited for, and discarding them would make this migration a
-- silent re-download of every book in the Library.
--
-- No index beyond the primary key. Reads are always by (source, cache_key),
-- which the primary key serves; the old table's book_id index was never
-- queried either, and carrying it across would be write amplification on every
-- page a reader caches.
--
-- openstax_cache is deliberately left alone. It carries an archive_release
-- column that it actually reads for invalidation, which this table's
-- content_revision does not yet do, so folding it in is a behaviour change
-- rather than a move.

CREATE TABLE source_page_cache (
    source           TEXT NOT NULL,
    cache_key        TEXT NOT NULL,
    book_id          TEXT NOT NULL,
    page_id          TEXT NOT NULL,
    content_gzip     BLOB NOT NULL,
    content_revision TEXT,
    fetched_at       TEXT NOT NULL,
    PRIMARY KEY (source, cache_key)
);

INSERT INTO source_page_cache (
    source, cache_key, book_id, page_id, content_gzip, content_revision, fetched_at
)
SELECT 'libretexts', cache_key, book_id, page_id, content_gzip, content_revision, fetched_at
FROM libretexts_cache;

DROP TABLE libretexts_cache;
