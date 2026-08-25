CREATE TABLE sentence_translations (
  paragraph_id   TEXT    NOT NULL REFERENCES paragraphs(id) ON DELETE CASCADE,
  sentence_index INTEGER NOT NULL,
  target_lang    TEXT    NOT NULL,
  text           TEXT    NOT NULL,
  qa_status      TEXT    NOT NULL,
  PRIMARY KEY (paragraph_id, sentence_index, target_lang)
);

CREATE TABLE section_translations (
  section_id     TEXT    NOT NULL REFERENCES sections(id) ON DELETE CASCADE,
  target_lang    TEXT    NOT NULL,
  source_lang    TEXT    NOT NULL,
  status         TEXT    NOT NULL,
  model_id       TEXT    NOT NULL,
  qa_sampled     INTEGER NOT NULL DEFAULT 0,
  qa_failed      INTEGER NOT NULL DEFAULT 0,
  qa_escalated   INTEGER NOT NULL DEFAULT 0,
  fallback_count INTEGER NOT NULL DEFAULT 0,
  updated_at     TEXT    NOT NULL,
  PRIMARY KEY (section_id, target_lang)
);

ALTER TABLE documents ADD COLUMN source_language TEXT;
