use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

use crate::db::models::{Document, Paragraph, Section, SourceType};
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
        let sentence_offsets: String = row.get(4)?;
        Ok(Paragraph {
            id: row.get(0)?,
            section_id: row.get(1)?,
            ordinal: row.get(2)?,
            text: row.get(3)?,
            sentence_offsets: serde_json::from_str(&sentence_offsets).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    4,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?,
        })
    })?;

    collect_rows(rows)
}

pub fn delete_document(conn: &Connection, id: &str) -> AppResult<()> {
    let cover_image_path = conn
        .query_row(
            "SELECT cover_image_path FROM documents WHERE id = ?1",
            params![id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();

    conn.execute("DELETE FROM documents WHERE id = ?1", params![id])?;

    if let Some(path) = cover_image_path {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }

    Ok(())
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

fn parse_source_type(value: &str) -> AppResult<SourceType> {
    match value {
        "openstax" => Ok(SourceType::Openstax),
        "libretexts" => Ok(SourceType::Libretexts),
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
