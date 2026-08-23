use tauri::State;

use crate::db::connection::DbPool;
use crate::db::library;
use crate::db::models::PlaybackState;
use crate::error::AppResult;

#[tauri::command]
pub async fn save_playback_state(
    state: State<'_, DbPool>,
    playback: PlaybackState,
) -> AppResult<()> {
    let conn = state.get()?;
    library::save_playback_state(&conn, &playback)
}

/// Where the reader stopped in this document, or `None` if they never started.
#[tauri::command]
pub async fn get_playback_state(
    state: State<'_, DbPool>,
    document_id: String,
) -> AppResult<Option<PlaybackState>> {
    let conn = state.get()?;
    library::get_playback_state(&conn, &document_id)
}
