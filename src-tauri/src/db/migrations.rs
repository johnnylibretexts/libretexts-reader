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
    (
        "0007_drop_kokoro_voices",
        include_str!("../../resources/migrations/0007_drop_kokoro_voices.sql"),
    ),
    (
        "0008_source_page_cache",
        include_str!("../../resources/migrations/0008_source_page_cache.sql"),
    ),
    (
        "0009_pressbooks_catalog",
        include_str!("../../resources/migrations/0009_pressbooks_catalog.sql"),
    ),
    (
        "0010_documents_allow_pressbooks",
        include_str!("../../resources/migrations/0010_documents_allow_pressbooks.sql"),
    ),
    (
        "0011_pressbooks_partial_crawl",
        include_str!("../../resources/migrations/0011_pressbooks_partial_crawl.sql"),
    ),
];

pub fn apply_migrations(conn: &mut Connection) -> AppResult<()> {
    apply_migration_list(conn, MIGRATIONS)
}

/// Apply a specific list, so a test can stop at the migration before the one
/// it is exercising. Production always passes the whole of `MIGRATIONS`.
fn apply_migration_list(conn: &mut Connection, migrations: &[(&str, &str)]) -> AppResult<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS _migrations (
            name TEXT PRIMARY KEY,
            applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )?;

    for (name, sql) in migrations {
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
    use rusqlite::{params, Connection};

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

    /// A connection with every migration up to and including `name` applied.
    fn migrated_conn_through(name: &str) -> Connection {
        let count = super::MIGRATIONS
            .iter()
            .position(|(n, _)| *n == name)
            .unwrap_or_else(|| panic!("migration {name} is registered"))
            + 1;
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.pragma_update(None, "foreign_keys", "ON")
            .expect("enable foreign keys");
        super::apply_migration_list(&mut conn, &super::MIGRATIONS[..count])
            .expect("apply migrations");
        conn
    }

    #[test]
    fn source_page_cache_carries_the_libretexts_rows_across() {
        let mut conn = migrated_conn_through("0007_drop_kokoro_voices");
        conn.execute(
            "INSERT INTO libretexts_cache
                 (cache_key, book_id, page_id, content_gzip, content_revision, fetched_at)
             VALUES ('bio-15711:4211', 'bio-15711', '4211', ?1, 'rev-9', '2026-01-17T00:00:00Z')",
            params![b"gzipped page bytes".to_vec()],
        )
        .expect("seed a cached page from before the migration");

        super::apply_migration_list(&mut conn, super::MIGRATIONS)
            .expect("apply the source-keyed cache migration");

        let (source, book_id, page_id, content, revision, fetched_at) = conn
            .query_row(
                "SELECT source, book_id, page_id, content_gzip, content_revision, fetched_at
                 FROM source_page_cache WHERE cache_key = 'bio-15711:4211'",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Vec<u8>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .expect("the cached page should survive the migration");

        assert_eq!(source, "libretexts");
        assert_eq!(book_id, "bio-15711");
        assert_eq!(page_id, "4211");
        assert_eq!(content, b"gzipped page bytes".to_vec());
        assert_eq!(revision.as_deref(), Some("rev-9"));
        assert_eq!(fetched_at, "2026-01-17T00:00:00Z");
    }

    #[test]
    fn the_source_specific_cache_table_is_gone() {
        let conn = migrated_conn();
        let remaining: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name = 'libretexts_cache'",
                [],
                |row| row.get(0),
            )
            .expect("read the schema");

        assert_eq!(remaining, 0, "libretexts_cache should have been replaced");
    }

    #[test]
    fn two_sources_can_cache_the_same_page_id_without_collision() {
        let conn = migrated_conn();
        conn.execute_batch(
            "INSERT INTO source_page_cache
                 (source, cache_key, book_id, page_id, content_gzip, content_revision, fetched_at)
             VALUES ('libretexts', 'book:1', 'book', '1', x'01', NULL, 'now'),
                    ('pressbooks',  'book:1', 'book', '1', x'02', NULL, 'now');",
        )
        .expect("two Sources should be able to hold the same cache key");

        let content: Vec<u8> = conn
            .query_row(
                "SELECT content_gzip FROM source_page_cache
                 WHERE source = 'pressbooks' AND cache_key = 'book:1'",
                [],
                |row| row.get(0),
            )
            .expect("read the second Source's row");

        assert_eq!(content, vec![0x02]);
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

    #[test]
    fn drop_kokoro_voices_removes_the_voices_table() {
        let conn = migrated_conn();
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'voices')",
                [],
                |row| row.get(0),
            )
            .expect("query sqlite_master");
        assert!(!exists, "the voices table must not survive migration 0007");
    }

    #[test]
    fn drop_kokoro_voices_rewrites_a_stored_kokoro_provider() {
        let conn = migrated_conn();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('tts_provider', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params!["\"kokoro\""],
        )
        .expect("seed a stored kokoro provider");

        conn.execute_batch(migration_sql("0007_drop_kokoro_voices"))
            .expect("re-apply the migration");

        let value: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'tts_provider'",
                [],
                |r| r.get(0),
            )
            .expect("read tts_provider");
        assert_eq!(value, "\"supertonic\"");
    }

    #[test]
    fn drop_kokoro_voices_rewrites_a_stored_kokoro_voice_id() {
        let conn = migrated_conn();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('default_voice_id', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params!["\"af_heart\""],
        )
        .expect("seed a stored kokoro voice id");

        conn.execute_batch(migration_sql("0007_drop_kokoro_voices"))
            .expect("re-apply the migration");

        let value: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'default_voice_id'",
                [],
                |r| r.get(0),
            )
            .expect("read default_voice_id");
        assert_eq!(
            value, "\"M1\"",
            "a Kokoro voice id would otherwise be swapped for M1 on every sentence forever"
        );
    }

    #[test]
    fn drop_kokoro_voices_leaves_a_supertonic_voice_id_alone() {
        let conn = migrated_conn();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('default_voice_id', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params!["\"F3\""],
        )
        .expect("seed a chosen Supertonic voice");

        conn.execute_batch(migration_sql("0007_drop_kokoro_voices"))
            .expect("run once");
        conn.execute_batch(migration_sql("0007_drop_kokoro_voices"))
            .expect("run twice");

        let value: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'default_voice_id'",
                [],
                |r| r.get(0),
            )
            .expect("read default_voice_id");
        assert_eq!(
            value, "\"F3\"",
            "the migration must not flatten a voice the reader deliberately chose"
        );
    }

    #[test]
    fn drop_kokoro_voices_drops_the_dead_model_settings() {
        let conn = migrated_conn();
        conn.execute_batch(
            "INSERT INTO settings (key, value) VALUES ('model_precision', '\"q8\"')
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value;
             INSERT INTO settings (key, value) VALUES ('model_downloaded', 'true')
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value;",
        )
        .expect("seed the dead model settings");

        conn.execute_batch(migration_sql("0007_drop_kokoro_voices"))
            .expect("run once");
        conn.execute_batch(migration_sql("0007_drop_kokoro_voices"))
            .expect("run twice");

        let remaining: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM settings
                     WHERE key IN ('model_precision', 'model_downloaded')",
                [],
                |r| r.get(0),
            )
            .expect("count dead settings");
        assert_eq!(remaining, 0);
    }
}
