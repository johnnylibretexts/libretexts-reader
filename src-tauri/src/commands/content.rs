use serde_json::json;
use tauri::{AppHandle, Emitter, Runtime, State, Window};

use crate::content;
use crate::db::connection::DbPool;
use crate::db::models::{LibreTextsBook, LibreTextsLibrary, OpenStaxBook, PressbooksBook};
use crate::error::AppResult;

#[tauri::command]
pub async fn import_openstax<R: Runtime>(
    state: State<'_, DbPool>,
    window: Window<R>,
    book_uuid: String,
) -> AppResult<String> {
    let pool = state.inner().clone();
    let progress_window = window.clone();
    let progress_book_uuid = book_uuid.clone();
    let document = content::openstax::import_book(pool, &book_uuid, move |current, total| {
        let _ = progress_window.emit(
            "import-progress",
            json!({
                "documentId": progress_book_uuid,
                "stage": "fetching",
                "current": current,
                "total": total,
                "message": null
            }),
        );
    })
    .await?;

    let mut conn = state.get()?;
    let document_id = document.persist(&mut conn)?;
    window.emit("library-changed", json!({}))?;
    window.emit(
        "import-progress",
        json!({
            "documentId": document_id,
            "stage": "complete",
            "current": 1,
            "total": 1,
            "message": null
        }),
    )?;
    Ok(document_id)
}

#[tauri::command]
pub async fn import_libretexts<R: Runtime>(
    state: State<'_, DbPool>,
    window: Window<R>,
    book_id: String,
) -> AppResult<String> {
    let pool = state.inner().clone();
    let progress_window = window.clone();
    let progress_book_id = book_id.clone();
    let document = content::libretexts::import_book(pool, &book_id, move |current, total| {
        let _ = progress_window.emit(
            "import-progress",
            json!({
                "documentId": progress_book_id,
                "stage": "fetching",
                "current": current,
                "total": total,
                "message": null
            }),
        );
    })
    .await?;

    let mut conn = state.get()?;
    let document_id = document.persist(&mut conn)?;
    window.emit("library-changed", json!({}))?;
    window.emit(
        "import-progress",
        json!({
            "documentId": document_id,
            "stage": "complete",
            "current": 1,
            "total": 1,
            "message": null
        }),
    )?;
    Ok(document_id)
}

#[tauri::command]
pub async fn import_pressbooks<R: Runtime>(
    state: State<'_, DbPool>,
    window: Window<R>,
    book_url: String,
) -> AppResult<String> {
    let pool = state.inner().clone();
    let progress_window = window.clone();
    let progress_book_url = book_url.clone();
    let document = content::pressbooks::import_book(pool, &book_url, move |current, total| {
        let _ = progress_window.emit(
            "import-progress",
            json!({
                "documentId": progress_book_url,
                "stage": "fetching",
                "current": current,
                "total": total,
                "message": null
            }),
        );
    })
    .await?;

    // Persisted only after the whole book has been assembled. A failure above
    // returns before this line, so the Library is never left holding half a
    // Document.
    let mut conn = state.get()?;
    let document_id = document.persist(&mut conn)?;
    window.emit("library-changed", json!({}))?;
    window.emit(
        "import-progress",
        json!({
            "documentId": document_id,
            "stage": "complete",
            "current": 1,
            "total": 1,
            "message": null
        }),
    )?;
    Ok(document_id)
}

#[tauri::command]
pub async fn import_epub<R: Runtime>(
    state: State<'_, DbPool>,
    window: Window<R>,
    file_path: String,
) -> AppResult<String> {
    let mut conn = state.get()?;
    let document_id =
        content::epub::import_from_path(std::path::Path::new(&file_path))?.persist(&mut conn)?;
    window.emit("library-changed", json!({}))?;
    Ok(document_id)
}

#[tauri::command]
pub async fn import_pdf<R: Runtime>(
    state: State<'_, DbPool>,
    window: Window<R>,
    file_path: String,
) -> AppResult<String> {
    let mut conn = state.get()?;
    let document_id =
        content::pdf::import_from_path(std::path::Path::new(&file_path))?.persist(&mut conn)?;
    window.emit("library-changed", json!({}))?;
    Ok(document_id)
}

#[tauri::command]
pub async fn import_pasted_text<R: Runtime>(
    state: State<'_, DbPool>,
    app: AppHandle<R>,
    title: String,
    text: String,
) -> AppResult<String> {
    let mut conn = state.get()?;
    let document_id = content::import_pasted(&title, &text)?.persist(&mut conn)?;
    app.emit("library-changed", json!({}))?;
    Ok(document_id)
}

#[tauri::command]
pub async fn import_url<R: Runtime>(
    state: State<'_, DbPool>,
    window: Window<R>,
    url: String,
) -> AppResult<String> {
    let mut conn = state.get()?;
    let document_id = content::article::import_from_url(&url)
        .await?
        .persist(&mut conn)?;
    window.emit("library-changed", json!({}))?;
    Ok(document_id)
}

#[tauri::command]
pub async fn list_openstax_catalog(_state: State<'_, DbPool>) -> AppResult<Vec<OpenStaxBook>> {
    content::openstax::catalog()
}

#[tauri::command]
pub async fn list_libretexts_catalog(
    state: State<'_, DbPool>,
    query: Option<String>,
    library: Option<String>,
) -> AppResult<Vec<LibreTextsBook>> {
    content::libretexts::list_catalog(state.inner().clone(), query, library).await
}

#[tauri::command]
pub async fn list_libretexts_libraries(
    state: State<'_, DbPool>,
) -> AppResult<Vec<LibreTextsLibrary>> {
    content::libretexts::list_libraries(state.inner().clone()).await
}

#[tauri::command]
pub async fn list_pressbooks_catalog(state: State<'_, DbPool>) -> AppResult<Vec<PressbooksBook>> {
    content::pressbooks::list_catalog(state.inner().clone()).await
}
