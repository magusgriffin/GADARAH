use rusqlite::{params, Connection, Transaction};
use rust_decimal::Decimal;
use std::str::FromStr;

use gadarah_core::{Bar, Timeframe};

use crate::error::DataError;

// ---------------------------------------------------------------------------
// Helpers: Decimal ↔ TEXT round-trip (lossless, no REAL precision loss)
// ---------------------------------------------------------------------------

fn dec_to_str(d: &Decimal) -> String {
    d.to_string()
}

fn str_to_dec(field: &'static str, s: &str) -> Result<Decimal, DataError> {
    Decimal::from_str(s).map_err(|_| DataError::InvalidDecimal {
        field,
        value: s.to_string(),
    })
}

fn tf_to_str(tf: Timeframe) -> &'static str {
    match tf {
        Timeframe::M1 => "M1",
        Timeframe::M5 => "M5",
        Timeframe::M15 => "M15",
        Timeframe::H1 => "H1",
        Timeframe::H4 => "H4",
        Timeframe::D1 => "D1",
    }
}

pub fn str_to_tf(s: &str) -> Result<Timeframe, DataError> {
    match s {
        "M1" => Ok(Timeframe::M1),
        "M5" => Ok(Timeframe::M5),
        "M15" => Ok(Timeframe::M15),
        "H1" => Ok(Timeframe::H1),
        "H4" => Ok(Timeframe::H4),
        "D1" => Ok(Timeframe::D1),
        other => Err(DataError::InvalidTimeframe(other.to_string())),
    }
}

// ---------------------------------------------------------------------------
// Bar CRUD
// ---------------------------------------------------------------------------

/// Insert a single bar (upsert on conflict).
pub fn insert_bar(conn: &Connection, symbol: &str, bar: &Bar) -> Result<(), DataError> {
    conn.execute(
        "INSERT INTO bars (symbol, timeframe, timestamp, open, high, low, close, volume)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(symbol, timeframe, timestamp) DO UPDATE SET
             open = excluded.open,
             high = excluded.high,
             low  = excluded.low,
             close = excluded.close,
             volume = excluded.volume",
        params![
            symbol,
            tf_to_str(bar.timeframe),
            bar.timestamp,
            dec_to_str(&bar.open),
            dec_to_str(&bar.high),
            dec_to_str(&bar.low),
            dec_to_str(&bar.close),
            bar.volume,
        ],
    )?;
    Ok(())
}

/// Batch insert bars inside a transaction for performance.
pub fn insert_bars(conn: &mut Connection, symbol: &str, bars: &[Bar]) -> Result<usize, DataError> {
    let tx = conn.transaction()?;
    let count = insert_bars_tx(&tx, symbol, bars)?;
    tx.commit()?;
    Ok(count)
}

/// Batch insert within an existing transaction.
pub fn insert_bars_tx(tx: &Transaction, symbol: &str, bars: &[Bar]) -> Result<usize, DataError> {
    let mut stmt = tx.prepare_cached(
        "INSERT INTO bars (symbol, timeframe, timestamp, open, high, low, close, volume)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(symbol, timeframe, timestamp) DO UPDATE SET
             open = excluded.open,
             high = excluded.high,
             low  = excluded.low,
             close = excluded.close,
             volume = excluded.volume",
    )?;
    let mut count = 0;
    for bar in bars {
        stmt.execute(params![
            symbol,
            tf_to_str(bar.timeframe),
            bar.timestamp,
            dec_to_str(&bar.open),
            dec_to_str(&bar.high),
            dec_to_str(&bar.low),
            dec_to_str(&bar.close),
            bar.volume,
        ])?;
        count += 1;
    }
    Ok(count)
}

/// Load bars for a symbol/timeframe in a time range [from_ts, to_ts], ordered by timestamp.
pub fn load_bars(
    conn: &Connection,
    symbol: &str,
    tf: Timeframe,
    from_ts: i64,
    to_ts: i64,
) -> Result<Vec<Bar>, DataError> {
    let mut stmt = conn.prepare_cached(
        "SELECT timestamp, open, high, low, close, volume
         FROM bars
         WHERE symbol = ?1 AND timeframe = ?2
          AND timestamp >= ?3 AND timestamp <= ?4
         ORDER BY timestamp ASC",
    )?;
    let rows = stmt.query_map(params![symbol, tf_to_str(tf), from_ts, to_ts], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, u64>(5)?,
        ))
    })?;
    let rows: Result<Vec<_>, _> = rows.collect();

    rows?
        .into_iter()
        .map(|(timestamp, open, high, low, close, volume)| {
            Ok(Bar {
                timestamp,
                open: str_to_dec("bars.open", &open)?,
                high: str_to_dec("bars.high", &high)?,
                low: str_to_dec("bars.low", &low)?,
                close: str_to_dec("bars.close", &close)?,
                volume,
                timeframe: tf,
            })
        })
        .collect()
}

/// Load ALL bars for a symbol/timeframe, ordered by timestamp.
pub fn load_all_bars(
    conn: &Connection,
    symbol: &str,
    tf: Timeframe,
) -> Result<Vec<Bar>, DataError> {
    load_bars(conn, symbol, tf, 0, i64::MAX)
}

/// Count bars for a symbol/timeframe.
pub fn count_bars(conn: &Connection, symbol: &str, tf: Timeframe) -> Result<i64, DataError> {
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM bars WHERE symbol = ?1 AND timeframe = ?2",
        params![symbol, tf_to_str(tf)],
        |r| r.get(0),
    )?;
    Ok(count)
}

/// Get the timestamp range (min, max) for a symbol/timeframe.
pub fn bar_time_range(
    conn: &Connection,
    symbol: &str,
    tf: Timeframe,
) -> Result<Option<(i64, i64)>, DataError> {
    let result: Option<(i64, i64)> = conn.query_row(
        "SELECT min(timestamp), max(timestamp) FROM bars WHERE symbol = ?1 AND timeframe = ?2",
        params![symbol, tf_to_str(tf)],
        |r| {
            let min: Option<i64> = r.get(0)?;
            let max: Option<i64> = r.get(1)?;
            Ok(min.zip(max))
        },
    )?;
    Ok(result)
}

/// Delete bars in a time range.
pub fn delete_bars(
    conn: &Connection,
    symbol: &str,
    tf: Timeframe,
    from_ts: i64,
    to_ts: i64,
) -> Result<usize, DataError> {
    let deleted = conn.execute(
        "DELETE FROM bars WHERE symbol = ?1 AND timeframe = ?2
         AND timestamp >= ?3 AND timestamp <= ?4",
        params![symbol, tf_to_str(tf), from_ts, to_ts],
    )?;
    Ok(deleted)
}

/// List distinct symbols in the database.
pub fn list_symbols(conn: &Connection) -> Result<Vec<String>, DataError> {
    let mut stmt = conn.prepare("SELECT DISTINCT symbol FROM bars ORDER BY symbol")?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    let syms: Result<Vec<String>, _> = rows.collect();
    Ok(syms?)
}

/// List distinct timeframes available for a symbol.
pub fn list_timeframes(conn: &Connection, symbol: &str) -> Result<Vec<Timeframe>, DataError> {
    let mut stmt =
        conn.prepare("SELECT DISTINCT timeframe FROM bars WHERE symbol = ?1 ORDER BY timeframe")?;
    let rows = stmt.query_map(params![symbol], |r| r.get::<_, String>(0))?;
    let mut tfs = Vec::new();
    for row in rows {
        if let Ok(tf) = str_to_tf(&row?) {
            tfs.push(tf);
        }
    }
    Ok(tfs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::init_schema;
    use rust_decimal_macros::dec;

    fn test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn
    }

    fn sample_bar(ts: i64) -> Bar {
        Bar {
            open: dec!(1.10000),
            high: dec!(1.10500),
            low: dec!(1.09800),
            close: dec!(1.10200),
            volume: 1500,
            timestamp: ts,
            timeframe: Timeframe::M5,
        }
    }

    #[test]
    fn insert_and_load_round_trip() {
        let conn = test_db();
        let bar = sample_bar(1700000000);
        insert_bar(&conn, "EURUSD", &bar).unwrap();

        let loaded = load_bars(&conn, "EURUSD", Timeframe::M5, 0, i64::MAX).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].open, bar.open);
        assert_eq!(loaded[0].high, bar.high);
        assert_eq!(loaded[0].close, bar.close);
        assert_eq!(loaded[0].volume, bar.volume);
    }

    #[test]
    fn upsert_overwrites() {
        let conn = test_db();
        let mut bar = sample_bar(1700000000);
        insert_bar(&conn, "EURUSD", &bar).unwrap();

        bar.close = dec!(1.11000);
        insert_bar(&conn, "EURUSD", &bar).unwrap();

        let loaded = load_all_bars(&conn, "EURUSD", Timeframe::M5).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].close, dec!(1.11000));
    }

    #[test]
    fn batch_insert() {
        let mut conn = test_db();
        let bars: Vec<Bar> = (0..100).map(|i| sample_bar(1700000000 + i * 300)).collect();
        let count = insert_bars(&mut conn, "GBPUSD", &bars).unwrap();
        assert_eq!(count, 100);
        assert_eq!(count_bars(&conn, "GBPUSD", Timeframe::M5).unwrap(), 100);
    }

    #[test]
    fn time_range_query() {
        let conn = test_db();
        for i in 0..10 {
            insert_bar(&conn, "EURUSD", &sample_bar(1700000000 + i * 300)).unwrap();
        }
        let loaded = load_bars(&conn, "EURUSD", Timeframe::M5, 1700000600, 1700001200).unwrap();
        assert_eq!(loaded.len(), 3); // ts 600, 900, 1200
    }

    #[test]
    fn list_symbols_and_timeframes() {
        let conn = test_db();
        insert_bar(&conn, "EURUSD", &sample_bar(1700000000)).unwrap();

        let mut bar_h1 = sample_bar(1700000000);
        bar_h1.timeframe = Timeframe::H1;
        insert_bar(&conn, "EURUSD", &bar_h1).unwrap();
        insert_bar(&conn, "GBPUSD", &sample_bar(1700000000)).unwrap();

        let syms = list_symbols(&conn).unwrap();
        assert_eq!(syms, vec!["EURUSD", "GBPUSD"]);

        let tfs = list_timeframes(&conn, "EURUSD").unwrap();
        assert_eq!(tfs.len(), 2);
    }

    #[test]
    fn delete_bars_range() {
        let conn = test_db();
        for i in 0..10 {
            insert_bar(&conn, "EURUSD", &sample_bar(1700000000 + i * 300)).unwrap();
        }
        let deleted = delete_bars(&conn, "EURUSD", Timeframe::M5, 1700000000, 1700000900).unwrap();
        assert_eq!(deleted, 4);
        assert_eq!(count_bars(&conn, "EURUSD", Timeframe::M5).unwrap(), 6);
    }

    #[test]
    fn load_bars_rejects_invalid_decimal_text() {
        let conn = test_db();
        conn.execute(
            "INSERT INTO bars (symbol, timeframe, timestamp, open, high, low, close, volume)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                "EURUSD",
                "M5",
                1700000000i64,
                "bad",
                "1.1010",
                "1.0990",
                "1.1005",
                42u64
            ],
        )
        .unwrap();

        let err = load_all_bars(&conn, "EURUSD", Timeframe::M5).unwrap_err();
        assert!(matches!(
            err,
            DataError::InvalidDecimal { field, value }
                if field == "bars.open" && value == "bad"
        ));
    }
}
