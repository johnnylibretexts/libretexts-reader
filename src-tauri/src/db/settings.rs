use std::collections::HashMap;
use std::path::PathBuf;

use rusqlite::{params, Connection};
use serde_json::{json, Value};

use crate::error::AppResult;

pub fn seed_default_settings(conn: &Connection) -> AppResult<()> {
    for (key, value) in default_settings()? {
        conn.execute(
            "INSERT OR IGNORE INTO settings (key, value) VALUES (?1, ?2)",
            params![key, serde_json::to_string(&value)?],
        )?;
    }

    Ok(())
}

pub fn get_setting(conn: &Connection, key: &str) -> AppResult<Option<Value>> {
    let mut statement = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
    let result = statement.query_row(params![key], |row| row.get::<_, String>(0));

    match result {
        Ok(value) => {
            let mut value = serde_json::from_str(&value)?;
            if key == "tts_provider" && migrate_removed_tts_provider(&mut value) {
                set_setting(conn, key, &value)?;
            }
            Ok(Some(value))
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub fn set_setting(conn: &Connection, key: &str, value: &Value) -> AppResult<()> {
    let mut value = value.clone();
    if key == "tts_provider" {
        migrate_removed_tts_provider(&mut value);
    }

    conn.execute(
        "INSERT INTO settings (key, value)
         VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, serde_json::to_string(&value)?],
    )?;
    Ok(())
}

pub fn get_all_settings(conn: &Connection) -> AppResult<HashMap<String, Value>> {
    let mut statement = conn.prepare("SELECT key, value FROM settings ORDER BY key")?;
    let entries = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;

    let mut settings = HashMap::new();
    for entry in entries {
        let (key, raw_value) = entry?;
        let mut value: Value = serde_json::from_str(&raw_value)?;
        if key == "tts_provider" && migrate_removed_tts_provider(&mut value) {
            set_setting(conn, &key, &value)?;
        }
        settings.insert(key, value);
    }

    Ok(settings)
}

fn default_settings() -> AppResult<Vec<(&'static str, Value)>> {
    Ok(vec![
        ("default_voice_id", json!("af_heart")),
        ("default_speed", json!(1.0)),
        ("export_directory", json!(default_export_directory())),
        ("model_precision", json!("q8")),
        ("theme", json!("system")),
        ("telemetry_opt_in", json!(false)),
        ("auto_check_updates", json!(true)),
        ("model_downloaded", json!(false)),
        ("tts_provider", json!("kokoro")),
        ("supertonic_voice_style", json!("M1")),
        ("supertonic_language", json!("en")),
    ])
}

fn migrate_removed_tts_provider(value: &mut Value) -> bool {
    match value.as_str() {
        Some("gemini" | "fish") => {
            *value = json!("supertonic");
            true
        }
        _ => false,
    }
}

pub(crate) fn default_export_directory() -> String {
    documents_dir()
        .join("LibreTexts Reader")
        .to_string_lossy()
        .to_string()
}

fn documents_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("LIBRETEXTS_READER_DOCUMENTS_DIR") {
        return PathBuf::from(path);
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(userprofile) = std::env::var_os("USERPROFILE") {
            return PathBuf::from(userprofile).join("Documents");
        }
    }

    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join("Documents");
    }

    PathBuf::from(".")
}
