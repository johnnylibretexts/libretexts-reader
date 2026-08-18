use std::path::Path;
use std::time::Duration;

use r2d2::{CustomizeConnection, Pool};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;

use crate::db::migrations;
use crate::error::AppResult;

pub type DbPool = Pool<SqliteConnectionManager>;

#[derive(Debug)]
struct SqliteConnectionCustomizer;

/// How long a writer waits for another writer's transaction before giving up.
///
/// Stated rather than inherited. `rusqlite` opens every connection with exactly
/// this timeout already, so this changes no behaviour — it says out loud that
/// the app depends on it, instead of depending on a default it never names.
///
/// Not zero, which is what SQLite itself defaults to: at zero the second writer
/// fails the moment it touches a held lock, and two paths here open write
/// transactions without coordinating — a Document persist and a Catalog crawl,
/// which the Import guard does not cover. Nothing retries either of them.
///
/// Five seconds is generous against what those actually cost: persisting a
/// 20,000-paragraph Document holds the lock for roughly 200ms. That leaves more
/// than an order of magnitude of headroom while still surfacing a genuinely
/// stuck writer as an error rather than an indefinitely frozen call.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

impl CustomizeConnection<Connection, rusqlite::Error> for SqliteConnectionCustomizer {
    fn on_acquire(&self, conn: &mut Connection) -> Result<(), rusqlite::Error> {
        // First, so it covers the pragmas below. r2d2 builds a connection on a
        // background thread whenever the pool grows or the reaper replaces an
        // idle one, which can land while a Document persist or a Catalog crawl
        // holds the write lock -- and `journal_mode=WAL` is itself a write.
        conn.busy_timeout(BUSY_TIMEOUT)?;
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

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    /// How long the first writer holds the lock. Long enough that a contender
    /// the scheduler delays briefly still arrives while it is held.
    const HOLD: Duration = Duration::from_millis(400);

    use rusqlite::TransactionBehavior;

    use super::temporary_pool;

    #[test]
    fn a_second_writer_waits_for_the_first_rather_than_failing_on_contact() {
        // A pin, not a fix: this passes on the timeout `rusqlite` applies by
        // itself, and it does not guard the `busy_timeout` call in `on_acquire`
        // -- with the same value on both sides, nothing can tell them apart.
        // What it does guard is the behaviour two uncoordinated write paths
        // depend on, a Document persist and a Catalog crawl: if the effective
        // timeout ever reaches zero, the loser fails on contact and nothing
        // retries. Setting BUSY_TIMEOUT to zero is what makes this fail.
        let (_dir, pool) = temporary_pool();

        let mut holder = pool.get().expect("a connection should be available");
        let held = holder
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .expect("the first writer should open a transaction");
        held.execute(
            "INSERT INTO settings (key, value) VALUES ('busy-first', '1')",
            [],
        )
        .expect("the first writer should write");

        // Spawned only once the write lock is genuinely held, so the second
        // writer cannot pass by arriving early.
        let (started, has_started) = mpsc::channel();
        let contender = pool.clone();
        let second = thread::spawn(move || {
            let conn = contender.get().expect("a connection should be available");
            started
                .send(())
                .expect("the test should still be listening");

            // Timed around the insert, because how long it blocked is the
            // finding. Inferring contention from the holder's sleep would let a
            // contender the scheduler delayed past the commit report success
            // having never met the lock at all -- green for the one reason this
            // test exists to catch.
            let reached_the_lock = Instant::now();
            (
                conn.execute(
                    "INSERT INTO settings (key, value) VALUES ('busy-second', '1')",
                    [],
                ),
                reached_the_lock.elapsed(),
            )
        });

        has_started
            .recv_timeout(Duration::from_secs(5))
            .expect("the second writer should reach its insert");
        thread::sleep(HOLD);
        held.commit().expect("the first writer should commit");

        let (wrote, waited) = second.join().expect("the second writer should not panic");
        let wrote = wrote.expect("the second writer should wait for the lock, not fail on contact");
        assert_eq!(wrote, 1);
        assert!(
            waited >= HOLD / 2,
            "the second writer returned in {waited:?}, so it never met the held lock and \
             proves nothing about waiting for one"
        );

        let conn = pool.get().expect("a connection should be available");
        let rows: u32 = conn
            .query_row(
                "SELECT COUNT(*) FROM settings WHERE key IN ('busy-first', 'busy-second')",
                [],
                |row| row.get(0),
            )
            .expect("the count should read back");
        assert_eq!(rows, 2, "both writers should have landed");
    }
}
