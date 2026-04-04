use std::collections::{BTreeSet, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use gadarah_core::Timeframe;

use crate::aggregator::aggregate_bars;
use crate::audit::{audit_bars, DataAuditResult};
use crate::csv_import::{import_csv, CsvFormat};
use crate::error::DataError;
use crate::store::{
    bar_time_range, count_bars, insert_bars, list_symbols, load_all_bars, str_to_tf,
};

const DEFAULT_MIN_HISTORY_DAYS: i64 = 730;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetRequirements {
    pub required_symbols: Vec<String>,
    pub required_timeframes: Vec<Timeframe>,
    pub min_history_days: i64,
}

impl Default for DatasetRequirements {
    fn default() -> Self {
        Self {
            required_symbols: Vec::new(),
            required_timeframes: vec![Timeframe::M15],
            min_history_days: DEFAULT_MIN_HISTORY_DAYS,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetSeriesReport {
    pub symbol: String,
    pub timeframe: Timeframe,
    pub present: bool,
    pub total_bars: usize,
    pub start_ts: Option<i64>,
    pub end_ts: Option<i64>,
    pub history_span_days: i64,
    pub audit: Option<DataAuditResult>,
    pub audit_passed: bool,
    pub history_passed: bool,
    pub all_zero_volume: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetReadinessReport {
    pub requirements: DatasetRequirements,
    pub series: Vec<DatasetSeriesReport>,
    pub missing_series: usize,
    pub failed_audits: usize,
    pub short_history_series: usize,
    pub all_zero_volume_series: usize,
}

impl DatasetReadinessReport {
    pub fn passed(&self) -> bool {
        self.missing_series == 0 && self.failed_audits == 0 && self.short_history_series == 0
    }
}

#[derive(Debug, Clone)]
pub struct DatasetImportOptions {
    pub symbols: Option<BTreeSet<String>>,
    pub timeframes: Option<HashSet<Timeframe>>,
    pub recursive: bool,
}

impl Default for DatasetImportOptions {
    fn default() -> Self {
        Self {
            symbols: None,
            timeframes: None,
            recursive: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DatasetFileSpec {
    pub path: PathBuf,
    pub symbol: String,
    pub timeframe: Timeframe,
    pub format: CsvFormat,
}

#[derive(Debug, Clone)]
pub struct FileImportResult {
    pub path: PathBuf,
    pub symbol: String,
    pub timeframe: Timeframe,
    pub format: CsvFormat,
    pub bars_imported: usize,
}

#[derive(Debug, Clone, Default)]
pub struct DatasetImportResult {
    pub files: Vec<FileImportResult>,
    pub total_bars_imported: usize,
}

#[derive(Debug, Clone)]
pub struct DerivedSeriesResult {
    pub symbol: String,
    pub source_timeframe: Timeframe,
    pub target_timeframe: Timeframe,
    pub bars_written: usize,
}

pub fn detect_csv_format(path: &Path) -> Result<CsvFormat, DataError> {
    let file = File::open(path).map_err(|e| DataError::Database(e.to_string()))?;
    let reader = BufReader::new(file);

    let mut header: Option<String> = None;
    let mut sample: Option<String> = None;

    for line in reader.lines() {
        let line = line.map_err(|e| DataError::Database(e.to_string()))?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if header.is_none() {
            header = Some(trimmed.to_string());
            if looks_like_data_row(trimmed) {
                sample = Some(trimmed.to_string());
                break;
            }
            continue;
        }
        sample = Some(trimmed.to_string());
        break;
    }

    let header = header.ok_or_else(|| DataError::Parse(format!("{} is empty", path.display())))?;
    let lower = header.to_ascii_lowercase();
    if lower.starts_with("date,time") {
        return Ok(CsvFormat::MetaTrader);
    }

    let first_value = sample
        .as_deref()
        .unwrap_or(header.as_str())
        .split([',', '\t'])
        .next()
        .unwrap_or("")
        .trim();

    if lower.starts_with("timestamp") {
        return Ok(if first_value.len() > 10 {
            CsvFormat::CTrader
        } else {
            CsvFormat::Unix
        });
    }

    if first_value.contains('.') || first_value.contains('/') || first_value.contains('-') {
        return Ok(CsvFormat::MetaTrader);
    }

    Ok(if first_value.len() > 10 {
        CsvFormat::CTrader
    } else {
        CsvFormat::Unix
    })
}

pub fn discover_dataset_files(
    dir: &Path,
    options: &DatasetImportOptions,
) -> Result<Vec<DatasetFileSpec>, DataError> {
    let mut paths = Vec::new();
    collect_csv_paths(dir, options.recursive, &mut paths)?;
    paths.sort();

    let mut files = Vec::new();
    for path in paths {
        let Some((symbol, timeframe)) = parse_symbol_timeframe(&path)? else {
            continue;
        };

        if let Some(symbols) = &options.symbols {
            if !symbols.contains(&symbol) {
                continue;
            }
        }
        if let Some(timeframes) = &options.timeframes {
            if !timeframes.contains(&timeframe) {
                continue;
            }
        }

        files.push(DatasetFileSpec {
            format: detect_csv_format(&path)?,
            path,
            symbol,
            timeframe,
        });
    }

    Ok(files)
}

pub fn import_dataset_dir(
    conn: &mut Connection,
    dir: &Path,
    options: &DatasetImportOptions,
) -> Result<DatasetImportResult, DataError> {
    let specs = discover_dataset_files(dir, options)?;
    let mut result = DatasetImportResult::default();

    for spec in specs {
        let file = File::open(&spec.path).map_err(|e| DataError::Database(e.to_string()))?;
        let reader = BufReader::new(file);
        let bars_imported = import_csv(conn, reader, &spec.symbol, spec.timeframe, spec.format)?;
        result.total_bars_imported += bars_imported;
        result.files.push(FileImportResult {
            path: spec.path,
            symbol: spec.symbol,
            timeframe: spec.timeframe,
            format: spec.format,
            bars_imported,
        });
    }

    Ok(result)
}

pub fn derive_timeframes_for_symbol(
    conn: &mut Connection,
    symbol: &str,
    source_tf: Timeframe,
    targets: &[Timeframe],
) -> Result<Vec<DerivedSeriesResult>, DataError> {
    let source = load_all_bars(conn, symbol, source_tf)?;
    if source.is_empty() {
        return Ok(Vec::new());
    }

    let mut derived = Vec::new();
    for &target_tf in targets {
        if target_tf.seconds() <= source_tf.seconds() {
            continue;
        }

        let aggregated = aggregate_bars(&source, target_tf)?;
        let written = insert_bars(conn, symbol, &aggregated)?;
        derived.push(DerivedSeriesResult {
            symbol: symbol.to_string(),
            source_timeframe: source_tf,
            target_timeframe: target_tf,
            bars_written: written,
        });
    }

    Ok(derived)
}

pub fn build_dataset_readiness_report(
    conn: &Connection,
    requirements: &DatasetRequirements,
) -> Result<DatasetReadinessReport, DataError> {
    let mut required_symbols = if requirements.required_symbols.is_empty() {
        list_symbols(conn)?
    } else {
        requirements.required_symbols.clone()
    };
    required_symbols.sort();
    required_symbols.dedup();

    let mut series = Vec::new();
    let mut missing_series = 0usize;
    let mut failed_audits = 0usize;
    let mut short_history_series = 0usize;
    let mut all_zero_volume_series = 0usize;

    for symbol in required_symbols {
        for &timeframe in &requirements.required_timeframes {
            let total_bars = count_bars(conn, &symbol, timeframe)? as usize;
            let time_range = bar_time_range(conn, &symbol, timeframe)?;

            if total_bars == 0 || time_range.is_none() {
                missing_series += 1;
                series.push(DatasetSeriesReport {
                    symbol: symbol.clone(),
                    timeframe,
                    present: false,
                    total_bars,
                    start_ts: None,
                    end_ts: None,
                    history_span_days: 0,
                    audit: None,
                    audit_passed: false,
                    history_passed: false,
                    all_zero_volume: false,
                });
                continue;
            }

            let (start_ts, end_ts) = time_range.unwrap();
            let history_span_days = ((end_ts - start_ts) / 86_400).max(0) + 1;
            let bars = load_all_bars(conn, &symbol, timeframe)?;
            let audit = audit_bars(&bars, timeframe);
            let audit_passed = audit.passed();
            let history_passed = history_span_days >= requirements.min_history_days;
            let all_zero_volume =
                audit.total_bars > 0 && audit.zero_volume_bars == audit.total_bars;

            if !audit_passed {
                failed_audits += 1;
            }
            if !history_passed {
                short_history_series += 1;
            }
            if all_zero_volume {
                all_zero_volume_series += 1;
            }

            series.push(DatasetSeriesReport {
                symbol: symbol.clone(),
                timeframe,
                present: true,
                total_bars,
                start_ts: Some(start_ts),
                end_ts: Some(end_ts),
                history_span_days,
                audit: Some(audit),
                audit_passed,
                history_passed,
                all_zero_volume,
            });
        }
    }

    Ok(DatasetReadinessReport {
        requirements: requirements.clone(),
        series,
        missing_series,
        failed_audits,
        short_history_series,
        all_zero_volume_series,
    })
}

fn collect_csv_paths(dir: &Path, recursive: bool, acc: &mut Vec<PathBuf>) -> Result<(), DataError> {
    for entry in fs::read_dir(dir).map_err(|e| DataError::Database(e.to_string()))? {
        let entry = entry.map_err(|e| DataError::Database(e.to_string()))?;
        let path = entry.path();
        if path.is_dir() {
            if recursive {
                collect_csv_paths(&path, true, acc)?;
            }
            continue;
        }
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("csv"))
        {
            acc.push(path);
        }
    }
    Ok(())
}

fn parse_symbol_timeframe(path: &Path) -> Result<Option<(String, Timeframe)>, DataError> {
    let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return Ok(None);
    };

    let Some((symbol, tf_raw)) = stem.rsplit_once('_') else {
        return Ok(None);
    };

    Ok(Some((symbol.to_string(), str_to_tf(tf_raw)?)))
}

fn looks_like_data_row(line: &str) -> bool {
    line.split([',', '\t'])
        .next()
        .is_some_and(|first| first.chars().all(|ch| ch.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use rust_decimal_macros::dec;

    use super::*;
    use crate::schema::init_schema;

    fn unique_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("gadarah_{name}_{nanos}"))
    }

    #[test]
    fn detects_csv_formats() {
        let dir = unique_path("formats");
        fs::create_dir_all(&dir).unwrap();

        let unix = dir.join("EURUSD_M15.csv");
        let ctrader = dir.join("EURUSD_H1.csv");
        let mt = dir.join("EURUSD_D1.csv");

        fs::write(
            &unix,
            "timestamp,open,high,low,close,volume\n1700000000,1,2,0,1,0\n",
        )
        .unwrap();
        fs::write(
            &ctrader,
            "Timestamp,Open,High,Low,Close,Volume\n1700000000000,1,2,0,1,0\n",
        )
        .unwrap();
        fs::write(
            &mt,
            "Date,Time,Open,High,Low,Close,Volume\n2024.01.02,13:30,1,2,0,1,1\n",
        )
        .unwrap();

        assert!(matches!(detect_csv_format(&unix).unwrap(), CsvFormat::Unix));
        assert!(matches!(
            detect_csv_format(&ctrader).unwrap(),
            CsvFormat::CTrader
        ));
        assert!(matches!(
            detect_csv_format(&mt).unwrap(),
            CsvFormat::MetaTrader
        ));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn imports_dataset_directory() {
        let dir = unique_path("import");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("EURUSD_M15.csv"),
            "timestamp,open,high,low,close,volume\n1700000000,1.1,1.2,1.0,1.15,0\n1700000900,1.15,1.25,1.1,1.2,0\n",
        )
        .unwrap();

        let mut conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        let result = import_dataset_dir(&mut conn, &dir, &DatasetImportOptions::default()).unwrap();

        assert_eq!(result.files.len(), 1);
        assert_eq!(result.total_bars_imported, 2);
        assert_eq!(count_bars(&conn, "EURUSD", Timeframe::M15).unwrap(), 2);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn readiness_report_flags_short_history() {
        let mut conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();

        let bars = vec![
            gadarah_core::Bar {
                open: dec!(1.1000),
                high: dec!(1.1010),
                low: dec!(1.0990),
                close: dec!(1.1005),
                volume: 0,
                timestamp: 0,
                timeframe: Timeframe::M15,
            },
            gadarah_core::Bar {
                open: dec!(1.1005),
                high: dec!(1.1015),
                low: dec!(1.1000),
                close: dec!(1.1010),
                volume: 0,
                timestamp: 900,
                timeframe: Timeframe::M15,
            },
        ];
        insert_bars(&mut conn, "EURUSD", &bars).unwrap();

        let report = build_dataset_readiness_report(
            &conn,
            &DatasetRequirements {
                required_symbols: vec!["EURUSD".to_string()],
                required_timeframes: vec![Timeframe::M15],
                min_history_days: 730,
            },
        )
        .unwrap();

        assert_eq!(report.series.len(), 1);
        assert_eq!(report.short_history_series, 1);
        assert_eq!(report.all_zero_volume_series, 1);
        assert!(!report.passed());
    }
}
