use tauri::State;

use crate::db::connection::DbPool;
use crate::db::library;
use crate::db::models::{Document, Paragraph, Section, SectionImage};
use crate::error::AppResult;

#[tauri::command]
pub async fn list_documents(state: State<'_, DbPool>) -> AppResult<Vec<Document>> {
    let conn = state.get()?;
    library::list_documents(&conn)
}

#[tauri::command]
pub async fn get_document(state: State<'_, DbPool>, id: String) -> AppResult<Document> {
    let conn = state.get()?;
    library::get_document(&conn, &id)
}

#[tauri::command]
pub async fn list_sections(
    state: State<'_, DbPool>,
    document_id: String,
) -> AppResult<Vec<Section>> {
    let conn = state.get()?;
    library::list_sections(&conn, &document_id)
}

#[tauri::command]
pub async fn list_paragraphs(
    state: State<'_, DbPool>,
    section_id: String,
    target_lang: Option<String>,
) -> AppResult<Vec<Paragraph>> {
    let conn = state.get()?;
    library::list_paragraphs(&conn, &section_id, target_lang.as_deref())
}

#[tauri::command]
pub async fn list_section_images(
    state: State<'_, DbPool>,
    section_id: String,
) -> AppResult<Vec<SectionImage>> {
    let conn = state.get()?;
    library::list_section_images(&conn, &section_id)
}

#[tauri::command]
pub async fn delete_document(state: State<'_, DbPool>, id: String) -> AppResult<()> {
    let conn = state.get()?;
    library::delete_document(&conn, &id)
}

#[tauri::command]
pub async fn search_documents(state: State<'_, DbPool>, query: String) -> AppResult<Vec<Document>> {
    let conn = state.get()?;
    library::search_documents(&conn, &query)
}
