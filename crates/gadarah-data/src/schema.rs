use rusqlite::Connection;

use crate::error::DataError;

/// All CREATE TABLE statements for the GADARAH database.
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS accounts (
    id          INTEGER PRIMARY KEY,
    firm_name   TEXT    NOT NULL,
    broker_account_id INTEGER NOT NULL UNIQUE,
    phase       TEXT    NOT NULL,
    balance     TEXT    NOT NULL,
    equity      TEXT    NOT NULL,
    high_water_mark TEXT NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS firm_symbols (
    firm_name           TEXT NOT NULL,
    our_symbol          TEXT NOT NULL,
    broker_symbol_id    INTEGER NOT NULL,
    pip_size            TEXT NOT NULL,
    lot_size            TEXT NOT NULL,
    pip_value_per_lot   TEXT NOT NULL,
    swap_long           TEXT,
    swap_short          TEXT,
    typical_spread_pips TEXT,
    commission_per_lot  TEXT,
    PRIMARY KEY (firm_name, our_symbol)
);

CREATE TABLE IF NOT EXISTS bars (
    symbol      TEXT    NOT NULL,
    timeframe   TEXT    NOT NULL,
    timestamp   INTEGER NOT NULL,
    open        TEXT    NOT NULL,
    high        TEXT    NOT NULL,
    low         TEXT    NOT NULL,
    close       TEXT    NOT NULL,
    volume      INTEGER NOT NULL,
    PRIMARY KEY (symbol, timeframe, timestamp)
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS idx_bars_lookup
    ON bars(symbol, timeframe, timestamp);

CREATE TABLE IF NOT EXISTS trades (
    id              INTEGER PRIMARY KEY,
    account_id      INTEGER NOT NULL,
    symbol          TEXT    NOT NULL,
    direction       TEXT    NOT NULL,
    head            TEXT    NOT NULL,
    regime          TEXT    NOT NULL,
    session         TEXT    NOT NULL,
    entry_price     TEXT    NOT NULL,
    sl_price        TEXT    NOT NULL,
    tp_price        TEXT    NOT NULL,
    lots            TEXT    NOT NULL,
    risk_pct        TEXT    NOT NULL,
    pyramid_level   INTEGER NOT NULL DEFAULT 0,
    opened_at       INTEGER NOT NULL,
    closed_at       INTEGER,
    close_price     TEXT,
    pnl_usd         TEXT,
    r_multiple      TEXT,
    close_reason    TEXT,
    slippage_pips   TEXT
);

CREATE INDEX IF NOT EXISTS idx_trades_account
    ON trades(account_id, opened_at);

CREATE TABLE IF NOT EXISTS equity_snapshots (
    id              INTEGER PRIMARY KEY,
    account_id      INTEGER NOT NULL,
    balance         TEXT    NOT NULL,
    equity          TEXT    NOT NULL,
    daily_pnl_usd   TEXT    NOT NULL,
    daily_dd_pct    TEXT    NOT NULL,
    total_dd_pct    TEXT    NOT NULL,
    day_state       TEXT    NOT NULL,
    snapshotted_at  INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_equity_account
    ON equity_snapshots(account_id, snapshotted_at);

CREATE TABLE IF NOT EXISTS drift_log (
    id                  INTEGER PRIMARY KEY,
    signal              TEXT    NOT NULL,
    live_win_rate       TEXT,
    expected_win_rate   TEXT,
    live_avg_r          TEXT,
    live_avg_slippage   TEXT,
    action_taken        TEXT    NOT NULL,
    logged_at           INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS config_changes (
    id          INTEGER PRIMARY KEY,
    account_id  INTEGER,
    key         TEXT    NOT NULL,
    old_value   TEXT,
    new_value   TEXT    NOT NULL,
    changed_at  INTEGER NOT NULL
);
"#;

/// Initialize the database schema. Safe to call multiple times (IF NOT EXISTS).
pub fn init_schema(conn: &Connection) -> Result<(), DataError> {
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    conn.execute_batch("PRAGMA synchronous=NORMAL;")?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;
    conn.execute_batch(SCHEMA)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_creates_without_error() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        // Verify a table exists
        let count: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='bars'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn schema_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        init_schema(&conn).unwrap(); // second call should not fail
    }
}
