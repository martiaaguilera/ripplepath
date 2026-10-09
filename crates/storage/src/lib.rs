//! SQLite persistence. Local-first: one file, no server, transactional.
//!
//! Schema changes only through numbered migrations in `migrations/`; the applied count is kept in
//! `PRAGMA user_version`. A database created by a newer Ripplepath is refused rather than guessed at.

mod evidence;
mod graph;

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

pub use evidence::{HistoryEntry, StoredCoverage, StoredResult};
pub use graph::{GraphDelta, IndexedFile, IndexedGraph};

const MIGRATIONS: &[&str] =
    &[include_str!("migrations/0001_index.sql"), include_str!("migrations/0002_test_evidence.sql")];

/// Number of migrations this build knows; exposed so `ripplepath --version`-style output and
/// reports can say which schema they wrote.
pub const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("database schema version {found} is newer than this build supports ({supported}); upgrade Ripplepath")]
    TooNew { found: u32, supported: u32 },
    #[error("corrupt value in column {column}: {value}")]
    Corrupt { column: &'static str, value: String },
}

pub struct Store {
    conn: Connection,
}

/// A cached extraction result.
pub enum CachedFacts {
    Ok(Vec<u8>),
    Error(String),
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        if let Some(parent) = path.parent() {
            // Best effort: a missing directory surfaces as the open error below.
            let _ = std::fs::create_dir_all(parent);
        }
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self, StorageError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self, StorageError> {
        // WAL lets the server read while an index run writes. Foreign keys only link evidence rows
        // to their run/report, so deleting a run removes its results.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        // Fact payloads average a few KiB, so most live on overflow pages, which SQLite reads with
        // one read call each, bypassing its page cache. A warm 20k-file index made 20k of them and
        // spent seconds in the kernel (docs/BENCHMARKS.md); reading through a bounded memory map
        // made those lookups several times faster. Pages are file-backed, so the OS can reclaim
        // them; the cap bounds the address space, not correctness.
        conn.pragma_update(None, "mmap_size", 268_435_456)?;
        let current: u32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if current > SCHEMA_VERSION {
            return Err(StorageError::TooNew { found: current, supported: SCHEMA_VERSION });
        }
        for (index, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
            let tx = conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", index as u32 + 1)?;
            tx.commit()?;
        }
        Ok(Self { conn })
    }

    pub fn schema_version(&self) -> Result<u32, StorageError> {
        Ok(self.conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }

    pub fn cached_facts(&self, blob: &str, path: &str, extractor: &str) -> Result<Option<CachedFacts>, StorageError> {
        let row: Option<(String, Vec<u8>)> = self
            .conn
            .prepare_cached("SELECT outcome, payload FROM file_facts WHERE blob = ?1 AND path = ?2 AND extractor = ?3")?
            .query_row(params![blob, path, extractor], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        Ok(row.map(|(outcome, payload)| match outcome.as_str() {
            "ok" => CachedFacts::Ok(payload),
            _ => CachedFacts::Error(String::from_utf8_lossy(&payload).into_owned()),
        }))
    }

    pub fn put_facts(&mut self, entries: &[(String, String, String, CachedFacts)]) -> Result<(), StorageError> {
        let tx = self.conn.transaction()?;
        {
            let mut insert = tx.prepare_cached(
                "INSERT OR REPLACE INTO file_facts (blob, path, extractor, outcome, payload) VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for (blob, path, extractor, facts) in entries {
                let (outcome, payload): (&str, &[u8]) = match facts {
                    CachedFacts::Ok(bytes) => ("ok", bytes),
                    CachedFacts::Error(message) => ("error", message.as_bytes()),
                };
                insert.execute(params![blob, path, extractor, outcome, payload])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }

    pub(crate) fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }
}
