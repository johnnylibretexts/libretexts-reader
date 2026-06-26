CREATE TABLE documents (
    id               TEXT PRIMARY KEY,
    title            TEXT NOT NULL,
    source_type      TEXT NOT NULL CHECK (source_type IN ('openstax','epub','pdf','pasted','url')),
    source_metadata  TEXT NOT NULL,
    cover_image_path TEXT,
    license          TEXT,
    attribution      TEXT,
    word_count       INTEGER NOT NULL DEFAULT 0,
    imported_at      TEXT NOT NULL,
    last_opened_at   TEXT
);

CREATE INDEX idx_documents_last_opened ON documents(last_opened_at DESC);
CREATE INDEX idx_documents_source_type ON documents(source_type);

CREATE TABLE sections (
    id          TEXT PRIMARY KEY,
    document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    ordinal     INTEGER NOT NULL,
    title       TEXT NOT NULL,
    word_count  INTEGER NOT NULL DEFAULT 0,
    UNIQUE (document_id, ordinal),
    -- Allows child rows to reference (document_id, id) via a composite FK.
    UNIQUE (document_id, id)
);

CREATE INDEX idx_sections_document ON sections(document_id, ordinal);

CREATE TABLE paragraphs (
    id               TEXT PRIMARY KEY,
    section_id       TEXT NOT NULL REFERENCES sections(id) ON DELETE CASCADE,
    ordinal          INTEGER NOT NULL,
    text             TEXT NOT NULL,
    sentence_offsets TEXT NOT NULL,
    UNIQUE (section_id, ordinal),
    -- Allows child rows to reference (section_id, id) via a composite FK.
    UNIQUE (section_id, id)
);

CREATE INDEX idx_paragraphs_section ON paragraphs(section_id, ordinal);

CREATE TABLE playback_state (
    document_id        TEXT PRIMARY KEY REFERENCES documents(id) ON DELETE CASCADE,
    section_id         TEXT NOT NULL,
    paragraph_id       TEXT NOT NULL,
    sentence_index     INTEGER NOT NULL DEFAULT 0,
    sentence_offset_ms INTEGER NOT NULL DEFAULT 0,
    voice_id           TEXT NOT NULL,
    speed              REAL NOT NULL DEFAULT 1.0,
    updated_at         TEXT NOT NULL,
    -- Enforce the hierarchy: the section must belong to this document and the
    -- paragraph must belong to that section, so a resume cursor cannot mix rows
    -- from different documents/sections.
    FOREIGN KEY (document_id, section_id)
        REFERENCES sections(document_id, id) ON DELETE CASCADE,
    FOREIGN KEY (section_id, paragraph_id)
        REFERENCES paragraphs(section_id, id) ON DELETE CASCADE
);

CREATE TABLE bookmarks (
    id             TEXT PRIMARY KEY,
    paragraph_id   TEXT NOT NULL REFERENCES paragraphs(id) ON DELETE CASCADE,
    sentence_index INTEGER NOT NULL,
    note           TEXT,
    created_at     TEXT NOT NULL
);

CREATE INDEX idx_bookmarks_paragraph ON bookmarks(paragraph_id);

CREATE TABLE voices (
    id            TEXT PRIMARY KEY,
    display_name  TEXT NOT NULL,
    language      TEXT NOT NULL,
    gender        TEXT NOT NULL,
    is_bundled    INTEGER NOT NULL DEFAULT 0,
    is_downloaded INTEGER NOT NULL DEFAULT 0,
    size_bytes    INTEGER NOT NULL DEFAULT 0,
    preview_path  TEXT
);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE openstax_cache (
    cache_key       TEXT PRIMARY KEY,
    book_uuid       TEXT NOT NULL,
    page_uuid       TEXT NOT NULL,
    content_gzip    BLOB NOT NULL,
    archive_release TEXT NOT NULL,
    fetched_at      TEXT NOT NULL
);

CREATE INDEX idx_openstax_cache_book ON openstax_cache(book_uuid);
