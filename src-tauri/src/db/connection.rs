use std::path::Path;

use r2d2::{CustomizeConnection, Pool};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;

use crate::db::migrations;
use crate::error::AppResult;

pub type DbPool = Pool<SqliteConnectionManager>;

#[derive(Debug)]
struct SqliteConnectionCustomizer;

impl CustomizeConnection<Connection, rusqlite::Error> for SqliteConnectionCustomizer {
    fn on_acquire(&self, conn: &mut Connection) -> Result<(), rusqlite::Error> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Ok(())
    }
}

pub fn init_pool(db_path: &Path) -> AppResult<DbPool> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let manager = SqliteConnectionManager::file(db_path);
    let pool = Pool::builder()
        .max_size(4)
        .connection_customizer(Box::new(SqliteConnectionCustomizer))
        .build(manager)?;

    {
        let mut conn = pool.get()?;
        migrations::apply_migrations(&mut conn)?;
        crate::db::settings::seed_default_settings(&conn)?;
    }

    Ok(pool)
}

/// A database in a throwaway directory, for tests.
///
/// The pool takes a file path and has no in-memory constructor, and an
/// in-memory one would not do: each pooled connection opens its own separate
/// database unless a shared-cache URI is used, so a row written through one
/// connection would be invisible to the next. The returned `TempDir` must
/// outlive the pool -- dropping it deletes the database.
///
/// This lives here so no test reaches for `LIBRETEXTS_READER_APP_DATA_DIR`:
/// `set_var` is process-global and Rust runs tests as threads in one process,
/// so one test's override can race another's.
#[cfg(test)]
pub fn temporary_pool() -> (tempfile::TempDir, DbPool) {
    let dir = tempfile::tempdir().expect("temporary directory should be created");
    let pool = init_pool(&dir.path().join("library.sqlite"))
        .expect("temporary database should initialize");
    (dir, pool)
}
