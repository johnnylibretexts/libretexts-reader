#![allow(dead_code)]

use rusqlite::{params, Connection};
use serde::Deserialize;

use crate::db::models::Voice;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceManifest {
    pub voices: Vec<VoiceMetadata>,
    pub bundle_url_template: String,
    pub mirror_url_template: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceMetadata {
    pub id: String,
    pub display_name: String,
    pub language: String,
    pub gender: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub is_bundled_default: bool,
}

pub fn load_voice_manifest() -> AppResult<VoiceManifest> {
    if let Some(path) = std::env::var_os("JOHNNY_READER_VOICE_MANIFEST_PATH") {
        let raw = std::fs::read_to_string(path)?;
        return Ok(serde_json::from_str(&raw)?);
    }

    let manifest = include_str!("../../resources/voices-manifest.json");
    Ok(serde_json::from_str(manifest)?)
}

pub fn voice_metadata(voice_id: &str) -> AppResult<VoiceMetadata> {
    load_voice_manifest()?
        .voices
        .into_iter()
        .find(|voice| voice.id == voice_id)
        .ok_or_else(|| AppError::InvalidInput(format!("unknown voice id: {voice_id}")))
}

pub fn seed_voice_catalog(conn: &mut Connection) -> AppResult<()> {
    let manifest = load_voice_manifest()?;
    let tx = conn.transaction()?;

    for voice in manifest.voices {
        tx.execute(
            "INSERT INTO voices (
                id, display_name, language, gender,
                is_bundled, is_downloaded, size_bytes, preview_path
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL)
            ON CONFLICT(id) DO UPDATE SET
                display_name = excluded.display_name,
                language = excluded.language,
                gender = excluded.gender,
                is_bundled = excluded.is_bundled,
                is_downloaded = CASE
                    WHEN excluded.is_bundled = 1 THEN 1
                    ELSE voices.is_downloaded
                END,
                size_bytes = excluded.size_bytes",
            params![
                voice.id,
                voice.display_name,
                voice.language,
                voice.gender,
                bool_to_sql(voice.is_bundled_default),
                bool_to_sql(voice.is_bundled_default),
                voice.size_bytes as i64,
            ],
        )?;
    }

    tx.commit()?;
    Ok(())
}

pub fn list_seeded_voices(conn: &Connection) -> AppResult<Vec<Voice>> {
    let mut statement = conn.prepare(
        "SELECT id, display_name, language, gender,
                is_bundled, is_downloaded, size_bytes, preview_path
         FROM voices
         ORDER BY id",
    )?;

    let voices = statement
        .query_map([], |row| {
            Ok(Voice {
                id: row.get(0)?,
                display_name: row.get(1)?,
                language: row.get(2)?,
                gender: row.get(3)?,
                is_bundled: sql_to_bool(row.get(4)?),
                is_downloaded: sql_to_bool(row.get(5)?),
                size_bytes: row.get::<_, i64>(6)? as u64,
                preview_path: row.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(voices)
}

pub fn set_voice_downloaded(
    conn: &Connection,
    voice_id: &str,
    is_downloaded: bool,
) -> AppResult<()> {
    let rows = conn.execute(
        "UPDATE voices
         SET is_downloaded = ?2,
             preview_path = CASE WHEN ?2 = 1 THEN preview_path ELSE NULL END
         WHERE id = ?1",
        params![voice_id, bool_to_sql(is_downloaded)],
    )?;

    if rows == 0 {
        return Err(AppError::InvalidInput(format!(
            "unknown voice id: {voice_id}"
        )));
    }

    Ok(())
}

fn bool_to_sql(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

fn sql_to_bool(value: i64) -> bool {
    value != 0
}
