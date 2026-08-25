use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

use crate::content::normalize::normalize_for_tts;
use crate::db::models::{Document, Paragraph, PlaybackState, Section, SectionImage, SourceType};
use crate::error::{AppError, AppResult};

/// The `SELECT` list and joins every Document read shares.
///
/// `progress` is derived here rather than stored: sections already behind the
/// resume cursor contribute their whole word count, and the section holding the
/// cursor contributes the fraction of its paragraphs the reader has passed. A
/// book with no cursor coalesces to zero. The clamp guards the one case the
/// arithmetic cannot: a Document whose section word counts do not sum to its
/// own would otherwise hand the card a bar wider than its track.
const DOCUMENT_COLUMNS: &str = "d.id, d.title, d.source_type, d.source_metadata,
                d.cover_image_path, d.license, d.attribution, d.word_count,
                COALESCE(NULLIF(TRIM(d.source_language), ''), 'en') AS source_language,
                d.imported_at, d.last_opened_at,
                COALESCE(
                    MIN(1.0, MAX(0.0,
                        (
                            COALESCE((SELECT SUM(prior.word_count)
                                      FROM sections prior
                                      WHERE prior.document_id = d.id
                                        AND prior.ordinal < s.ordinal), 0)
                            + (CAST(p.ordinal AS REAL)
                               / (SELECT COUNT(*) FROM paragraphs kin
                                  WHERE kin.section_id = s.id))
                              * s.word_count
                        ) / NULLIF(d.word_count, 0)
                    )),
                    0.0
                ) AS progress";

/// Left joins throughout: a Document with no resume cursor is the ordinary
/// case, not a missing row.
const DOCUMENT_FROM: &str = "FROM documents d
         LEFT JOIN playback_state ps ON ps.document_id = d.id
         LEFT JOIN sections s ON s.id = ps.section_id
         LEFT JOIN paragraphs p ON p.id = ps.paragraph_id";

pub fn list_documents(conn: &Connection) -> AppResult<Vec<Document>> {
    let mut statement = conn.prepare(&format!(
        "SELECT {DOCUMENT_COLUMNS}
         {DOCUMENT_FROM}
         ORDER BY COALESCE(d.last_opened_at, d.imported_at) DESC"
    ))?;
    let rows = statement.query_map([], document_from_row)?;
    collect_rows(rows)
}

pub fn search_documents(conn: &Connection, query: &str) -> AppResult<Vec<Document>> {
    let pattern = format!("%{}%", query.trim());
    let mut statement = conn.prepare(&format!(
        "SELECT {DOCUMENT_COLUMNS}
         {DOCUMENT_FROM}
         WHERE d.title LIKE ?1
         ORDER BY COALESCE(d.last_opened_at, d.imported_at) DESC"
    ))?;
    let rows = statement.query_map(params![pattern], document_from_row)?;
    collect_rows(rows)
}

pub fn get_document(conn: &Connection, id: &str) -> AppResult<Document> {
    let mut statement = conn.prepare(&format!(
        "SELECT {DOCUMENT_COLUMNS}
         {DOCUMENT_FROM}
         WHERE d.id = ?1"
    ))?;

    statement
        .query_row(params![id], document_from_row)
        .map_err(Into::into)
}

pub fn list_sections(conn: &Connection, document_id: &str) -> AppResult<Vec<Section>> {
    let mut statement = conn.prepare(
        "SELECT id, document_id, ordinal, title, word_count
         FROM sections
         WHERE document_id = ?1
         ORDER BY ordinal",
    )?;
    let rows = statement.query_map(params![document_id], |row| {
        Ok(Section {
            id: row.get(0)?,
            document_id: row.get(1)?,
            ordinal: row.get(2)?,
            title: row.get(3)?,
            word_count: row.get(4)?,
        })
    })?;

    collect_rows(rows)
}

pub fn list_paragraphs(
    conn: &Connection,
    section_id: &str,
    target_lang: Option<&str>,
) -> AppResult<Vec<Paragraph>> {
    let translations = load_translations(conn, section_id, target_lang)?;
    let mut statement = conn.prepare(
        "SELECT id, section_id, ordinal, text, sentence_offsets
         FROM paragraphs
         WHERE section_id = ?1
         ORDER BY ordinal",
    )?;
    let rows = statement.query_map(params![section_id], |row| {
        let id: String = row.get(0)?;
        let raw_offsets: String = row.get(4)?;
        let text: String = row.get(3)?;
        let sentence_offsets: Vec<(usize, usize)> =
            serde_json::from_str(&raw_offsets).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    4,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;

        // Translation deliberately breaks the old "speech is derived from display"
        // invariant. The one that survives -- and the one every consumer indexes on --
        // is exactly one speech form per offset, in order. Falling back per index
        // rather than per chapter is what makes a partially translated chapter a valid
        // state rather than a corrupt one, which in turn is why Cancel needs no
        // rollback.
        let sentence_speech = sentence_offsets
            .iter()
            .enumerate()
            .map(|(index, (start, end))| {
                translations
                    .get(&(id.clone(), index as i64))
                    .map(|translation| normalize_for_tts(translation))
                    .unwrap_or_else(|| {
                        normalize_for_tts(text.get(*start..*end).unwrap_or_default())
                    })
            })
            .collect();

        Ok(Paragraph {
            id,
            section_id: row.get(1)?,
            ordinal: row.get(2)?,
            text,
            sentence_offsets,
            sentence_speech,
        })
    })?;

    collect_rows(rows)
}

fn load_translations(
    conn: &Connection,
    section_id: &str,
    target_lang: Option<&str>,
) -> AppResult<HashMap<(String, i64), String>> {
    let Some(target_lang) = target_lang else {
        return Ok(HashMap::new());
    };
    let mut statement = conn.prepare(
        "SELECT t.paragraph_id, t.sentence_index, t.text
           FROM sentence_translations t
           JOIN paragraphs p ON p.id = t.paragraph_id
          WHERE p.section_id = ?1 AND t.target_lang = ?2 AND t.qa_status != 'rejected'",
    )?;
    let rows = statement.query_map(params![section_id, target_lang], |row| {
        Ok((
            (row.get::<_, String>(0)?, row.get::<_, i64>(1)?),
            row.get::<_, String>(2)?,
        ))
    })?;
    rows.collect::<Result<HashMap<_, _>, _>>()
        .map_err(Into::into)
}

pub fn list_section_images(conn: &Connection, section_id: &str) -> AppResult<Vec<SectionImage>> {
    let mut statement = conn.prepare(
        "SELECT id, section_id, ordinal, source_url, local_path,
                alt_text, caption, content_type, anchor_paragraph_ordinal
         FROM section_images
         WHERE section_id = ?1
         ORDER BY ordinal",
    )?;
    let rows = statement.query_map(params![section_id], |row| {
        Ok(SectionImage {
            id: row.get(0)?,
            section_id: row.get(1)?,
            ordinal: row.get(2)?,
            source_url: row.get(3)?,
            local_path: row.get(4)?,
            alt_text: row.get(5)?,
            caption: row.get(6)?,
            content_type: row.get(7)?,
            anchor_paragraph_ordinal: row.get(8)?,
        })
    })?;

    collect_rows(rows)
}

pub fn delete_document(conn: &Connection, id: &str) -> AppResult<()> {
    let row = conn
        .query_row(
            "SELECT source_type, source_metadata, cover_image_path FROM documents WHERE id = ?1",
            params![id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?;
    let image_paths = document_image_paths(conn, id)?;

    conn.execute("DELETE FROM documents WHERE id = ?1", params![id])?;

    if let Some((source_type, source_metadata, cover_image_path)) = row {
        // The cache outlives an Import by design, but a reader who deletes
        // the Document has no other way to force a fresh copy on the next
        // one, so the deletion has to reach it too.
        if source_type == "libretexts" {
            if let Some(book_id) = parse_json(&source_metadata)
                .ok()
                .and_then(|value| value.get("book_id")?.as_str().map(str::to_string))
            {
                conn.execute(
                    "DELETE FROM source_page_cache WHERE source = 'libretexts' AND book_id = ?1",
                    params![book_id],
                )?;
            }
        }

        if let Some(path) = cover_image_path {
            remove_file_if_present(&path)?;
        }
    }
    for path in image_paths {
        remove_file_if_present(&path)?;
    }

    Ok(())
}

/// Correct a book's declared/detected language and discard speech translations
/// derived from the old source language.
pub fn set_document_source_language(
    conn: &mut Connection,
    document_id: &str,
    source_language: &str,
) -> AppResult<()> {
    let source_language = source_language
        .trim()
        .to_ascii_lowercase()
        .replace('_', "-");
    if source_language.len() < 2
        || source_language.len() > 16
        || !source_language
            .chars()
            .all(|character| character.is_ascii_lowercase() || character == '-')
    {
        return Err(AppError::InvalidInput(
            "source language must be a short language code such as en or es".into(),
        ));
    }

    let tx = conn.transaction()?;
    let updated = tx.execute(
        "UPDATE documents SET source_language = ?1 WHERE id = ?2",
        params![source_language, document_id],
    )?;
    if updated == 0 {
        return Err(AppError::InvalidInput("document was not found".into()));
    }
    tx.execute(
        "DELETE FROM sentence_translations
          WHERE paragraph_id IN (
                SELECT p.id
                  FROM paragraphs p
                  JOIN sections s ON s.id = p.section_id
                 WHERE s.document_id = ?1
          )",
        [document_id],
    )?;
    tx.execute(
        "DELETE FROM section_translations
          WHERE section_id IN (
                SELECT id FROM sections WHERE document_id = ?1
          )",
        [document_id],
    )?;
    tx.commit()?;
    Ok(())
}

fn document_image_paths(conn: &Connection, document_id: &str) -> AppResult<Vec<String>> {
    let mut statement = conn.prepare(
        "SELECT DISTINCT section_images.local_path
         FROM section_images
         INNER JOIN sections ON sections.id = section_images.section_id
         WHERE sections.document_id = ?1",
    )?;
    let rows = statement.query_map(params![document_id], |row| row.get(0))?;
    collect_rows(rows)
}

fn remove_file_if_present(path: &str) -> AppResult<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// Write the resume cursor for a document, and stamp the document as opened.
///
/// The two writes belong together: `last_opened_at` is what orders the Library,
/// and a cursor that moved without the shelf noticing leaves the book the
/// reader is actually listening to sitting where it was.
pub fn save_playback_state(conn: &Connection, playback: &PlaybackState) -> AppResult<()> {
    let updated_at = playback.updated_at.to_rfc3339();
    conn.execute(
        "INSERT INTO playback_state (
             document_id, section_id, paragraph_id, sentence_index,
             sentence_offset_ms, voice_id, speed, updated_at
         )
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(document_id) DO UPDATE SET
             section_id = excluded.section_id,
             paragraph_id = excluded.paragraph_id,
             sentence_index = excluded.sentence_index,
             sentence_offset_ms = excluded.sentence_offset_ms,
             voice_id = excluded.voice_id,
             speed = excluded.speed,
             updated_at = excluded.updated_at",
        params![
            &playback.document_id,
            &playback.section_id,
            &playback.paragraph_id,
            playback.sentence_index,
            playback.sentence_offset_ms,
            &playback.voice_id,
            playback.speed,
            &updated_at,
        ],
    )?;
    conn.execute(
        "UPDATE documents SET last_opened_at = ?1 WHERE id = ?2",
        params![&updated_at, &playback.document_id],
    )?;
    Ok(())
}

/// The resume cursor for a document, or `None` when the reader has never
/// opened it.
///
/// `None` is a real answer, not a failure: the row is deleted with its
/// document, and its composite foreign keys mean a section or paragraph that
/// disappeared in a re-import takes the cursor with it. So a caller that gets
/// `None` opens at the beginning; only an `Err` means the read itself broke.
pub fn get_playback_state(
    conn: &Connection,
    document_id: &str,
) -> AppResult<Option<PlaybackState>> {
    let mut statement = conn.prepare(
        "SELECT document_id, section_id, paragraph_id, sentence_index,
                sentence_offset_ms, voice_id, speed, updated_at
         FROM playback_state
         WHERE document_id = ?1",
    )?;

    statement
        .query_row(params![document_id], playback_state_from_row)
        .optional()
        .map_err(Into::into)
}

fn collect_rows<T>(rows: impl Iterator<Item = Result<T, rusqlite::Error>>) -> AppResult<Vec<T>> {
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn document_from_row(row: &rusqlite::Row<'_>) -> Result<Document, rusqlite::Error> {
    let source_type: String = row.get(2)?;
    let source_metadata: String = row.get(3)?;
    let imported_at: String = row.get(9)?;
    let last_opened_at: Option<String> = row.get(10)?;

    Ok(Document {
        id: row.get(0)?,
        title: row.get(1)?,
        source_type: parse_source_type(&source_type).map_err(to_sql_conversion_error(2))?,
        source_metadata: parse_json(&source_metadata).map_err(to_sql_conversion_error(3))?,
        cover_image_path: row.get(4)?,
        license: row.get(5)?,
        attribution: row.get(6)?,
        word_count: row.get(7)?,
        source_language: row.get(8)?,
        imported_at: parse_datetime(&imported_at).map_err(to_sql_conversion_error(9))?,
        last_opened_at: last_opened_at
            .as_deref()
            .map(parse_datetime)
            .transpose()
            .map_err(to_sql_conversion_error(10))?,
        progress: row.get(11)?,
    })
}

fn playback_state_from_row(row: &rusqlite::Row<'_>) -> Result<PlaybackState, rusqlite::Error> {
    let updated_at: String = row.get(7)?;

    Ok(PlaybackState {
        document_id: row.get(0)?,
        section_id: row.get(1)?,
        paragraph_id: row.get(2)?,
        sentence_index: row.get(3)?,
        sentence_offset_ms: row.get(4)?,
        voice_id: row.get(5)?,
        speed: row.get(6)?,
        updated_at: parse_datetime(&updated_at).map_err(to_sql_conversion_error(7))?,
    })
}

/// The inverse of `source_type_str`, and the one direction the compiler cannot
/// check: this matches on `&str`, so a Source added to the enum still compiles
/// here. Getting it wrong is not a per-row problem -- `document_from_row` runs
/// inside `query_map`, so one unreadable row fails the entire Library listing.
fn parse_source_type(value: &str) -> AppResult<SourceType> {
    match value {
        "openstax" => Ok(SourceType::Openstax),
        "libretexts" => Ok(SourceType::Libretexts),
        "pressbooks" => Ok(SourceType::Pressbooks),
        "epub" => Ok(SourceType::Epub),
        "pdf" => Ok(SourceType::Pdf),
        "pasted" => Ok(SourceType::Pasted),
        "url" => Ok(SourceType::Url),
        _ => Err(AppError::InvalidInput(format!(
            "unknown source type: {value}"
        ))),
    }
}

fn parse_json(value: &str) -> AppResult<Value> {
    serde_json::from_str(value).map_err(Into::into)
}

fn parse_datetime(value: &str) -> AppResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|datetime| datetime.with_timezone(&Utc))
        .map_err(|error| AppError::InvalidInput(format!("invalid timestamp: {error}")))
}

fn to_sql_conversion_error(column: usize) -> impl FnOnce(AppError) -> rusqlite::Error {
    move |error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use crate::content::document::{DocumentBuilder, SectionBuilder};
    use crate::db::connection::temporary_pool;
    use crate::db::migrations::apply_migrations;
    use crate::db::models::{PlaybackState, SourceType};

    fn seed_section_with_two_sentences() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open an in-memory database");
        apply_migrations(&mut conn).expect("apply migrations");
        conn.execute_batch(
            "INSERT INTO documents
                 (id, title, source_type, source_metadata, word_count, source_language, imported_at)
             VALUES ('doc-1', 'A Book', 'pasted', '{}', 4, 'en', '2026-08-24T00:00:00Z');
             INSERT INTO sections (id, document_id, ordinal, title, word_count)
             VALUES ('sec-1', 'doc-1', 0, 'One', 4);
             INSERT INTO paragraphs (id, section_id, ordinal, text, sentence_offsets)
             VALUES (
                 'p1',
                 'sec-1',
                 0,
                 'First sentence. Second sentence.',
                 '[[0,15],[16,32]]'
             );",
        )
        .expect("seed a section with two sentences");
        conn
    }

    #[test]
    fn a_translated_sentence_is_spoken_and_an_untranslated_one_falls_back() {
        let conn = seed_section_with_two_sentences();
        conn.execute(
            "INSERT INTO sentence_translations
                 (paragraph_id, sentence_index, target_lang, text, qa_status)
             VALUES ('p1', 0, 'es', 'Primera frase.', 'passed')",
            [],
        )
        .unwrap();

        let paragraphs = super::list_paragraphs(&conn, "sec-1", Some("es")).unwrap();
        let speech = &paragraphs[0].sentence_speech;

        assert_eq!(speech[0], "Primera frase.");
        assert_eq!(
            speech[1], "Second sentence.",
            "no row means the original, not silence"
        );
    }

    #[test]
    fn a_rejected_translation_is_never_spoken() {
        let conn = seed_section_with_two_sentences();
        conn.execute(
            "INSERT INTO sentence_translations
                 (paragraph_id, sentence_index, target_lang, text, qa_status)
             VALUES ('p1', 0, 'es', 'Basura.', 'rejected')",
            [],
        )
        .unwrap();

        let paragraphs = super::list_paragraphs(&conn, "sec-1", Some("es")).unwrap();
        assert_eq!(paragraphs[0].sentence_speech[0], "First sentence.");
    }

    #[test]
    fn there_is_always_one_speech_form_per_offset() {
        // Highlighting, the resume cursor, progress and chapter export all index
        // on this equality. A short list breaks all four at once, far from here.
        let conn = seed_section_with_two_sentences();
        for target in [None, Some("es"), Some("fr")] {
            let paragraphs = super::list_paragraphs(&conn, "sec-1", target).unwrap();
            for paragraph in paragraphs {
                assert_eq!(
                    paragraph.sentence_offsets.len(),
                    paragraph.sentence_speech.len()
                );
            }
        }
    }

    #[test]
    fn a_translated_math_token_is_still_normalized_before_it_is_spoken() {
        // The translated branch must pass through `normalize_for_tts` too, or a
        // restored `[[mathml:...]]` reaches the speech engine undecoded.
        let conn = seed_section_with_two_sentences();
        conn.execute(
            "INSERT INTO sentence_translations
                 (paragraph_id, sentence_index, target_lang, text, qa_status)
             VALUES ('p1', 0, 'es', 'Resuelve [[latex:eA==]].', 'passed')",
            [],
        )
        .unwrap();

        let paragraphs = super::list_paragraphs(&conn, "sec-1", Some("es")).unwrap();
        assert!(!paragraphs[0].sentence_speech[0].contains("[[latex:"));
    }

    #[test]
    fn correcting_the_source_language_invalidates_translations_from_the_old_source() {
        let mut conn = seed_section_with_two_sentences();
        conn.execute_batch(
            "INSERT INTO sentence_translations
                 (paragraph_id, sentence_index, target_lang, text, qa_status)
             VALUES ('p1', 0, 'es', 'Primera frase.', 'passed');
             INSERT INTO section_translations
                 (section_id, target_lang, source_lang, status, model_id, updated_at)
             VALUES ('sec-1', 'es', 'en', 'complete', 'model-v1', '2026-08-24T00:00:00Z');",
        )
        .unwrap();

        super::set_document_source_language(&mut conn, "doc-1", " FR ").unwrap();

        assert_eq!(
            super::get_document(&conn, "doc-1").unwrap().source_language,
            "fr"
        );
        let sentence_rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM sentence_translations", [], |row| {
                row.get(0)
            })
            .unwrap();
        let section_rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM section_translations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!((sentence_rows, section_rows), (0, 0));
    }

    /// Every Source must survive a persist-and-list round trip.
    ///
    /// `source_type_str` writes the string and `parse_source_type` reads it
    /// back, and neither is checked against the other by the compiler --
    /// `parse_source_type` matches on `&str`. A Source added to one and not the
    /// other persists fine and then fails to list, and because the failure is
    /// per-row inside `query_map` it takes the *whole* Library listing down,
    /// not just its own row.
    #[test]
    fn every_source_type_survives_a_persist_and_list_round_trip() {
        for source_type in [
            SourceType::Openstax,
            SourceType::Libretexts,
            SourceType::Pressbooks,
            SourceType::Epub,
            SourceType::Pdf,
            SourceType::Pasted,
            SourceType::Url,
        ] {
            let (_dir, pool) = temporary_pool();
            let mut conn = pool.get().expect("a connection should be available");

            let document_id = DocumentBuilder {
                title: "A Book".to_string(),
                source_type,
                source_metadata: serde_json::json!({}),
                source_language: "es".to_string(),
                cover_image_path: None,
                license: None,
                attribution: None,
                sections: vec![SectionBuilder::text("One", vec!["A sentence.".to_string()])],
            }
            .persist(&mut conn)
            .expect("the document should persist");

            let source_language: String = conn
                .query_row(
                    "SELECT source_language FROM documents WHERE id = ?1",
                    [&document_id],
                    |row| row.get(0),
                )
                .expect("the source language should persist");

            let documents = super::list_documents(&conn)
                .unwrap_or_else(|error| panic!("{source_type:?} should list back: {error}"));

            assert_eq!(documents.len(), 1);
            assert_eq!(documents[0].source_type, source_type);
            assert_eq!(source_language, "es");
        }
    }

    /// `source_page_cache` outlives an Import by design -- it is what makes a
    /// second Import of the same book cheap -- but a reader who deletes the
    /// Document has no other way to force a re-download, so the deletion has
    /// to reach the cache too.
    #[test]
    fn deleting_a_libretexts_document_clears_its_cached_pages() {
        let (_dir, pool) = temporary_pool();
        let mut conn = pool.get().expect("a connection should be available");

        let document_id = DocumentBuilder {
            title: "A Book".to_string(),
            source_type: SourceType::Libretexts,
            source_metadata: serde_json::json!({"book_id": "bio-15711"}),
            source_language: "en".to_string(),
            cover_image_path: None,
            license: None,
            attribution: None,
            sections: vec![SectionBuilder::text("One", vec!["A sentence.".to_string()])],
        }
        .persist(&mut conn)
        .expect("the document should persist");

        conn.execute(
            "INSERT INTO source_page_cache
                (source, cache_key, book_id, page_id, content_gzip, content_revision, fetched_at)
             VALUES ('libretexts', 'bio-15711:1', 'bio-15711', '1', X'1f8b', NULL, '2026-01-01T00:00:00Z')",
            [],
        )
        .expect("seeding a cache row should succeed");

        super::delete_document(&conn, &document_id).expect("delete should succeed");

        let remaining: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM source_page_cache WHERE book_id = 'bio-15711'",
                [],
                |row| row.get(0),
            )
            .expect("the count query should succeed");

        assert_eq!(
            remaining, 0,
            "deleting the Document should clear its cached pages"
        );
    }

    /// A cursor written in one session has to read back in the next. Without
    /// this the row is write-only: `save_playback_state` fills it on every
    /// seek and nothing has ever looked at it, so every book reopens at its
    /// first paragraph.
    #[test]
    fn a_saved_playback_cursor_reads_back() {
        let (_dir, pool) = temporary_pool();
        let mut conn = pool.get().expect("a connection should be available");

        let document_id = DocumentBuilder {
            title: "A Book".to_string(),
            source_type: SourceType::Openstax,
            source_metadata: serde_json::json!({}),
            source_language: "en".to_string(),
            cover_image_path: None,
            license: None,
            attribution: None,
            sections: vec![
                SectionBuilder::text("One", vec!["A sentence.".to_string()]),
                SectionBuilder::text(
                    "Two",
                    vec!["Another sentence.".to_string(), "A third.".to_string()],
                ),
            ],
        }
        .persist(&mut conn)
        .expect("the document should persist");

        let sections = super::list_sections(&conn, &document_id).expect("sections should list");
        let paragraphs =
            super::list_paragraphs(&conn, &sections[1].id, None).expect("paragraphs should list");

        let cursor = PlaybackState {
            document_id: document_id.clone(),
            section_id: sections[1].id.clone(),
            paragraph_id: paragraphs[1].id.clone(),
            sentence_index: 3,
            sentence_offset_ms: 0,
            voice_id: "F3".to_string(),
            speed: 1.25,
            updated_at: "2026-08-23T12:00:00Z"
                .parse::<chrono::DateTime<chrono::Utc>>()
                .expect("the timestamp should parse"),
        };
        super::save_playback_state(&conn, &cursor).expect("the cursor should save");

        let restored = super::get_playback_state(&conn, &document_id)
            .expect("the read should succeed")
            .expect("a saved cursor should come back");

        assert_eq!(restored.section_id, sections[1].id);
        assert_eq!(restored.paragraph_id, paragraphs[1].id);
        assert_eq!(restored.sentence_index, 3);
        assert_eq!(restored.voice_id, "F3");
        assert_eq!(restored.speed, 1.25);
        assert_eq!(restored.updated_at, cursor.updated_at);
    }

    /// Nothing to resume is the ordinary case for a freshly imported book, and
    /// it must be distinguishable from a failed read -- the caller opens at the
    /// start for the first and shows an error for the second.
    #[test]
    fn a_document_with_no_saved_cursor_reads_back_as_none() {
        let (_dir, pool) = temporary_pool();
        let mut conn = pool.get().expect("a connection should be available");

        let document_id = DocumentBuilder {
            title: "A Book".to_string(),
            source_type: SourceType::Openstax,
            source_metadata: serde_json::json!({}),
            source_language: "en".to_string(),
            cover_image_path: None,
            license: None,
            attribution: None,
            sections: vec![SectionBuilder::text("One", vec!["A sentence.".to_string()])],
        }
        .persist(&mut conn)
        .expect("the document should persist");

        let restored =
            super::get_playback_state(&conn, &document_id).expect("the read should succeed");

        assert!(restored.is_none(), "an unread book has no cursor to resume");
    }

    /// The Library card's progress bar is the only place a reader sees how far
    /// into a book they are, and it is derived here rather than stored: the
    /// cursor is the single source of truth, so a bar can never disagree with
    /// where the reader actually resumes.
    ///
    /// Two sections of 2 and 4 words. A cursor on the second section's second
    /// paragraph (ordinal 1 of 2) is 2 words of finished section plus half of
    /// the 4-word section in progress, over 6 words in the book.
    #[test]
    fn a_saved_cursor_gives_the_document_word_weighted_progress() {
        let (_dir, pool) = temporary_pool();
        let mut conn = pool.get().expect("a connection should be available");

        let document_id = DocumentBuilder {
            title: "A Book".to_string(),
            source_type: SourceType::Openstax,
            source_metadata: serde_json::json!({}),
            source_language: "en".to_string(),
            cover_image_path: None,
            license: None,
            attribution: None,
            sections: vec![
                SectionBuilder::text("One", vec!["A sentence.".to_string()]),
                SectionBuilder::text(
                    "Two",
                    vec!["Another sentence.".to_string(), "A third.".to_string()],
                ),
            ],
        }
        .persist(&mut conn)
        .expect("the document should persist");

        let sections = super::list_sections(&conn, &document_id).expect("sections should list");
        let paragraphs =
            super::list_paragraphs(&conn, &sections[1].id, None).expect("paragraphs should list");

        super::save_playback_state(
            &conn,
            &PlaybackState {
                document_id: document_id.clone(),
                section_id: sections[1].id.clone(),
                paragraph_id: paragraphs[1].id.clone(),
                sentence_index: 0,
                sentence_offset_ms: 0,
                voice_id: "M1".to_string(),
                speed: 1.0,
                updated_at: chrono::Utc::now(),
            },
        )
        .expect("the cursor should save");

        let documents = super::list_documents(&conn).expect("documents should list");

        assert_eq!(documents.len(), 1);
        assert!(
            (documents[0].progress - 4.0 / 6.0).abs() < 1e-6,
            "expected two thirds, got {}",
            documents[0].progress
        );
    }

    /// A book nobody has opened must read as zero, not as absent -- the card
    /// renders the bar either way, and a NULL reaching it would render as an
    /// empty style attribute rather than an empty bar.
    #[test]
    fn a_document_with_no_cursor_has_zero_progress() {
        let (_dir, pool) = temporary_pool();
        let mut conn = pool.get().expect("a connection should be available");

        DocumentBuilder {
            title: "A Book".to_string(),
            source_type: SourceType::Openstax,
            source_metadata: serde_json::json!({}),
            source_language: "en".to_string(),
            cover_image_path: None,
            license: None,
            attribution: None,
            sections: vec![SectionBuilder::text("One", vec!["A sentence.".to_string()])],
        }
        .persist(&mut conn)
        .expect("the document should persist");

        let documents = super::list_documents(&conn).expect("documents should list");

        assert_eq!(documents[0].progress, 0.0);
    }
}
