use std::collections::{BTreeSet, HashSet};
use std::path::Path;

use gadarah_core::Timeframe;
use gadarah_data::{
    build_dataset_readiness_report, derive_timeframes_for_symbol, import_dataset_dir, Database,
    DatasetImportOptions, DatasetReadinessReport, DatasetRequirements,
};

use crate::config::load_config;

const DEFAULT_CONFIG_PATH: &str = "config/gadarah.toml";
const DEFAULT_DATASET_DB_PATH: &str = "data/phase1_dataset.db";
const DEFAULT_SOURCE_DIR: &str = "data/fetched";

pub fn run_dataset_build(args: &[String]) {
    let db_path = arg_value(args, "--db").unwrap_or_else(|| DEFAULT_DATASET_DB_PATH.to_string());
    let source_dir = arg_value(args, "--source").unwrap_or_else(|| DEFAULT_SOURCE_DIR.to_string());
    let symbols = parse_symbol_filter(args);
    let timeframes = parse_timeframe_filter(args, "--timeframes");
    let required_timeframes =
        parse_timeframe_list(args, "--required-timeframes").unwrap_or_else(|| vec![Timeframe::M15]);
    let min_history_days = parse_i64_arg(args, "--min-history-days", 730);
    let derive_from = arg_value(args, "--derive-from").and_then(|raw| parse_timeframe(&raw).ok());
    let derive_to = parse_timeframe_list(args, "--derive-to").unwrap_or_default();

    if let Some(parent) = Path::new(&db_path).parent() {
        std::fs::create_dir_all(parent).ok();
    }

    let mut db = match Database::open(&db_path) {
        Ok(db) => db,
        Err(err) => {
            eprintln!("Failed to open database {}: {}", db_path, err);
            return;
        }
    };

    let import_options = DatasetImportOptions {
        symbols: symbols.clone(),
        timeframes,
        recursive: true,
    };
    let import_result =
        match import_dataset_dir(db.conn_mut(), Path::new(&source_dir), &import_options) {
            Ok(result) => result,
            Err(err) => {
                eprintln!("Dataset import failed: {err}");
                return;
            }
        };

    let mut derived_count = 0usize;
    if let Some(source_tf) = derive_from {
        let mut symbols_to_derive = BTreeSet::new();
        for file in &import_result.files {
            symbols_to_derive.insert(file.symbol.clone());
        }
        if symbols_to_derive.is_empty() {
            symbols_to_derive.extend(symbols.clone().unwrap_or_default());
        }

        for symbol in symbols_to_derive {
            match derive_timeframes_for_symbol(db.conn_mut(), &symbol, source_tf, &derive_to) {
                Ok(results) => {
                    for derived in results {
                        println!(
                            "Derived {:?} -> {:?} for {} ({} bars)",
                            derived.source_timeframe,
                            derived.target_timeframe,
                            derived.symbol,
                            derived.bars_written
                        );
                        derived_count += derived.bars_written;
                    }
                }
                Err(err) => {
                    eprintln!("Failed to derive timeframes for {}: {}", symbol, err);
                    return;
                }
            }
        }
    }

    let requirements = DatasetRequirements {
        required_symbols: symbols
            .map(|set| set.into_iter().collect())
            .unwrap_or_default(),
        required_timeframes,
        min_history_days,
    };

    let readiness = match build_dataset_readiness_report(db.conn(), &requirements) {
        Ok(report) => report,
        Err(err) => {
            eprintln!("Failed to build dataset report: {err}");
            return;
        }
    };

    println!("{}", "=".repeat(60));
    println!("DATASET BUILD");
    println!("{}", "=".repeat(60));
    println!("Source dir:        {}", source_dir);
    println!("Database:          {}", db_path);
    println!("Imported files:    {}", import_result.files.len());
    println!("Imported bars:     {}", import_result.total_bars_imported);
    println!("Derived bars:      {}", derived_count);
    if !import_result.files.is_empty() {
        println!("\nImported series:");
        for file in &import_result.files {
            println!(
                "  {} {:?} {:?}: {}",
                file.symbol, file.timeframe, file.format, file.bars_imported
            );
        }
    }

    println!();
    print_dataset_readiness(&readiness);
}

pub fn run_dataset_pipeline(args: &[String]) {
    println!("DATASET PIPELINE");
    println!("Using the stable dataset import + readiness flow.");
    println!();
    run_dataset_build(args);
}

pub fn run_dataset_report(args: &[String]) {
    let config_path =
        arg_value(args, "--config").unwrap_or_else(|| DEFAULT_CONFIG_PATH.to_string());
    let config = load_config(Path::new(&config_path)).ok();
    let db_path = arg_value(args, "--db")
        .or_else(|| config.as_ref().map(|cfg| cfg.engine.db_path.clone()))
        .unwrap_or_else(|| "data/gadarah.db".to_string());
    let symbols = parse_symbol_filter(args)
        .map(|set| set.into_iter().collect())
        .unwrap_or_default();
    let required_timeframes =
        parse_timeframe_list(args, "--required-timeframes").unwrap_or_else(|| vec![Timeframe::M15]);
    let min_history_days = parse_i64_arg(args, "--min-history-days", 730);

    let db = match Database::open(&db_path) {
        Ok(db) => db,
        Err(err) => {
            eprintln!("Failed to open database {}: {}", db_path, err);
            return;
        }
    };

    let report = match build_dataset_readiness_report(
        db.conn(),
        &DatasetRequirements {
            required_symbols: symbols,
            required_timeframes,
            min_history_days,
        },
    ) {
        Ok(report) => report,
        Err(err) => {
            eprintln!("Dataset report failed: {err}");
            return;
        }
    };

    println!("{}", "=".repeat(60));
    println!("DATASET REPORT");
    println!("{}", "=".repeat(60));
    println!("Database:          {}", db_path);
    print_dataset_readiness(&report);
}

pub fn print_dataset_readiness(report: &DatasetReadinessReport) {
    println!(
        "Required tfs:      {}",
        report
            .requirements
            .required_timeframes
            .iter()
            .map(|tf| format!("{tf:?}"))
            .collect::<Vec<_>>()
            .join(",")
    );
    println!(
        "Min history days:  {}",
        report.requirements.min_history_days
    );
    println!("Missing series:    {}", report.missing_series);
    println!("Failed audits:     {}", report.failed_audits);
    println!("Short history:     {}", report.short_history_series);
    println!("All-zero volume:   {}", report.all_zero_volume_series);
    println!(
        "Verdict:           {}",
        if report.passed() { "PASS" } else { "FAIL" }
    );

    if report.series.is_empty() {
        return;
    }

    println!("\nSeries:");
    println!(
        "{:<8} {:<4} {:>7} {:>6} {:>6} {:>6} {:>7}",
        "Symbol", "TF", "Bars", "Days", "Audit", "Hist", "Volume"
    );
    println!("{}", "-".repeat(56));
    for series in &report.series {
        let audit = if !series.present {
            "MISS".to_string()
        } else if series.audit_passed {
            "PASS".to_string()
        } else {
            "FAIL".to_string()
        };
        let history = if !series.present {
            "MISS"
        } else if series.history_passed {
            "PASS"
        } else {
            "FAIL"
        };
        let volume = if !series.present {
            "-"
        } else if series.all_zero_volume {
            "ZERO"
        } else {
            "OK"
        };
        println!(
            "{:<8} {:<4} {:>7} {:>6} {:>6} {:>6} {:>7}",
            series.symbol,
            format!("{:?}", series.timeframe),
            series.total_bars,
            series.history_span_days,
            audit,
            history,
            volume
        );
    }

    let mut issues = Vec::new();
    for series in &report.series {
        if !series.present {
            issues.push(format!(
                "{} {:?}: missing series",
                series.symbol, series.timeframe
            ));
            continue;
        }

        if !series.history_passed {
            issues.push(format!(
                "{} {:?}: history {}d < {}d",
                series.symbol,
                series.timeframe,
                series.history_span_days,
                report.requirements.min_history_days
            ));
        }

        if let Some(audit) = &series.audit {
            if !series.audit_passed {
                issues.push(format!(
                    "{} {:?}: gaps={} missing={} duplicates={} misaligned={}",
                    series.symbol,
                    series.timeframe,
                    audit.unexpected_gap_count,
                    audit.missing_bar_estimate,
                    audit.duplicate_timestamps,
                    audit.misaligned_timestamps
                ));
            }
        }

        if series.all_zero_volume {
            issues.push(format!(
                "{} {:?}: all bars have zero volume",
                series.symbol, series.timeframe
            ));
        }
    }

    if !issues.is_empty() {
        println!("\nIssues:");
        for issue in issues {
            println!("  {}", issue);
        }
    }
}

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].clone())
}

fn parse_timeframe(raw: &str) -> Result<Timeframe, String> {
    match raw.to_uppercase().as_str() {
        "M1" => Ok(Timeframe::M1),
        "M5" => Ok(Timeframe::M5),
        "M15" => Ok(Timeframe::M15),
        "H1" => Ok(Timeframe::H1),
        "H4" => Ok(Timeframe::H4),
        "D1" => Ok(Timeframe::D1),
        _ => Err(format!("Invalid timeframe: {raw}")),
    }
}

fn parse_timeframe_filter(args: &[String], flag: &str) -> Option<HashSet<Timeframe>> {
    parse_timeframe_list(args, flag).map(|list| list.into_iter().collect())
}

fn parse_timeframe_list(args: &[String], flag: &str) -> Option<Vec<Timeframe>> {
    let raw = arg_value(args, flag)?;
    let mut tfs = Vec::new();
    for part in raw.split(',') {
        match parse_timeframe(part.trim()) {
            Ok(tf) => tfs.push(tf),
            Err(err) => {
                eprintln!("{err}");
                return None;
            }
        }
    }
    if tfs.is_empty() {
        None
    } else {
        Some(tfs)
    }
}

fn parse_symbol_filter(args: &[String]) -> Option<BTreeSet<String>> {
    let raw = arg_value(args, "--symbols")?;
    let symbols = raw
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| part.to_string())
        .collect::<BTreeSet<_>>();
    if symbols.is_empty() {
        None
    } else {
        Some(symbols)
    }
}

fn parse_i64_arg(args: &[String], flag: &str, default: i64) -> i64 {
    arg_value(args, flag)
        .and_then(|raw| raw.parse::<i64>().ok())
        .unwrap_or(default)
}
