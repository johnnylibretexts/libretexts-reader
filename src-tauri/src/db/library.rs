use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

use crate::content::normalize::normalize_for_tts;
use crate::db::models::{Document, Paragraph, Section, SectionImage, SourceType};
use crate::error::{AppError, AppResult};

pub fn list_documents(conn: &Connection) -> AppResult<Vec<Document>> {
    let mut statement = conn.prepare(
        "SELECT id, title, source_type, source_metadata, cover_image_path,
                license, attribution, word_count, imported_at, last_opened_at
         FROM documents
         ORDER BY COALESCE(last_opened_at, imported_at) DESC",
    )?;
    let rows = statement.query_map([], document_from_row)?;
    collect_rows(rows)
}

pub fn search_documents(conn: &Connection, query: &str) -> AppResult<Vec<Document>> {
    let pattern = format!("%{}%", query.trim());
    let mut statement = conn.prepare(
        "SELECT id, title, source_type, source_metadata, cover_image_path,
                license, attribution, word_count, imported_at, last_opened_at
         FROM documents
         WHERE title LIKE ?1
         ORDER BY COALESCE(last_opened_at, imported_at) DESC",
    )?;
    let rows = statement.query_map(params![pattern], document_from_row)?;
    collect_rows(rows)
}

pub fn get_document(conn: &Connection, id: &str) -> AppResult<Document> {
    let mut statement = conn.prepare(
        "SELECT id, title, source_type, source_metadata, cover_image_path,
                license, attribution, word_count, imported_at, last_opened_at
         FROM documents
         WHERE id = ?1",
    )?;

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

pub fn list_paragraphs(conn: &Connection, section_id: &str) -> AppResult<Vec<Paragraph>> {
    let mut statement = conn.prepare(
        "SELECT id, section_id, ordinal, text, sentence_offsets
         FROM paragraphs
         WHERE section_id = ?1
         ORDER BY ordinal",
    )?;
    let rows = statement.query_map(params![section_id], |row| {
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

        // Every Paragraph carries both forms, so this is the one place they can
        // fall out of step — and they cannot, because the speech form is
        // derived from the display form right here, sentence by sentence.
        let sentence_speech = sentence_offsets
            .iter()
            .map(|(start, end)| normalize_for_tts(text.get(*start..*end).unwrap_or_default()))
            .collect();

        Ok(Paragraph {
            id: row.get(0)?,
            section_id: row.get(1)?,
            ordinal: row.get(2)?,
            text,
            sentence_offsets,
            sentence_speech,
        })
    })?;

    collect_rows(rows)
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

fn collect_rows<T>(rows: impl Iterator<Item = Result<T, rusqlite::Error>>) -> AppResult<Vec<T>> {
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn document_from_row(row: &rusqlite::Row<'_>) -> Result<Document, rusqlite::Error> {
    let source_type: String = row.get(2)?;
    let source_metadata: String = row.get(3)?;
    let imported_at: String = row.get(8)?;
    let last_opened_at: Option<String> = row.get(9)?;

    Ok(Document {
        id: row.get(0)?,
        title: row.get(1)?,
        source_type: parse_source_type(&source_type).map_err(to_sql_conversion_error(2))?,
        source_metadata: parse_json(&source_metadata).map_err(to_sql_conversion_error(3))?,
        cover_image_path: row.get(4)?,
        license: row.get(5)?,
        attribution: row.get(6)?,
        word_count: row.get(7)?,
        imported_at: parse_datetime(&imported_at).map_err(to_sql_conversion_error(8))?,
        last_opened_at: last_opened_at
            .as_deref()
            .map(parse_datetime)
            .transpose()
            .map_err(to_sql_conversion_error(9))?,
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
    use crate::content::document::{DocumentBuilder, SectionBuilder};
    use crate::db::connection::temporary_pool;
    use crate::db::models::SourceType;

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

            DocumentBuilder {
                title: "A Book".to_string(),
                source_type,
                source_metadata: serde_json::json!({}),
                cover_image_path: None,
                license: None,
                attribution: None,
                sections: vec![SectionBuilder::text("One", vec!["A sentence.".to_string()])],
            }
            .persist(&mut conn)
            .expect("the document should persist");

            let documents = super::list_documents(&conn)
                .unwrap_or_else(|error| panic!("{source_type:?} should list back: {error}"));

            assert_eq!(documents.len(), 1);
            assert_eq!(documents[0].source_type, source_type);
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
}
