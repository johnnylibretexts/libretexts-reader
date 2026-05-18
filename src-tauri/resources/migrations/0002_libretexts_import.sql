CREATE TABLE documents_new (
    id               TEXT PRIMARY KEY,
    title            TEXT NOT NULL,
    source_type      TEXT NOT NULL CHECK (source_type IN ('openstax','libretexts','epub','pdf','pasted','url')),
    source_metadata  TEXT NOT NULL,
    cover_image_path TEXT,
    license          TEXT,
    attribution      TEXT,
    word_count       INTEGER NOT NULL DEFAULT 0,
    imported_at      TEXT NOT NULL,
    last_opened_at   TEXT
);

INSERT INTO documents_new (
    id, title, source_type, source_metadata, cover_image_path,
    license, attribution, word_count, imported_at, last_opened_at
)
SELECT
    id, title, source_type, source_metadata, cover_image_path,
    license, attribution, word_count, imported_at, last_opened_at
FROM documents;

DROP TABLE documents;
ALTER TABLE documents_new RENAME TO documents;

CREATE INDEX idx_documents_last_opened ON documents(last_opened_at DESC);
CREATE INDEX idx_documents_source_type ON documents(source_type);

CREATE TABLE libretexts_cache (
    cache_key        TEXT PRIMARY KEY,
    book_id          TEXT NOT NULL,
    page_id          TEXT NOT NULL,
    content_gzip     BLOB NOT NULL,
    content_revision TEXT,
    fetched_at       TEXT NOT NULL
);

CREATE INDEX idx_libretexts_cache_book ON libretexts_cache(book_id);
