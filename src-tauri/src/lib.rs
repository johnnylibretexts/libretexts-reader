mod cleanup;
pub mod commands;
pub mod content;
pub mod db;
pub mod error;
pub mod net;
mod paths;
pub mod secrets;
pub mod tts;

use db::connection::init_pool;
use tauri::Manager;
use tts::supertonic::model::SupertonicDownload;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            commands::library::list_documents,
            commands::library::get_document,
            commands::library::list_sections,
            commands::library::list_paragraphs,
            commands::library::list_section_images,
            commands::library::delete_document,
            commands::library::search_documents,
            commands::content::import_openstax,
            commands::content::import_libretexts,
            commands::content::import_pressbooks,
            commands::content::import_epub,
            commands::content::import_pdf,
            commands::content::import_pasted_text,
            commands::content::import_url,
            commands::content::list_openstax_catalog,
            commands::content::list_libretexts_catalog,
            commands::content::list_libretexts_libraries,
            commands::content::list_pressbooks_catalogs,
            commands::content::list_pressbooks_books,
            commands::content::search_pressbooks_books,
            commands::playback::save_playback_state,
            commands::playback::get_playback_state,
            commands::settings::get_setting,
            commands::settings::set_setting,
            commands::settings::get_all_settings,
            commands::tts::synthesize_speech,
            commands::chapter_tts::get_supertonic_model_status,
            commands::chapter_tts::ensure_supertonic_model_downloaded,
            commands::chapter_tts::cancel_supertonic_model_download,
            commands::chapter_tts::preview_supertonic_tts,
            commands::chapter_tts::estimate_supertonic_chapter,
            commands::chapter_tts::export_supertonic_chapter_mp3,
            commands::fish::get_fish_key_status,
            commands::fish::get_fish_credit,
            commands::fish::set_fish_api_key,
            commands::fish::clear_fish_api_key,
            commands::fish::list_fish_voices,
        ])
        .setup(|app| {
            let db_path = paths::database_path()?;
            paths::models_dir()?;
            paths::covers_dir()?;
            paths::images_dir()?;
            paths::cache_dir()?;
            paths::temp_dir()?;
            cleanup::reclaim_kokoro_artifacts();
            cleanup::reclaim_stale_tts_cache();
            let pool = init_pool(&db_path)?;
            app.manage(pool);
            // Shared by the two model-download commands, and the reason there
            // is only ever one download: the player and Settings can both ask
            // for the model, and the second joins the first. See
            // `SupertonicDownload`.
            app.manage(SupertonicDownload::default());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
