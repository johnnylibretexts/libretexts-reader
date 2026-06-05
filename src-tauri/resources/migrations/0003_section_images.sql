CREATE TABLE section_images (
    id           TEXT PRIMARY KEY,
    section_id   TEXT NOT NULL REFERENCES sections(id) ON DELETE CASCADE,
    ordinal      INTEGER NOT NULL,
    source_url   TEXT NOT NULL,
    local_path   TEXT NOT NULL,
    alt_text     TEXT,
    caption      TEXT,
    content_type TEXT,
    UNIQUE (section_id, ordinal)
);

CREATE INDEX idx_section_images_section ON section_images(section_id, ordinal);
