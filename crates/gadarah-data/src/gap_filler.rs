//! Gap detection and filling for imported OHLCV bar data.
//!
//! After importing raw CSV data, intraday gaps occur due to data-feed outages,
//! broker maintenance windows, or missing source files. This module detects those
//! gaps and fills them with flat synthetic bars so the backtest engine never
//! encounters timestamp discontinuities that would break indicators.
//!
//! **Synthetic bar construction**: OHLC = previous bar's close, volume = 0.
//! This is the standard approach: it preserves the last known price without
//! inventing price movement, and the zero volume distinguishes synthetic bars
//! from real market activity.
//!
//! **Weekend gaps** (Friday close → Sunday/Monday open) are always skipped —
//! they are expected market closures, not data errors.
//!
//! Uses `INSERT OR IGNORE` so real bars already in the DB are never overwritten.

use chrono::{Datelike, TimeZone, Utc, Weekday};
use rusqlite::{params, Connection};
use tracing::info;

use gadarah_core::{Bar, Timeframe};

use crate::error::DataError;
use crate::store::load_all_bars;

// ---------------------------------------------------------------------------
// Gap range
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct GapRange {
    /// Timestamp of the last real bar before the gap.
    pub prev_ts: i64,
    /// Timestamp of the first real bar after the gap.
    pub next_ts: i64,
    /// Number of missing bar slots in this gap.
    pub missing_bars: u64,
}

// ---------------------------------------------------------------------------
// Report
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct GapFillReport {
    pub symbol: String,
    pub timeframe: Timeframe,
    /// Number of distinct gap ranges found.
    pub gaps_detected: usize,
    /// Total synthetic bars inserted to fill those gaps.
    pub bars_inserted: usize,
}

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------

/// Detect intraday gaps in a sorted, contiguous bar sequence.
///
/// Returns one `GapRange` per discontinuity, excluding expected weekend gaps.
pub fn detect_gaps(bars: &[Bar], timeframe: Timeframe) -> Vec<GapRange> {
    let step = timeframe.seconds();
    let mut gaps = Vec::new();

    for window in bars.windows(2) {
        let prev = &window[0];
        let curr = &window[1];
        let delta = curr.timestamp - prev.timestamp;

        if delta <= step {
            continue; // continuous or exact step — no gap
        }

        if is_expected_weekend_gap(prev.timestamp, curr.timestamp) {
            continue;
        }

        let missing = ((delta / step) as u64).saturating_sub(1);
        if missing > 0 {
            gaps.push(GapRange {
                prev_ts: prev.timestamp,
                next_ts: curr.timestamp,
                missing_bars: missing,
            });
        }
    }

    gaps
}

// ---------------------------------------------------------------------------
// Filling
// ---------------------------------------------------------------------------

/// Fill intraday gaps for a symbol/timeframe in the database.
///
/// Reads all bars, detects gaps, and inserts flat synthetic bars for each
/// missing slot. `INSERT OR IGNORE` ensures real bars are never overwritten.
pub fn fill_gaps(
    conn: &Connection,
    symbol: &str,
    timeframe: Timeframe,
) -> Result<GapFillReport, DataError> {
    let bars = load_all_bars(conn, symbol, timeframe)?;

    if bars.len() < 2 {
        return Ok(GapFillReport {
            symbol: symbol.to_string(),
            timeframe,
            gaps_detected: 0,
            bars_inserted: 0,
        });
    }

    let gaps = detect_gaps(&bars, timeframe);
    let gaps_detected = gaps.len();

    if gaps.is_empty() {
        return Ok(GapFillReport {
            symbol: symbol.to_string(),
            timeframe,
            gaps_detected: 0,
            bars_inserted: 0,
        });
    }

    // Build a timestamp → close-price lookup from the real bars so we can
    // find the flat price for each gap without re-scanning the full slice.
    let close_by_ts: std::collections::HashMap<i64, rust_decimal::Decimal> =
        bars.iter().map(|b| (b.timestamp, b.close)).collect();

    let step = timeframe.seconds();
    let tf_str = timeframe_str(timeframe);

    // Collect all synthetic (timestamp, price_text) pairs.
    let mut synthetic: Vec<(i64, String)> = Vec::new();
    for gap in &gaps {
        let flat_price = match close_by_ts.get(&gap.prev_ts) {
            Some(p) => p.to_string(),
            None => continue, // shouldn't happen, but skip rather than panic
        };
        let mut ts = gap.prev_ts + step;
        while ts < gap.next_ts {
            synthetic.push((ts, flat_price.clone()));
            ts += step;
        }
    }

    let bars_to_insert = synthetic.len();
    info!(
        "Gap filler: {} gap(s), inserting {} synthetic bars for {}/{:?}",
        gaps_detected, bars_to_insert, symbol, timeframe
    );

    // Batch insert inside a manual transaction for speed.
    // The `?4` reuse (open/high/low/close all = flat price) is valid SQLite.
    conn.execute_batch("BEGIN")?;
    let result = (|| -> Result<(), rusqlite::Error> {
        let mut stmt = conn.prepare_cached(
            "INSERT OR IGNORE INTO bars
             (symbol, timeframe, timestamp, open, high, low, close, volume)
             VALUES (?1, ?2, ?3, ?4, ?4, ?4, ?4, 0)",
        )?;
        for (ts, price) in &synthetic {
            stmt.execute(params![symbol, tf_str, ts, price])?;
        }
        Ok(())
    })();
    if result.is_err() {
        conn.execute_batch("ROLLBACK").ok();
        return Err(DataError::Sqlite(result.unwrap_err()));
    }
    conn.execute_batch("COMMIT")?;

    Ok(GapFillReport {
        symbol: symbol.to_string(),
        timeframe,
        gaps_detected,
        bars_inserted: bars_to_insert,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub(crate) fn timeframe_str(tf: Timeframe) -> &'static str {
    match tf {
        Timeframe::M1 => "M1",
        Timeframe::M5 => "M5",
        Timeframe::M15 => "M15",
        Timeframe::H1 => "H1",
        Timeframe::H4 => "H4",
        Timeframe::D1 => "D1",
    }
}

/// Returns true if the gap between two timestamps represents an expected
/// market weekend closure (Friday → Sunday/Monday).
fn is_expected_weekend_gap(prev_ts: i64, curr_ts: i64) -> bool {
    let Some(prev_dt) = Utc.timestamp_opt(prev_ts, 0).single() else {
        return false;
    };
    let Some(curr_dt) = Utc.timestamp_opt(curr_ts, 0).single() else {
        return false;
    };
    matches!(prev_dt.weekday(), Weekday::Fri)
        && matches!(curr_dt.weekday(), Weekday::Sun | Weekday::Mon)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::store::insert_bars;
    use rust_decimal_macros::dec;

    fn bar(ts: i64, close: rust_decimal::Decimal) -> Bar {
        Bar {
            timestamp: ts,
            open: close,
            high: close,
            low: close,
            close,
            volume: 100,
            timeframe: Timeframe::M15,
        }
    }

    #[test]
    fn detects_single_intraday_gap() {
        let bars = vec![
            bar(0, dec!(1.1000)),
            bar(900, dec!(1.1010)),
            // timestamps 1800 and 2700 are missing
            bar(3600, dec!(1.1020)),
        ];
        let gaps = detect_gaps(&bars, Timeframe::M15);
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].prev_ts, 900);
        assert_eq!(gaps[0].next_ts, 3600);
        assert_eq!(gaps[0].missing_bars, 2);
    }

    #[test]
    fn continuous_sequence_has_no_gaps() {
        let bars: Vec<Bar> = (0..10).map(|i| bar(i * 900, dec!(1.1))).collect();
        let gaps = detect_gaps(&bars, Timeframe::M15);
        assert!(gaps.is_empty());
    }

    #[test]
    fn weekend_gap_is_skipped() {
        // Friday 2026-04-03 21:00 UTC → Monday 2026-04-06 00:00 UTC
        let friday_ts = Utc
            .with_ymd_and_hms(2026, 4, 3, 21, 0, 0)
            .single()
            .unwrap()
            .timestamp();
        let monday_ts = Utc
            .with_ymd_and_hms(2026, 4, 6, 0, 0, 0)
            .single()
            .unwrap()
            .timestamp();
        let bars = vec![bar(friday_ts, dec!(1.1)), bar(monday_ts, dec!(1.1))];
        let gaps = detect_gaps(&bars, Timeframe::M15);
        assert!(gaps.is_empty());
    }

    #[test]
    fn synthetic_bars_use_previous_close_as_flat_ohlc() {
        let mut db = Database::in_memory().unwrap();
        let real_bars = vec![
            Bar {
                timestamp: 0,
                open: dec!(1.10),
                high: dec!(1.11),
                low: dec!(1.09),
                close: dec!(1.105),
                volume: 100,
                timeframe: Timeframe::M15,
            },
            Bar {
                timestamp: 900,
                open: dec!(1.105),
                high: dec!(1.115),
                low: dec!(1.095),
                close: dec!(1.110),
                volume: 90,
                timeframe: Timeframe::M15,
            },
            // gap: ts 1800 and 2700 missing
            Bar {
                timestamp: 3600,
                open: dec!(1.110),
                high: dec!(1.120),
                low: dec!(1.100),
                close: dec!(1.115),
                volume: 80,
                timeframe: Timeframe::M15,
            },
        ];
        insert_bars(db.conn_mut(), "EURUSD", &real_bars).unwrap();

        let report = fill_gaps(db.conn(), "EURUSD", Timeframe::M15).unwrap();
        assert_eq!(report.gaps_detected, 1);
        assert_eq!(report.bars_inserted, 2);

        let all = load_all_bars(db.conn(), "EURUSD", Timeframe::M15).unwrap();
        assert_eq!(all.len(), 5);

        // Synthetic bar at ts=1800 should be flat at close of ts=900 (1.110)
        let synth = all.iter().find(|b| b.timestamp == 1800).unwrap();
        assert_eq!(synth.open, dec!(1.110));
        assert_eq!(synth.high, dec!(1.110));
        assert_eq!(synth.low, dec!(1.110));
        assert_eq!(synth.close, dec!(1.110));
        assert_eq!(synth.volume, 0);
    }

    #[test]
    fn fill_does_not_overwrite_real_bars() {
        let mut db = Database::in_memory().unwrap();
        let bars = vec![
            bar(0, dec!(1.1)),
            // gap at 900
            bar(1800, dec!(1.2)),
        ];
        insert_bars(db.conn_mut(), "EURUSD", &bars).unwrap();

        // Insert a real bar at ts=900 before fill runs
        let real_at_900 = vec![Bar {
            timestamp: 900,
            open: dec!(1.15),
            high: dec!(1.17),
            low: dec!(1.13),
            close: dec!(1.16),
            volume: 200,
            timeframe: Timeframe::M15,
        }];
        insert_bars(db.conn_mut(), "EURUSD", &real_at_900).unwrap();

        let report = fill_gaps(db.conn(), "EURUSD", Timeframe::M15).unwrap();
        // Gap filler sees no gaps now (all slots filled)
        assert_eq!(report.bars_inserted, 0);

        // The real bar at 900 is intact
        let all = load_all_bars(db.conn(), "EURUSD", Timeframe::M15).unwrap();
        let b900 = all.iter().find(|b| b.timestamp == 900).unwrap();
        assert_eq!(b900.close, dec!(1.16));
        assert_eq!(b900.volume, 200);
    }
}
