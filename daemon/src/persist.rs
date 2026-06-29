//! SQLite-backed persistence for the blocklist and session.
//!
//! The schema is intentionally tiny: one row per blocked domain / CIDR, and a
//! single-row `session` table. WAL mode keeps reads cheap and survives an
//! unclean shutdown. All mutations run in a transaction so a batch is atomic.

use std::path::Path;

use rusqlite::{params, Connection};

use crate::state::{Session, Snapshot};

/// Owns the SQLite connection. Not `Sync`; callers serialize access (the state
/// mutex in [`crate::state::StateManager`]).
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open (creating if absent) the database at `path` and ensure the schema
    /// exists. The parent directory must already exist.
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;",
        )?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> rusqlite::Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS domains (name TEXT PRIMARY KEY) WITHOUT ROWID;
             CREATE TABLE IF NOT EXISTS cidrs  (cidr TEXT PRIMARY KEY) WITHOUT ROWID;
             CREATE TABLE IF NOT EXISTS session (
                 id         INTEGER PRIMARY KEY CHECK (id = 1),
                 until_unix INTEGER NOT NULL,
                 committed  INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )?;
        // Schema version for future migrations.
        self.conn.execute(
            "INSERT OR IGNORE INTO meta(key, value) VALUES ('schema_version', '1')",
            [],
        )?;
        Ok(())
    }

    /// Load all persisted state into an in-memory snapshot.
    pub fn load(&self) -> rusqlite::Result<Snapshot> {
        let mut snapshot = Snapshot::default();

        {
            let mut stmt = self.conn.prepare("SELECT name FROM domains")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            for row in rows {
                snapshot.domains.insert(row?);
            }
        }
        {
            let mut stmt = self.conn.prepare("SELECT cidr FROM cidrs")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            for row in rows {
                snapshot.cidrs.insert(row?);
            }
        }
        {
            let mut stmt = self
                .conn
                .prepare("SELECT until_unix, committed FROM session WHERE id = 1")?;
            let mut rows = stmt.query([])?;
            if let Some(row) = rows.next()? {
                snapshot.session = Some(Session {
                    until_unix: row.get(0)?,
                    committed: row.get::<_, i64>(1)? != 0,
                });
            }
        }

        Ok(snapshot)
    }

    pub fn add_domains(&mut self, names: &[String]) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare("INSERT OR IGNORE INTO domains(name) VALUES (?1)")?;
            for name in names {
                stmt.execute(params![name])?;
            }
        }
        tx.commit()
    }

    pub fn remove_domains(&mut self, names: &[String]) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare("DELETE FROM domains WHERE name = ?1")?;
            for name in names {
                stmt.execute(params![name])?;
            }
        }
        tx.commit()
    }

    pub fn add_cidrs(&mut self, cidrs: &[String]) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare("INSERT OR IGNORE INTO cidrs(cidr) VALUES (?1)")?;
            for cidr in cidrs {
                stmt.execute(params![cidr])?;
            }
        }
        tx.commit()
    }

    pub fn remove_cidrs(&mut self, cidrs: &[String]) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare("DELETE FROM cidrs WHERE cidr = ?1")?;
            for cidr in cidrs {
                stmt.execute(params![cidr])?;
            }
        }
        tx.commit()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db() -> (Store, std::path::PathBuf) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir()
            .join(format!("blockerd-persist-{}-{n}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        (Store::open(&path).unwrap(), path)
    }

    #[test]
    fn round_trips_through_reopen() {
        let (mut store, path) = temp_db();
        store.add_domains(&["a.com".into(), "b.com".into()]).unwrap();
        store.add_cidrs(&["10.0.0.0/8".into()]).unwrap();
        store.remove_domains(&["a.com".into()]).unwrap();
        drop(store);

        // Reopen: surviving state should be exactly {b.com} and {10.0.0.0/8}.
        let reopened = Store::open(&path).unwrap();
        let snap = reopened.load().unwrap();
        assert_eq!(snap.domains.iter().cloned().collect::<Vec<_>>(), ["b.com"]);
        assert_eq!(snap.cidrs.iter().cloned().collect::<Vec<_>>(), ["10.0.0.0/8"]);
        assert!(snap.session.is_none());

        drop(reopened);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn insert_is_idempotent() {
        let (mut store, path) = temp_db();
        store.add_domains(&["dup.com".into()]).unwrap();
        store.add_domains(&["dup.com".into()]).unwrap();
        let snap = store.load().unwrap();
        assert_eq!(snap.domains.len(), 1);
        drop(store);
        let _ = std::fs::remove_file(&path);
    }
}
