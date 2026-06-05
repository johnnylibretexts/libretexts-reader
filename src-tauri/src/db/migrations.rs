use rusqlite::{params, Connection};

use crate::error::AppResult;

const MIGRATIONS: &[(&str, &str)] = &[
    (
        "0001_initial_schema",
        include_str!("../../resources/migrations/0001_initial_schema.sql"),
    ),
    (
        "0002_libretexts_import",
        include_str!("../../resources/migrations/0002_libretexts_import.sql"),
    ),
    (
        "0003_section_images",
        include_str!("../../resources/migrations/0003_section_images.sql"),
    ),
    (
        "0004_section_image_anchors",
        include_str!("../../resources/migrations/0004_section_image_anchors.sql"),
    ),
];

pub fn apply_migrations(conn: &mut Connection) -> AppResult<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS _migrations (
            name TEXT PRIMARY KEY,
            applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )?;

    for (name, sql) in MIGRATIONS {
        let already_applied: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM _migrations WHERE name = ?1)",
            params![name],
            |row| row.get(0),
        )?;

        if already_applied {
            continue;
        }

        let foreign_keys_enabled: bool =
            conn.query_row("PRAGMA foreign_keys", [], |row| row.get::<_, u32>(0))? != 0;
        if foreign_keys_enabled {
            conn.pragma_update(None, "foreign_keys", "OFF")?;
        }

        let result = (|| -> AppResult<()> {
            let tx = conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.execute("INSERT INTO _migrations (name) VALUES (?1)", params![name])?;
            tx.commit()?;
            Ok(())
        })();

        if foreign_keys_enabled {
            conn.pragma_update(None, "foreign_keys", "ON")?;
        }

        result?;
    }

    Ok(())
}
