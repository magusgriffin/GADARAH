//! High-level post-processing pipeline for imported OHLCV data.
//!
//! After raw CSV files are imported into the database via `import_dataset_dir`,
//! this pipeline cleans and validates the data:
//!
//! 1. **Gap fill** — detect and fill intraday gaps with flat synthetic bars.
//! 2. **Volume smooth** — replace isolated zero-volume bars with window-median
//!    estimates so volume-based signals function correctly.
//! 3. **Audit** — run the data-quality audit on every series and collect results.
//!
//! Run this once after a new data import and before backtesting.

use gadarah_core::Timeframe;
use rusqlite::Connection;
use tracing::info;

use crate::audit::{audit_bars, DataAuditResult};
use crate::error::DataError;
use crate::gap_filler::{fill_gaps, GapFillReport};
use crate::store::{list_timeframes, load_all_bars};
use crate::volume_processor::{process_volumes, VolumeProcessStats};

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Symbols to process. Empty = process all symbols found in the DB.
    pub symbols: Vec<String>,
    /// Timeframes to process per symbol. Empty = all timeframes present in DB.
    pub timeframes: Vec<Timeframe>,
    /// Half-window size for the rolling-volume smoother.
    pub volume_window: usize,
    /// Zero-volume cluster length above which bars are left unchanged.
    pub volume_max_cluster: usize,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            symbols: Vec::new(),
            timeframes: Vec::new(),
            volume_window: 10,
            volume_max_cluster: 6,
        }
    }
}

// ---------------------------------------------------------------------------
// Report
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SeriesPipelineResult {
    pub symbol: String,
    pub timeframe: Timeframe,
    pub gap_fill: GapFillReport,
    pub volume: VolumeProcessStats,
    pub audit: DataAuditResult,
}

#[derive(Debug, Clone)]
pub struct PipelineReport {
    pub series: Vec<SeriesPipelineResult>,
    /// Total synthetic bars inserted across all series.
    pub total_gaps_filled: usize,
    /// Total zero-volume bars updated across all series.
    pub total_volume_bars_fixed: usize,
    /// Number of series that still fail the data audit after processing.
    pub series_still_failing: usize,
}

impl PipelineReport {
    pub fn passed(&self) -> bool {
        self.series_still_failing == 0
    }
}

// ---------------------------------------------------------------------------
// Pipeline
// ---------------------------------------------------------------------------

/// Run the full post-processing pipeline on an already-imported database.
///
/// Loads all symbols/timeframes present in the DB (filtered by `config`),
/// then runs gap-fill → volume-smooth → audit for each series.
pub fn run_pipeline(conn: &Connection, config: &PipelineConfig) -> Result<PipelineReport, DataError> {
    // Determine which symbols to process.
    let all_symbols = list_symbols_in_db(conn)?;
    let symbols: Vec<String> = if config.symbols.is_empty() {
        all_symbols
    } else {
        config
            .symbols
            .iter()
            .filter(|s| all_symbols.contains(s))
            .cloned()
            .collect()
    };

    let mut series_results = Vec::new();
    let mut total_gaps_filled = 0usize;
    let mut total_volume_bars_fixed = 0usize;
    let mut series_still_failing = 0usize;

    for symbol in &symbols {
        // Determine which timeframes to process for this symbol.
        let db_timeframes = list_timeframes(conn, symbol)?;
        let timeframes: Vec<Timeframe> = if config.timeframes.is_empty() {
            db_timeframes
        } else {
            config
                .timeframes
                .iter()
                .copied()
                .filter(|tf| db_timeframes.contains(tf))
                .collect()
        };

        for timeframe in timeframes {
            info!("Pipeline: processing {} {:?}", symbol, timeframe);

            // Step 1: Gap fill
            let gap_report = fill_gaps(conn, symbol, timeframe)?;
            total_gaps_filled += gap_report.bars_inserted;

            // Step 2: Volume smoothing
            let vol_stats = process_volumes(
                conn,
                symbol,
                timeframe,
                config.volume_window,
                config.volume_max_cluster,
            )?;
            total_volume_bars_fixed += vol_stats.bars_updated;

            // Step 3: Audit (re-load bars after fill/smooth)
            let bars = load_all_bars(conn, symbol, timeframe)?;
            let audit = audit_bars(&bars, timeframe);
            if !audit.passed() {
                series_still_failing += 1;
            }

            series_results.push(SeriesPipelineResult {
                symbol: symbol.clone(),
                timeframe,
                gap_fill: gap_report,
                volume: vol_stats,
                audit,
            });
        }
    }

    Ok(PipelineReport {
        series: series_results,
        total_gaps_filled,
        total_volume_bars_fixed,
        series_still_failing,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// List all distinct symbols present in the bars table.
fn list_symbols_in_db(conn: &Connection) -> Result<Vec<String>, DataError> {
    let mut stmt =
        conn.prepare_cached("SELECT DISTINCT symbol FROM bars ORDER BY symbol ASC")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let symbols: Result<Vec<_>, _> = rows.collect();
    Ok(symbols?)
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

    fn make_bars(_symbol: &str) -> Vec<Bar> {
        vec![
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
            // gap: 1800 missing
            Bar {
                timestamp: 2700,
                open: dec!(1.110),
                high: dec!(1.120),
                low: dec!(1.100),
                close: dec!(1.115),
                volume: 0, // zero-volume bar
                timeframe: Timeframe::M15,
            },
            Bar {
                timestamp: 3600,
                open: dec!(1.115),
                high: dec!(1.125),
                low: dec!(1.105),
                close: dec!(1.120),
                volume: 110,
                timeframe: Timeframe::M15,
            },
        ]
    }

    #[test]
    fn pipeline_fills_gaps_and_smooths_volume() {
        let mut db = Database::in_memory().unwrap();
        insert_bars(db.conn_mut(), "EURUSD", &make_bars("EURUSD")).unwrap();

        let config = PipelineConfig {
            symbols: vec!["EURUSD".to_string()],
            timeframes: vec![Timeframe::M15],
            volume_window: 2,
            volume_max_cluster: 3,
        };
        let report = run_pipeline(db.conn(), &config).unwrap();

        assert_eq!(report.series.len(), 1);
        assert_eq!(report.total_gaps_filled, 1); // ts=1800 synthetic bar inserted
        // Volume processor sees 2 zero-vol bars: ts=1800 (synthetic) + ts=2700 (real)
        assert_eq!(report.total_volume_bars_fixed, 2);
    }

    #[test]
    fn pipeline_reports_still_failing_after_unfixable_issue() {
        let mut db = Database::in_memory().unwrap();
        // Insert bar with invalid OHLC (high < low) which audit will flag
        let bars = vec![
            Bar {
                timestamp: 0,
                open: dec!(1.10),
                high: dec!(1.09), // invalid: high < low
                low: dec!(1.11),
                close: dec!(1.10),
                volume: 100,
                timeframe: Timeframe::M15,
            },
            Bar {
                timestamp: 900,
                open: dec!(1.10),
                high: dec!(1.11),
                low: dec!(1.09),
                close: dec!(1.10),
                volume: 100,
                timeframe: Timeframe::M15,
            },
        ];
        insert_bars(db.conn_mut(), "EURUSD", &bars).unwrap();

        let config = PipelineConfig {
            symbols: vec!["EURUSD".to_string()],
            timeframes: vec![Timeframe::M15],
            ..Default::default()
        };
        let report = run_pipeline(db.conn(), &config).unwrap();
        assert_eq!(report.series_still_failing, 1);
        assert!(!report.passed());
    }
}
