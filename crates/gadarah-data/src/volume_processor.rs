//! Volume smoothing for imported OHLCV bar data.
//!
//! Tick/volume data from sources like Dukascopy can have individual zero-volume
//! bars caused by sparse tick data or aggregation artifacts, particularly during
//! Asian-session off-hours. These zeros break volume-based signals (e.g. the
//! Breakout head's volume-confirmation filter).
//!
//! This module replaces isolated zero-volume bars with an estimate derived from
//! a rolling window of surrounding non-zero bars. The approach is conservative:
//!
//! - Only bars where the rolling window yields a positive estimate are updated.
//! - Bars in large zero-volume clusters (> `max_cluster`) are left unchanged —
//!   they likely represent genuine closed-market or data-outage periods.
//! - The estimate is the median of the non-zero volumes in the window, which
//!   is more robust to outlier bars than the mean.
//! - Uses `INSERT OR REPLACE` (upsert) to write updated volumes back.

use rusqlite::{params, Connection};
use tracing::info;

use gadarah_core::Timeframe;

use crate::error::DataError;
use crate::store::load_all_bars;

// ---------------------------------------------------------------------------
// Report
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct VolumeProcessStats {
    pub symbol: String,
    pub timeframe: Timeframe,
    pub total_bars: usize,
    pub zero_volume_bars: usize,
    pub bars_updated: usize,
    pub large_clusters_skipped: usize,
}

// ---------------------------------------------------------------------------
// Processing
// ---------------------------------------------------------------------------

/// Replace isolated zero-volume bars with a rolling-window median estimate.
///
/// # Parameters
/// - `window`: half-window size; each bar looks at `window` bars before and after.
/// - `max_cluster`: zero-volume runs longer than this are left unchanged.
pub fn process_volumes(
    conn: &Connection,
    symbol: &str,
    timeframe: Timeframe,
    window: usize,
    max_cluster: usize,
) -> Result<VolumeProcessStats, DataError> {
    let bars = load_all_bars(conn, symbol, timeframe)?;
    let total_bars = bars.len();
    let tf_str = crate::gap_filler::timeframe_str(timeframe);

    // Count zero-volume bars and their cluster memberships.
    let zero_volume_bars = bars.iter().filter(|b| b.volume == 0).count();

    if zero_volume_bars == 0 {
        return Ok(VolumeProcessStats {
            symbol: symbol.to_string(),
            timeframe,
            total_bars,
            zero_volume_bars: 0,
            bars_updated: 0,
            large_clusters_skipped: 0,
        });
    }

    // Build a cluster-size map: for each zero-volume bar index, record the
    // length of its contiguous zero-volume run.
    let mut cluster_size = vec![0usize; bars.len()];
    let mut i = 0;
    while i < bars.len() {
        if bars[i].volume == 0 {
            let start = i;
            while i < bars.len() && bars[i].volume == 0 {
                i += 1;
            }
            let len = i - start;
            for j in start..i {
                cluster_size[j] = len;
            }
        } else {
            i += 1;
        }
    }

    // For each bar that needs updating, compute the rolling-window median.
    let mut updates: Vec<(i64, u64)> = Vec::new(); // (timestamp, new_volume)
    let mut large_clusters_skipped = 0usize;

    for (idx, bar) in bars.iter().enumerate() {
        if bar.volume != 0 {
            continue;
        }

        if cluster_size[idx] > max_cluster {
            large_clusters_skipped += 1;
            continue;
        }

        // Collect non-zero volumes from the surrounding window.
        let lo = idx.saturating_sub(window);
        let hi = (idx + window + 1).min(bars.len());
        let mut surrounding: Vec<u64> = bars[lo..hi]
            .iter()
            .filter(|b| b.volume > 0)
            .map(|b| b.volume)
            .collect();

        if surrounding.is_empty() {
            // No non-zero neighbours — leave unchanged.
            continue;
        }

        surrounding.sort_unstable();
        let median = surrounding[surrounding.len() / 2];
        updates.push((bar.timestamp, median));
    }

    let bars_updated = updates.len();
    info!(
        "Volume processor: {} zero-vol bars, {} updated, {} large clusters skipped for {}/{:?}",
        zero_volume_bars, bars_updated, large_clusters_skipped, symbol, timeframe
    );

    if updates.is_empty() {
        return Ok(VolumeProcessStats {
            symbol: symbol.to_string(),
            timeframe,
            total_bars,
            zero_volume_bars,
            bars_updated: 0,
            large_clusters_skipped,
        });
    }

    // Write updates back to the database in a single transaction.
    conn.execute_batch("BEGIN")?;
    let result = (|| -> Result<(), rusqlite::Error> {
        let mut stmt = conn.prepare_cached(
            "UPDATE bars SET volume = ?1 WHERE symbol = ?2 AND timeframe = ?3 AND timestamp = ?4",
        )?;
        for (ts, vol) in &updates {
            stmt.execute(params![vol, symbol, tf_str, ts])?;
        }
        Ok(())
    })();
    if result.is_err() {
        conn.execute_batch("ROLLBACK").ok();
        return Err(DataError::Sqlite(result.unwrap_err()));
    }
    conn.execute_batch("COMMIT")?;

    Ok(VolumeProcessStats {
        symbol: symbol.to_string(),
        timeframe,
        total_bars,
        zero_volume_bars,
        bars_updated,
        large_clusters_skipped,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::store::insert_bars;
    use gadarah_core::Bar;
    use rust_decimal_macros::dec;

    fn bar_with_vol(ts: i64, vol: u64) -> Bar {
        Bar {
            timestamp: ts,
            open: dec!(1.10),
            high: dec!(1.11),
            low: dec!(1.09),
            close: dec!(1.105),
            volume: vol,
            timeframe: Timeframe::M15,
        }
    }

    #[test]
    fn isolated_zero_replaced_with_window_median() {
        let mut db = Database::in_memory().unwrap();
        let bars = vec![
            bar_with_vol(0, 100),
            bar_with_vol(900, 120),
            bar_with_vol(1800, 0), // isolated zero
            bar_with_vol(2700, 110),
            bar_with_vol(3600, 130),
        ];
        insert_bars(db.conn_mut(), "EURUSD", &bars).unwrap();

        let stats =
            process_volumes(db.conn(), "EURUSD", Timeframe::M15, 2, 3).unwrap();
        assert_eq!(stats.bars_updated, 1);
        assert_eq!(stats.large_clusters_skipped, 0);

        let updated = load_all_bars(db.conn(), "EURUSD", Timeframe::M15).unwrap();
        let b = updated.iter().find(|b| b.timestamp == 1800).unwrap();
        assert!(b.volume > 0, "zero bar should have been replaced");
    }

    #[test]
    fn large_cluster_is_skipped() {
        let mut db = Database::in_memory().unwrap();
        // 5 consecutive zero-volume bars surrounded by real bars
        let mut bars = vec![bar_with_vol(0, 100)];
        for i in 1..=5 {
            bars.push(bar_with_vol(i * 900, 0));
        }
        bars.push(bar_with_vol(6 * 900, 110));
        insert_bars(db.conn_mut(), "EURUSD", &bars).unwrap();

        // max_cluster = 4 → cluster of 5 is too large and is skipped
        let stats =
            process_volumes(db.conn(), "EURUSD", Timeframe::M15, 3, 4).unwrap();
        assert_eq!(stats.bars_updated, 0);
        assert_eq!(stats.large_clusters_skipped, 5);
    }

    #[test]
    fn no_op_when_no_zero_volume() {
        let mut db = Database::in_memory().unwrap();
        let bars: Vec<Bar> = (0..5).map(|i| bar_with_vol(i * 900, 100)).collect();
        insert_bars(db.conn_mut(), "EURUSD", &bars).unwrap();

        let stats =
            process_volumes(db.conn(), "EURUSD", Timeframe::M15, 2, 3).unwrap();
        assert_eq!(stats.zero_volume_bars, 0);
        assert_eq!(stats.bars_updated, 0);
    }
}
