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
        crate::voices::manifest::seed_voice_catalog(&mut conn)?;
    }

    Ok(pool)
}
