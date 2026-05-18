use rusqlite::{params, Connection};
use serde_json::Value;
use uuid::Uuid;

use crate::content::tokenize::sentence_boundaries;
use crate::db::models::SourceType;
use crate::error::{AppError, AppResult};

pub struct DocumentBuilder {
    pub title: String,
    pub source_type: SourceType,
    pub source_metadata: Value,
    pub cover_image_path: Option<String>,
    pub license: Option<String>,
    pub attribution: Option<String>,
    pub sections: Vec<SectionBuilder>,
}

pub struct SectionBuilder {
    pub title: String,
    pub paragraphs: Vec<String>,
}

impl DocumentBuilder {
    pub fn persist(self, db: &mut Connection) -> AppResult<String> {
        if self.sections.is_empty() {
            return Err(AppError::InvalidInput(
                "document must contain at least one section".into(),
            ));
        }

        let document_id = Uuid::new_v4().to_string();
        let imported_at = chrono::Utc::now().to_rfc3339();
        let source_metadata = serde_json::to_string(&self.source_metadata)?;
        let document_word_count = self
            .sections
            .iter()
            .flat_map(|section| section.paragraphs.iter())
            .map(|paragraph| word_count(paragraph))
            .sum::<usize>() as u32;

        let tx = db.transaction()?;
        tx.execute(
            "INSERT INTO documents (
                id, title, source_type, source_metadata, cover_image_path,
                license, attribution, word_count, imported_at, last_opened_at
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL)",
            params![
                document_id,
                self.title,
                source_type_str(self.source_type),
                source_metadata,
                self.cover_image_path,
                self.license,
                self.attribution,
                document_word_count,
                imported_at,
            ],
        )?;

        for (section_index, section) in self.sections.into_iter().enumerate() {
            let section_id = Uuid::new_v4().to_string();
            let section_word_count = section
                .paragraphs
                .iter()
                .map(|paragraph| word_count(paragraph))
                .sum::<usize>() as u32;

            tx.execute(
                "INSERT INTO sections (id, document_id, ordinal, title, word_count)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    section_id,
                    document_id,
                    section_index as u32,
                    section.title,
                    section_word_count,
                ],
            )?;

            for (paragraph_index, paragraph) in section.paragraphs.into_iter().enumerate() {
                let paragraph_id = Uuid::new_v4().to_string();
                let sentence_offsets = serde_json::to_string(&sentence_boundaries(&paragraph))?;

                tx.execute(
                    "INSERT INTO paragraphs (id, section_id, ordinal, text, sentence_offsets)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        paragraph_id,
                        section_id,
                        paragraph_index as u32,
                        paragraph,
                        sentence_offsets,
                    ],
                )?;
            }
        }

        tx.commit()?;
        Ok(document_id)
    }
}

fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

fn source_type_str(source_type: SourceType) -> &'static str {
    match source_type {
        SourceType::Openstax => "openstax",
        SourceType::Libretexts => "libretexts",
        SourceType::Epub => "epub",
        SourceType::Pdf => "pdf",
        SourceType::Pasted => "pasted",
        SourceType::Url => "url",
    }
}
