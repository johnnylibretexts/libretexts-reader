use serde_json::json;
use tauri::{AppHandle, Emitter, Runtime, State, Window};

use crate::content;
use crate::content::cancel::ImportCancel;
use crate::db::connection::DbPool;
use crate::db::models::{
    LibreTextsBook, LibreTextsLibrary, OpenStaxBook, PressbooksBook, PressbooksCatalog,
    PressbooksCatalogListing,
};
use crate::error::AppResult;
use crate::paths;

#[tauri::command]
pub async fn import_openstax<R: Runtime>(
    state: State<'_, DbPool>,
    window: Window<R>,
    cancel: State<'_, ImportCancel>,
    book_uuid: String,
) -> AppResult<String> {
    let pool = state.inner().clone();
    // Cleared on the way in, never on the way out -- see `ImportCancel::clear`.
    let cancel = ImportCancel::clone(&cancel);
    cancel.clear();
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
        // Reported progress and the chance to stop are the same moment: this
        // runs after each page lands, so a cancel takes effect before the next
        // request rather than interrupting the one in flight.
        cancel.check()
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
    cancel: State<'_, ImportCancel>,
    book_id: String,
) -> AppResult<String> {
    let pool = state.inner().clone();
    let cancel = ImportCancel::clone(&cancel);
    cancel.clear();
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
        cancel.check()
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
    cancel: State<'_, ImportCancel>,
    book_url: String,
) -> AppResult<String> {
    content::pressbooks::verify_offered_book_url(&book_url)?;

    let pool = state.inner().clone();
    let cancel = ImportCancel::clone(&cancel);
    cancel.clear();
    let progress_window = window.clone();
    let progress_book_url = book_url.clone();
    // The one place the real covers directory is named. `paths::covers_dir`
    // creates it, which is why `import_book` takes it rather than resolving it.
    let covers_dir = paths::covers_dir()?;
    let document =
        content::pressbooks::import_book(pool, &book_url, &covers_dir, move |current, total| {
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
            cancel.check()
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

/// The Catalogs the picker offers. Bundled, so this needs no network.
#[tauri::command]
pub async fn list_pressbooks_catalogs(
    _state: State<'_, DbPool>,
) -> AppResult<Vec<PressbooksCatalog>> {
    content::pressbooks::catalogs()
}

/// List a Catalog, reporting crawl progress as `catalog-progress`.
///
/// Its own event rather than `import-progress`: a Catalog crawl and an Import
/// can run at the same time, and they mean different things to a reader --
/// folding them into one event would have a Catalog's pages overwrite a book's
/// sections in the same indicator.
#[tauri::command]
pub async fn list_pressbooks_books<R: Runtime>(
    state: State<'_, DbPool>,
    window: Window<R>,
    host: String,
) -> AppResult<PressbooksCatalogListing> {
    let progress_host = host.clone();
    content::pressbooks::list_books(state.inner().clone(), &host, move |current, total| {
        let _ = window.emit(
            "catalog-progress",
            json!({
                "host": progress_host.clone(),
                "current": current,
                "total": total,
            }),
        );
    })
    .await
}

/// Filter an already-listed Catalog. Local, so it costs no request -- which is
/// what lets the webview call it on every keystroke.
#[tauri::command]
pub async fn search_pressbooks_books(
    state: State<'_, DbPool>,
    host: String,
    query: String,
) -> AppResult<Vec<PressbooksBook>> {
    content::pressbooks::search_books(state.inner().clone(), &host, &query)
}

/// Ask the import in flight to stop.
///
/// Resolves once the request is recorded, not once the import has ended: the
/// fetch fails at its next page boundary, from inside whichever `import_*`
/// command is still running. Safe to call when nothing is importing -- the
/// flag is cleared by the next import that starts.
#[tauri::command]
pub async fn cancel_import(cancel: State<'_, ImportCancel>) -> AppResult<()> {
    cancel.request();
    Ok(())
}
