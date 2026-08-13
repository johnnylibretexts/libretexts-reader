use rusqlite::{params, Connection};

use crate::error::{AppError, AppResult};

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
    (
        "0005_rebase_app_dir_paths",
        include_str!("../../resources/migrations/0005_rebase_app_dir_paths.sql"),
    ),
    (
        "0006_rebase_export_directory",
        include_str!("../../resources/migrations/0006_rebase_export_directory.sql"),
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
            // Validate referential integrity before recording the migration as
            // applied. Foreign keys are disabled during the batch, so a bad
            // migration could otherwise leave dangling references behind.
            let violations: i64 = tx.query_row(
                "SELECT COUNT(*) FROM pragma_foreign_key_check()",
                [],
                |row| row.get(0),
            )?;
            if violations > 0 {
                return Err(AppError::Migration(format!(
                    "migration {name} left {violations} foreign key violation(s)"
                )));
            }
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

#[cfg(test)]
mod tests {
    use super::apply_migrations;
    use rusqlite::Connection;

    fn migrated_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.pragma_update(None, "foreign_keys", "ON")
            .expect("enable foreign keys");
        apply_migrations(&mut conn).expect("apply migrations");
        conn
    }

    fn migration_sql(name: &str) -> &'static str {
        super::MIGRATIONS
            .iter()
            .find(|(n, _)| *n == name)
            .unwrap_or_else(|| panic!("migration {name} is registered"))
            .1
    }

    #[test]
    fn rebase_app_dir_paths_rewrites_the_old_identifier() {
        let conn = migrated_conn();
        conn.execute_batch(
            "INSERT INTO documents (id, title, source_type, source_metadata, imported_at, cover_image_path)
                 VALUES ('doc1', 'D', 'pasted', '{}', 'now',
                         '/Users/x/Library/Application Support/dev.johnnyrobot.reader/covers/c.png');
             INSERT INTO sections (id, document_id, ordinal, title)
                 VALUES ('sec1', 'doc1', 0, 'S');
             INSERT INTO section_images (id, section_id, ordinal, source_url, local_path)
                 VALUES ('img1', 'sec1', 0, 'https://e.test/i.png',
                         '/Users/x/Library/Application Support/dev.johnnyrobot.reader/images/i.png');",
        )
        .expect("seed rows carrying the old identifier");

        conn.execute_batch(migration_sql("0005_rebase_app_dir_paths"))
            .expect("re-apply the rebase migration");

        let cover: String = conn
            .query_row(
                "SELECT cover_image_path FROM documents WHERE id = 'doc1'",
                [],
                |r| r.get(0),
            )
            .expect("read cover path");
        let image: String = conn
            .query_row(
                "SELECT local_path FROM section_images WHERE id = 'img1'",
                [],
                |r| r.get(0),
            )
            .expect("read image path");

        assert!(
            cover.contains("dev.johnnylibretexts.reader"),
            "cover not rebased: {cover}"
        );
        assert!(
            !cover.contains("dev.johnnyrobot.reader"),
            "old prefix survived: {cover}"
        );
        assert!(
            image.contains("dev.johnnylibretexts.reader"),
            "image not rebased: {image}"
        );
        assert!(
            !image.contains("dev.johnnyrobot.reader"),
            "old prefix survived: {image}"
        );
    }

    #[test]
    fn rebase_app_dir_paths_is_idempotent_on_a_matching_path() {
        let conn = migrated_conn();
        conn.execute_batch(
            "INSERT INTO documents (id, title, source_type, source_metadata, imported_at, cover_image_path)
                 VALUES ('doc2', 'D', 'pasted', '{}', 'now',
                         '/Users/x/Library/Application Support/dev.johnnyrobot.reader/covers/c.png');",
        )
        .expect("seed a path carrying the old identifier");

        conn.execute_batch(migration_sql("0005_rebase_app_dir_paths"))
            .expect("first run");
        conn.execute_batch(migration_sql("0005_rebase_app_dir_paths"))
            .expect("second run");

        let cover: String = conn
            .query_row(
                "SELECT cover_image_path FROM documents WHERE id = 'doc2'",
                [],
                |r| r.get(0),
            )
            .expect("read cover path");

        assert_eq!(
            cover, "/Users/x/Library/Application Support/dev.johnnylibretexts.reader/covers/c.png",
            "running the migration twice must not double-rewrite the path"
        );
    }

    #[test]
    fn rebase_app_dir_paths_leaves_other_paths_alone() {
        let conn = migrated_conn();
        conn.execute_batch(
            "INSERT INTO documents (id, title, source_type, source_metadata, imported_at, cover_image_path)
                 VALUES ('doc3', 'D', 'pasted', '{}', 'now', '/somewhere/else/c.png');",
        )
        .expect("seed an unrelated path");

        conn.execute_batch(migration_sql("0005_rebase_app_dir_paths"))
            .expect("first run");
        conn.execute_batch(migration_sql("0005_rebase_app_dir_paths"))
            .expect("second run");

        let cover: String = conn
            .query_row(
                "SELECT cover_image_path FROM documents WHERE id = 'doc3'",
                [],
                |r| r.get(0),
            )
            .expect("read cover path");
        assert_eq!(cover, "/somewhere/else/c.png");
    }

    #[test]
    fn migrations_apply_with_no_foreign_key_violations() {
        let conn = migrated_conn();
        let violations: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_foreign_key_check()",
                [],
                |row| row.get(0),
            )
            .expect("foreign key check");
        assert_eq!(violations, 0);
    }

    #[test]
    fn rebase_export_directory_rewrites_the_old_product_name() {
        let conn = migrated_conn();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('export_directory', ?1)",
            rusqlite::params!["\"/Users/x/Documents/Johnny Reader\""],
        )
        .expect("seed the old export directory");

        conn.execute_batch(migration_sql("0006_rebase_export_directory"))
            .expect("re-apply the rebase migration");

        let value: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'export_directory'",
                [],
                |r| r.get(0),
            )
            .expect("read export directory");

        assert_eq!(value, "\"/Users/x/Documents/LibreTexts Reader\"");
    }

    #[test]
    fn rebase_export_directory_is_idempotent_on_a_matching_path() {
        let conn = migrated_conn();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('export_directory', ?1)",
            rusqlite::params!["\"/Users/x/Documents/Johnny Reader\""],
        )
        .expect("seed the old export directory");

        conn.execute_batch(migration_sql("0006_rebase_export_directory"))
            .expect("run once");
        conn.execute_batch(migration_sql("0006_rebase_export_directory"))
            .expect("run twice");

        let value: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'export_directory'",
                [],
                |r| r.get(0),
            )
            .expect("read export directory");

        assert_eq!(
            value, "\"/Users/x/Documents/LibreTexts Reader\"",
            "running the migration twice must not double-rewrite the path"
        );
    }

    #[test]
    fn rebase_export_directory_leaves_a_custom_path_alone() {
        let conn = migrated_conn();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('export_directory', ?1)",
            rusqlite::params!["\"/Users/x/Music/Exports\""],
        )
        .expect("seed a custom export directory");

        conn.execute_batch(migration_sql("0006_rebase_export_directory"))
            .expect("run once");
        conn.execute_batch(migration_sql("0006_rebase_export_directory"))
            .expect("run twice");

        let value: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'export_directory'",
                [],
                |r| r.get(0),
            )
            .expect("read export directory");

        assert_eq!(value, "\"/Users/x/Music/Exports\"");
    }

    #[test]
    fn playback_state_rejects_cross_document_cursor() {
        let conn = migrated_conn();
        conn.execute_batch(
            "INSERT INTO documents (id, title, source_type, source_metadata, imported_at)
                 VALUES ('docA', 'A', 'pasted', '{}', 'now'),
                        ('docB', 'B', 'pasted', '{}', 'now');
             INSERT INTO sections (id, document_id, ordinal, title)
                 VALUES ('secA', 'docA', 0, 'A0'),
                        ('secB', 'docB', 0, 'B0');
             INSERT INTO paragraphs (id, section_id, ordinal, text, sentence_offsets)
                 VALUES ('parA', 'secA', 0, 'a', '[]'),
                        ('parB', 'secB', 0, 'b', '[]');",
        )
        .expect("seed hierarchy");

        // A consistent cursor (document -> its section -> its paragraph) inserts.
        conn.execute(
            "INSERT INTO playback_state (document_id, section_id, paragraph_id, voice_id, updated_at)
                 VALUES ('docA', 'secA', 'parA', 'v', 'now')",
            [],
        )
        .expect("consistent cursor should insert");

        // A cursor whose section belongs to another document is rejected.
        let result = conn.execute(
            "INSERT INTO playback_state (document_id, section_id, paragraph_id, voice_id, updated_at)
                 VALUES ('docB', 'secA', 'parA', 'v', 'now')",
            [],
        );
        assert!(result.is_err(), "cross-document cursor must be rejected");
    }
}
