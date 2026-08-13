use std::collections::HashMap;
use std::path::PathBuf;

use rusqlite::{params, Connection};
use serde_json::{json, Value};

use crate::error::AppResult;
use crate::tts::supertonic::voice::is_valid_supertonic_voice_style;

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
            if migrate_removed_setting(key, &mut value) {
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
    migrate_removed_setting(key, &mut value);

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
        if migrate_removed_setting(&key, &mut value) {
            set_setting(conn, &key, &value)?;
        }
        settings.insert(key, value);
    }

    Ok(settings)
}

fn default_settings() -> AppResult<Vec<(&'static str, Value)>> {
    Ok(vec![
        ("default_voice_id", json!("M1")),
        ("default_speed", json!(1.0)),
        ("export_directory", json!(default_export_directory())),
        ("theme", json!("system")),
        ("telemetry_opt_in", json!(false)),
        ("auto_check_updates", json!(true)),
        ("tts_provider", json!("supertonic")),
        ("supertonic_voice_style", json!("M1")),
        ("supertonic_language", json!("en")),
    ])
}

/// Rewrite a stored value that names something the app no longer has.
///
/// Returns true when the value changed, which is the caller's signal to write
/// it back. Retired providers: `system` was the Web Speech path; `gemini` and
/// `fish` predate Supertonic; `kokoro` was removed once it proved it could not
/// produce audio in a bundled build.
fn migrate_removed_setting(key: &str, value: &mut Value) -> bool {
    match key {
        "tts_provider" => migrate_removed_tts_provider(value),
        "default_voice_id" => migrate_removed_voice_id(value),
        _ => false,
    }
}

fn migrate_removed_tts_provider(value: &mut Value) -> bool {
    match value.as_str() {
        Some("gemini" | "fish" | "kokoro") => {
            *value = json!("supertonic");
            true
        }
        _ => false,
    }
}

/// A voice id belonging to a removed engine is not merely stale: the Supertonic
/// adapter falls back rather than failing, so it would be silently swapped for
/// the default on every sentence, forever, with nothing surfaced to the reader.
fn migrate_removed_voice_id(value: &mut Value) -> bool {
    match value.as_str() {
        Some(voice_style) if !is_valid_supertonic_voice_style(voice_style) => {
            *value = json!("M1");
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

#[cfg(test)]
mod tests {
    use super::{get_all_settings, get_setting, set_setting};
    use rusqlite::Connection;
    use serde_json::json;

    fn settings_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
            .expect("create settings table");
        conn
    }

    fn raw_value(conn: &Connection, key: &str) -> String {
        conn.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            rusqlite::params![key],
            |row| row.get(0),
        )
        .expect("read raw value")
    }

    #[test]
    fn reading_a_stored_kokoro_provider_migrates_and_persists_it() {
        let conn = settings_conn();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('tts_provider', ?1)",
            rusqlite::params!["\"kokoro\""],
        )
        .expect("seed kokoro");

        let value = get_setting(&conn, "tts_provider").expect("read").unwrap();

        assert_eq!(value, json!("supertonic"));
        assert_eq!(
            raw_value(&conn, "tts_provider"),
            "\"supertonic\"",
            "the migrated value must be written back, not just returned"
        );
    }

    #[test]
    fn writing_a_kokoro_provider_stores_supertonic_instead() {
        let conn = settings_conn();
        set_setting(&conn, "tts_provider", &json!("kokoro")).expect("write");
        assert_eq!(raw_value(&conn, "tts_provider"), "\"supertonic\"");
    }

    #[test]
    fn reading_a_stored_kokoro_voice_id_migrates_it() {
        let conn = settings_conn();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('default_voice_id', ?1)",
            rusqlite::params!["\"af_heart\""],
        )
        .expect("seed a kokoro voice id");

        let all = get_all_settings(&conn).expect("read all");

        assert_eq!(all.get("default_voice_id"), Some(&json!("M1")));
        assert_eq!(raw_value(&conn, "default_voice_id"), "\"M1\"");
    }

    #[test]
    fn a_chosen_supertonic_voice_id_is_left_alone() {
        let conn = settings_conn();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('default_voice_id', ?1)",
            rusqlite::params!["\"F3\""],
        )
        .expect("seed a chosen voice");

        let value = get_setting(&conn, "default_voice_id")
            .expect("read")
            .unwrap();

        assert_eq!(value, json!("F3"));
    }
}
