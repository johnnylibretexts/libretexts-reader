-- Let a Document say it came from Pressbooks.
--
-- `documents.source_type` carries a CHECK constraint listing every Source by
-- name, so adding a Source to the Rust enum is not enough: the INSERT fails
-- with a constraint violation and the Import cannot be saved at all. Migration
-- 0002 widened this same list for LibreTexts; this does the same for
-- Pressbooks, by the same table swap, because SQLite cannot alter a CHECK in
-- place.
--
-- Four places name the set of Sources and none of them are linked by the
-- compiler: this constraint, `SourceType` in db/models.rs, `source_type_str`
-- and `parse_source_type`. A round-trip test in db/library.rs now persists and
-- lists one Document per variant, which fails if any of the four is missed.

CREATE TABLE documents_new (
    id               TEXT PRIMARY KEY,
    title            TEXT NOT NULL,
    source_type      TEXT NOT NULL CHECK (source_type IN ('openstax','libretexts','pressbooks','epub','pdf','pasted','url')),
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
