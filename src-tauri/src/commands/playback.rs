use rusqlite::params;
use tauri::State;

use crate::db::connection::DbPool;
use crate::db::models::PlaybackState;
use crate::error::AppResult;

#[tauri::command]
pub async fn save_playback_state(
    state: State<'_, DbPool>,
    playback: PlaybackState,
) -> AppResult<()> {
    let conn = state.get()?;
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
