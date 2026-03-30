use rusqlite::Connection;
use std::path::Path;

use crate::error::DataError;
use crate::schema::init_schema;

// ---------------------------------------------------------------------------
// Database handle with auto-initialization
// ---------------------------------------------------------------------------

/// A thin wrapper around rusqlite::Connection that ensures the schema exists.
pub struct Database {
    pub conn: Connection,
}

impl Database {
    /// Open (or create) a database file and initialize the schema.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DataError> {
        let conn = Connection::open(path)?;
        init_schema(&conn)?;
        Ok(Self { conn })
    }

    /// Create an in-memory database (useful for testing and backtesting).
    pub fn in_memory() -> Result<Self, DataError> {
        let conn = Connection::open_in_memory()?;
        init_schema(&conn)?;
        Ok(Self { conn })
    }

    /// Get a reference to the inner connection.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Get a mutable reference to the inner connection (needed for transactions).
    pub fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_memory_db_works() {
        let db = Database::in_memory().unwrap();
        // Verify schema is present
        let count: i64 = db
            .conn()
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(count >= 6); // accounts, firm_symbols, bars, trades, equity_snapshots, drift_log, config_changes
    }
}
