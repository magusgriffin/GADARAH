use rusqlite::Connection;
use rust_decimal::Decimal;
use std::io::BufRead;
use std::str::FromStr;

use gadarah_core::{Bar, Timeframe};

use crate::error::DataError;
use crate::store;

// ---------------------------------------------------------------------------
// CSV Import: generic multi-format bar importer
// ---------------------------------------------------------------------------

/// Supported CSV formats for historical data import.
#[derive(Debug, Clone, Copy)]
pub enum CsvFormat {
    /// MetaTrader: Date,Time,Open,High,Low,Close,Volume
    /// Example: 2024.01.02,00:00,1.10456,1.10512,1.10401,1.10489,1234
    MetaTrader,

    /// cTrader export: Timestamp(ms),Open,High,Low,Close,Volume
    CTrader,

    /// Generic: unix_timestamp,open,high,low,close,volume
    Unix,
}

/// Import bars from a CSV reader into the database.
///
/// Returns the number of bars imported.
pub fn import_csv<R: BufRead>(
    conn: &mut Connection,
    reader: R,
    symbol: &str,
    tf: Timeframe,
    format: CsvFormat,
) -> Result<usize, DataError> {
    let mut bars = Vec::new();

    for (line_idx, line_result) in reader.lines().enumerate() {
        let line = line_result.map_err(|e| DataError::CsvParse {
            line: line_idx + 1,
            msg: e.to_string(),
        })?;
        let trimmed = line.trim();

        // Skip empty lines and headers
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with("Date")
            || trimmed.starts_with("date")
            || trimmed.starts_with("Timestamp")
            || trimmed.starts_with("timestamp")
        {
            continue;
        }

        let bar = parse_line(trimmed, tf, format, line_idx + 1)?;
        bars.push(bar);
    }

    store::insert_bars(conn, symbol, &bars)
}

fn parse_line(
    line: &str,
    tf: Timeframe,
    format: CsvFormat,
    line_num: usize,
) -> Result<Bar, DataError> {
    let err = |msg: &str| DataError::CsvParse {
        line: line_num,
        msg: msg.to_string(),
    };

    // Split by comma or tab
    let fields: Vec<&str> = line.split([',', '\t']).collect();

    match format {
        CsvFormat::MetaTrader => {
            // Date,Time,Open,High,Low,Close,Volume  (7 fields)
            // OR Date,Time,Open,High,Low,Close,TickVolume,Volume (8 fields)
            if fields.len() < 7 {
                return Err(err("expected at least 7 fields (Date,Time,O,H,L,C,Vol)"));
            }
            let timestamp =
                parse_mt_datetime(fields[0], fields[1]).ok_or_else(|| err("invalid date/time"))?;
            Ok(Bar {
                open: parse_dec(fields[2]).ok_or_else(|| err("invalid open"))?,
                high: parse_dec(fields[3]).ok_or_else(|| err("invalid high"))?,
                low: parse_dec(fields[4]).ok_or_else(|| err("invalid low"))?,
                close: parse_dec(fields[5]).ok_or_else(|| err("invalid close"))?,
                volume: fields[6].trim().parse().unwrap_or(0),
                timestamp,
                timeframe: tf,
            })
        }
        CsvFormat::CTrader => {
            // Timestamp(ms),Open,High,Low,Close,Volume (6 fields)
            if fields.len() < 6 {
                return Err(err("expected 6 fields (Timestamp_ms,O,H,L,C,Vol)"));
            }
            let ts_ms: i64 = fields[0]
                .trim()
                .parse()
                .map_err(|_| err("invalid timestamp"))?;
            Ok(Bar {
                open: parse_dec(fields[1]).ok_or_else(|| err("invalid open"))?,
                high: parse_dec(fields[2]).ok_or_else(|| err("invalid high"))?,
                low: parse_dec(fields[3]).ok_or_else(|| err("invalid low"))?,
                close: parse_dec(fields[4]).ok_or_else(|| err("invalid close"))?,
                volume: fields[5].trim().parse().unwrap_or(0),
                timestamp: ts_ms / 1000,
                timeframe: tf,
            })
        }
        CsvFormat::Unix => {
            // unix_timestamp,open,high,low,close,volume (6 fields)
            if fields.len() < 6 {
                return Err(err("expected 6 fields (unix_ts,O,H,L,C,Vol)"));
            }
            let ts: i64 = fields[0]
                .trim()
                .parse()
                .map_err(|_| err("invalid timestamp"))?;
            Ok(Bar {
                open: parse_dec(fields[1]).ok_or_else(|| err("invalid open"))?,
                high: parse_dec(fields[2]).ok_or_else(|| err("invalid high"))?,
                low: parse_dec(fields[3]).ok_or_else(|| err("invalid low"))?,
                close: parse_dec(fields[4]).ok_or_else(|| err("invalid close"))?,
                volume: fields[5].trim().parse().unwrap_or(0),
                timestamp: ts,
                timeframe: tf,
            })
        }
    }
}

fn parse_dec(s: &str) -> Option<Decimal> {
    Decimal::from_str(s.trim()).ok()
}

/// Parse MetaTrader date/time: "2024.01.02" + "00:00" → unix timestamp.
fn parse_mt_datetime(date_str: &str, time_str: &str) -> Option<i64> {
    // Date: YYYY.MM.DD or YYYY/MM/DD or YYYY-MM-DD
    let date_parts: Vec<&str> = date_str.split(['.', '/', '-']).collect();
    if date_parts.len() != 3 {
        return None;
    }
    let year: i32 = date_parts[0].trim().parse().ok()?;
    let month: u32 = date_parts[1].trim().parse().ok()?;
    let day: u32 = date_parts[2].trim().parse().ok()?;

    // Time: HH:MM or HH:MM:SS
    let time_parts: Vec<&str> = time_str.split(':').collect();
    if time_parts.len() < 2 {
        return None;
    }
    let hour: u32 = time_parts[0].trim().parse().ok()?;
    let minute: u32 = time_parts[1].trim().parse().ok()?;
    let second: u32 = if time_parts.len() > 2 {
        time_parts[2].trim().parse().unwrap_or(0)
    } else {
        0
    };

    // Convert to unix timestamp (UTC) using chrono
    use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
    let date = NaiveDate::from_ymd_opt(year, month, day)?;
    let time = NaiveTime::from_hms_opt(hour, minute, second)?;
    let dt = NaiveDateTime::new(date, time);
    Some(dt.and_utc().timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mt_datetime_works() {
        let ts = parse_mt_datetime("2024.01.02", "13:30").unwrap();
        // 2024-01-02 13:30:00 UTC
        assert_eq!(ts, 1704202200);
    }

    #[test]
    fn import_unix_csv() {
        let csv_data = b"timestamp,open,high,low,close,volume\n\
            1700000000,1.10000,1.10500,1.09800,1.10200,1500\n\
            1700000300,1.10200,1.10600,1.10100,1.10400,1200\n";

        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::schema::init_schema(&conn).unwrap();

        let count = import_csv(
            &mut conn,
            &csv_data[..],
            "EURUSD",
            Timeframe::M5,
            CsvFormat::Unix,
        )
        .unwrap();
        assert_eq!(count, 2);

        let bars = store::load_all_bars(&conn, "EURUSD", Timeframe::M5).unwrap();
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0].timestamp, 1700000000);
    }

    #[test]
    fn import_mt_csv() {
        let csv_data = b"Date,Time,Open,High,Low,Close,Volume\n\
            2024.01.02,13:30,1.10000,1.10500,1.09800,1.10200,1500\n\
            2024.01.02,13:35,1.10200,1.10600,1.10100,1.10400,1200\n";

        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::schema::init_schema(&conn).unwrap();

        let count = import_csv(
            &mut conn,
            &csv_data[..],
            "EURUSD",
            Timeframe::M5,
            CsvFormat::MetaTrader,
        )
        .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn skips_header_and_empty_lines() {
        let csv_data = b"# Comment line\n\
            timestamp,open,high,low,close,volume\n\
            \n\
            1700000000,1.10000,1.10500,1.09800,1.10200,1500\n";

        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::schema::init_schema(&conn).unwrap();

        let count = import_csv(
            &mut conn,
            &csv_data[..],
            "EURUSD",
            Timeframe::M5,
            CsvFormat::Unix,
        )
        .unwrap();
        assert_eq!(count, 1);
    }
}
