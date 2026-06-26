pub mod commands;
pub mod content;
pub mod db;
pub mod error;
mod paths;
mod voices;

use db::connection::init_pool;
use tauri::Manager;

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
            commands::content::import_epub,
            commands::content::import_pdf,
            commands::content::import_pasted_text,
            commands::content::import_url,
            commands::content::list_openstax_catalog,
            commands::content::list_libretexts_catalog,
            commands::content::list_libretexts_libraries,
            commands::playback::save_playback_state,
            commands::voices::list_voices,
            commands::voices::download_voice,
            commands::voices::delete_voice,
            commands::voices::ensure_model_downloaded,
            commands::voices::get_model_path,
            commands::settings::get_setting,
            commands::settings::set_setting,
            commands::settings::get_all_settings,
            commands::tts::synthesize_speech,
            commands::supertonic_tts::get_supertonic_model_status,
            commands::supertonic_tts::ensure_supertonic_model_downloaded,
            commands::supertonic_tts::preview_supertonic_tts,
            commands::supertonic_tts::estimate_supertonic_chapter,
            commands::supertonic_tts::export_supertonic_chapter_mp3,
        ])
        .setup(|app| {
            let db_path = paths::database_path()?;
            paths::models_dir()?;
            paths::voices_dir()?;
            paths::covers_dir()?;
            paths::images_dir()?;
            paths::cache_dir()?;
            paths::temp_dir()?;
            let pool = init_pool(&db_path)?;
            app.manage(pool);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
