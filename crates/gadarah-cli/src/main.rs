mod config;
mod dataset_cli;
mod phase1;
mod synth;
mod tuner;

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json;

use crate::config::load_config;
use crate::tuner::{find_robust_params, tune_stress_params};
use gadarah_core::Timeframe;
use gadarah_data::{
    aggregate_bars, bar_time_range, count_bars, detect_csv_format, import_csv, import_dataset_dir,
    list_symbols, list_timeframes, load_all_bars, CsvFormat, Database, DatasetImportOptions,
};

const DEFAULT_CONFIG_PATH: &str = "config/gadarah.toml";
const DEFAULT_DB_PATH: &str = "data/gadarah.db";

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "gadarah=info".parse().unwrap()),
        )
        .compact()
        .init();

    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("help");

    match cmd {
        "import" => cmd_import(&args[2..]),
        "bulk-import" => cmd_bulk_import(&args[2..]),
        "aggregate" => cmd_aggregate(&args[2..]),
        "audit-data" => phase1::run_audit_data(&args[2..]),
        "backtest" => phase1::run_backtest(&args[2..]),
        "dataset-build" => dataset_cli::run_dataset_build(&args[2..]),
        "dataset-report" => dataset_cli::run_dataset_report(&args[2..]),
        "dataset-pipeline" => dataset_cli::run_dataset_pipeline(&args[2..]),
        "diagnose" => cmd_diagnose(&args[2..]),
        "validate" => phase1::run_validate(&args[2..]),
        "portfolio" => cmd_portfolio(&args[2..]),
        "synth" => cmd_synth(&args[2..]),
        "full" => cmd_full(&args[2..]),
        "tune" => cmd_tune(&args[2..]),
        "live" => phase1::run_live(&args[2..]),
        "benchmarks" => phase1::run_benchmarks(&args[2..]),
        "help" | "--help" | "-h" => print_help(),
        _ => {
            eprintln!("Unknown command: {cmd}");
            print_help();
        }
    }
}

/// Send a notification to a Discord channel via webhook.
/// URL is read from the `GADARAH_DISCORD_WEBHOOK` environment variable.
/// Silently does nothing if the variable is unset or the request fails.
pub fn notify_discord(msg: &str) {
    let Ok(url) = std::env::var("GADARAH_DISCORD_WEBHOOK") else {
        return;
    };
    let body = serde_json::json!({ "content": msg });
    if let Err(err) = ureq::post(&url).send_json(body) {
        tracing::warn!("Discord notification failed: {err}");
    }
}

fn print_help() {
    println!("GADARAH — Prop Trading Engine CLI");
    println!();
    println!("Usage: gadarah <command> [options]");
    println!();
    println!("Commands:");
    println!("  import            <csv_file> <symbol> <timeframe> [format] [--db <path>]");
    println!("  bulk-import       <dir> [--db <path>]");
    println!("  aggregate         <from_tf> <to_tf> [--symbol <sym>] [--db <path>]");
    println!("  audit-data        [phase1 options]");
    println!("  backtest          [phase1 options]");
    println!("  dataset-build     [dataset options]");
    println!("  dataset-report    [dataset options]");
    println!("  dataset-pipeline  Alias for dataset-build");
    println!("  diagnose          [--db <path>] [--symbol <sym>]");
    println!("  validate          [phase1 options]");
    println!("  portfolio         [--db <path>] [--symbols <csv>] [--risk <pct>]");
    println!("  synth             [--db <path>] [--symbol <sym>] [--bars <n> | --two-years]");
    println!("  full              Alias for validate");
    println!("  tune              [--db <path>] [--symbols <csv>] [--iterations <n>]");
    println!("  live              [phase1 options]");
    println!("  benchmarks        [phase1 options]");
}

fn cmd_import(args: &[String]) {
    if args.len() < 3 {
        eprintln!("Usage: gadarah import <csv_file> <symbol> <timeframe> [format] [--db <path>]");
        return;
    }

    let csv_path = Path::new(&args[0]);
    let symbol = &args[1];
    let timeframe = match parse_timeframe(&args[2]) {
        Ok(tf) => tf,
        Err(err) => {
            eprintln!("{err}");
            return;
        }
    };

    let format = match args.get(3) {
        Some(raw) if !raw.starts_with("--") => match parse_csv_format(raw) {
            Ok(fmt) => fmt,
            Err(err) => {
                eprintln!("{err}");
                return;
            }
        },
        _ => match detect_csv_format(csv_path) {
            Ok(fmt) => fmt,
            Err(err) => {
                eprintln!("Failed to auto-detect CSV format: {err}");
                return;
            }
        },
    };

    let db_path = arg_value(args, "--db").unwrap_or_else(default_db_path);
    let mut db = match Database::open(&db_path) {
        Ok(db) => db,
        Err(err) => {
            eprintln!("Failed to open database {}: {}", db_path, err);
            return;
        }
    };

    let file = match File::open(csv_path) {
        Ok(file) => file,
        Err(err) => {
            eprintln!("Failed to open {}: {}", csv_path.display(), err);
            return;
        }
    };

    match import_csv(
        db.conn_mut(),
        BufReader::new(file),
        symbol,
        timeframe,
        format,
    ) {
        Ok(count) => {
            println!(
                "Imported {} bars for {} {:?} into {}",
                count, symbol, timeframe, db_path
            )
        }
        Err(err) => eprintln!("Import failed: {err}"),
    }
}

fn cmd_bulk_import(args: &[String]) {
    let Some(dir) = args.first() else {
        eprintln!("Usage: gadarah bulk-import <dir> [--db <path>]");
        return;
    };

    let db_path = arg_value(args, "--db").unwrap_or_else(default_db_path);
    let mut db = match Database::open(&db_path) {
        Ok(db) => db,
        Err(err) => {
            eprintln!("Failed to open database {}: {}", db_path, err);
            return;
        }
    };

    match import_dataset_dir(
        db.conn_mut(),
        Path::new(dir),
        &DatasetImportOptions::default(),
    ) {
        Ok(result) => {
            println!("Imported {} files into {}", result.files.len(), db_path);
            println!("Total bars: {}", result.total_bars_imported);
        }
        Err(err) => eprintln!("Bulk import failed: {err}"),
    }
}

fn cmd_aggregate(args: &[String]) {
    if args.len() < 2 {
        eprintln!("Usage: gadarah aggregate <from_tf> <to_tf> [--symbol <sym>] [--db <path>]");
        return;
    }

    let from_tf = match parse_timeframe(&args[0]) {
        Ok(tf) => tf,
        Err(err) => {
            eprintln!("{err}");
            return;
        }
    };
    let to_tf = match parse_timeframe(&args[1]) {
        Ok(tf) => tf,
        Err(err) => {
            eprintln!("{err}");
            return;
        }
    };
    let db_path = arg_value(args, "--db").unwrap_or_else(default_db_path);
    let mut db = match Database::open(&db_path) {
        Ok(db) => db,
        Err(err) => {
            eprintln!("Failed to open database {}: {}", db_path, err);
            return;
        }
    };

    let symbols = match arg_value(args, "--symbol") {
        Some(symbol) => vec![symbol],
        None => match list_symbols(db.conn()) {
            Ok(symbols) => symbols,
            Err(err) => {
                eprintln!("Failed to list symbols: {err}");
                return;
            }
        },
    };

    for symbol in symbols {
        let source = match load_all_bars(db.conn(), &symbol, from_tf) {
            Ok(bars) => bars,
            Err(err) => {
                eprintln!("Failed to load {} {:?} bars: {}", symbol, from_tf, err);
                continue;
            }
        };

        if source.is_empty() {
            println!("Skipping {} {:?}: no source bars", symbol, from_tf);
            continue;
        }

        match aggregate_bars(&source, to_tf) {
            Ok(aggregated) => {
                match gadarah_data::insert_bars(db.conn_mut(), &symbol, &aggregated) {
                    Ok(_count) => println!(
                        "Aggregated {} {:?} bars into {} {:?} bars",
                        source.len(),
                        from_tf,
                        symbol,
                        to_tf
                    ),
                    Err(err) => {
                        eprintln!("Failed to write aggregated bars for {}: {}", symbol, err)
                    }
                }
            }
            Err(err) => eprintln!("Aggregation failed for {}: {}", symbol, err),
        }
    }
}

fn cmd_diagnose(args: &[String]) {
    let db_path = arg_value(args, "--db").unwrap_or_else(default_db_path);
    let db = match Database::open(&db_path) {
        Ok(db) => db,
        Err(err) => {
            eprintln!("Failed to open database {}: {}", db_path, err);
            return;
        }
    };

    let symbols = match arg_value(args, "--symbol") {
        Some(symbol) => vec![symbol],
        None => match list_symbols(db.conn()) {
            Ok(symbols) => symbols,
            Err(err) => {
                eprintln!("Failed to list symbols: {err}");
                return;
            }
        },
    };

    println!("Database: {}", db_path);
    if symbols.is_empty() {
        println!("No symbols found.");
        return;
    }

    for symbol in symbols {
        println!();
        println!("{}", "=".repeat(50));
        println!("SYMBOL {}", symbol);
        println!("{}", "=".repeat(50));
        let timeframes = match list_timeframes(db.conn(), &symbol) {
            Ok(tfs) => tfs,
            Err(err) => {
                eprintln!("Failed to list timeframes for {}: {}", symbol, err);
                continue;
            }
        };

        if timeframes.is_empty() {
            println!("No timeframes stored.");
            continue;
        }

        for timeframe in timeframes {
            let count = count_bars(db.conn(), &symbol, timeframe).unwrap_or(0);
            let range = bar_time_range(db.conn(), &symbol, timeframe).ok().flatten();
            match range {
                Some((start, end)) => {
                    println!(
                        "{:?}: {:>8} bars  range=[{}, {}]",
                        timeframe, count, start, end
                    );
                }
                None => println!("{:?}: {:>8} bars", timeframe, count),
            }
        }
    }
}

fn cmd_portfolio(args: &[String]) {
    let db_path = arg_value(args, "--db").unwrap_or_else(default_db_path);
    let db = match Database::open(&db_path) {
        Ok(db) => db,
        Err(err) => {
            eprintln!("Failed to open database {}: {}", db_path, err);
            return;
        }
    };

    let symbols = parse_csv_arg(args, "--symbols")
        .unwrap_or_else(|| list_symbols(db.conn()).unwrap_or_default());
    let risk = parse_decimal_flag(args, "--risk").unwrap_or(dec!(0.74));

    if symbols.is_empty() {
        eprintln!("No symbols available. Pass --symbols or import data first.");
        return;
    }

    println!("Portfolio Summary");
    println!("Database:      {}", db_path);
    println!("Base risk:     {}%", risk);
    println!("Symbols:       {}", symbols.join(", "));
    println!("Heat if equal: {}%", risk * Decimal::from(symbols.len()));
    println!();

    for symbol in symbols {
        let count = count_bars(db.conn(), &symbol, Timeframe::M15).unwrap_or(0);
        let range = bar_time_range(db.conn(), &symbol, Timeframe::M15)
            .ok()
            .flatten();
        match range {
            Some((start, end)) => {
                println!("{} M15 bars={} range=[{}, {}]", symbol, count, start, end)
            }
            None => println!("{} M15 bars={}", symbol, count),
        }
    }
}

fn cmd_synth(args: &[String]) {
    let db_path = arg_value(args, "--db").unwrap_or_else(default_db_path);
    let symbol = arg_value(args, "--symbol").unwrap_or_else(|| "EURUSD".to_string());
    let start_price = parse_decimal_flag(args, "--price").unwrap_or(dec!(1.1000));
    let seed = parse_u64_flag(args, "--seed").unwrap_or(42);
    let bars = parse_usize_flag(args, "--bars").unwrap_or(1_000);
    let two_years = has_flag(args, "--two-years");
    let start_ts = parse_i64_flag(args, "--start-ts").unwrap_or(1_704_067_200);

    let series = if two_years {
        synth::generate_2y_bars(&symbol, start_price, seed)
    } else {
        synth::generate_bars(&symbol, bars, start_price, start_ts, seed)
    };

    let mut db = match Database::open(&db_path) {
        Ok(db) => db,
        Err(err) => {
            eprintln!("Failed to open database {}: {}", db_path, err);
            return;
        }
    };

    match gadarah_data::insert_bars(db.conn_mut(), &symbol, &series) {
        Ok(count) => println!(
            "Inserted {} synthetic {:?} bars for {} into {}",
            count,
            Timeframe::M15,
            symbol,
            db_path
        ),
        Err(err) => eprintln!("Synthetic import failed: {err}"),
    }
}

fn cmd_full(args: &[String]) {
    phase1::run_validate(args);
}

fn cmd_tune(args: &[String]) {
    let db_path = arg_value(args, "--db").unwrap_or_else(default_db_path);
    let raw_symbols =
        parse_csv_arg(args, "--symbols").unwrap_or_else(|| vec!["EURUSD".to_string()]);
    let iterations = parse_usize_flag(args, "--iterations").unwrap_or(100);
    let symbol_refs: Vec<&str> = raw_symbols.iter().map(String::as_str).collect();

    let results = tune_stress_params(&db_path, &symbol_refs, iterations);
    if results.is_empty() {
        eprintln!("No tuning results generated.");
        return;
    }

    let best = find_robust_params(&results);
    println!(
        "Selected stress config: loss_multiplier={} win_rate_reduction={} extra_slippage_usd={}",
        best.loss_multiplier, best.win_rate_reduction, best.extra_slippage_usd
    );
}

fn default_db_path() -> String {
    load_config(Path::new(DEFAULT_CONFIG_PATH))
        .map(|cfg| cfg.engine.db_path)
        .unwrap_or_else(|_| DEFAULT_DB_PATH.to_string())
}

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].clone())
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|arg| arg == flag)
}

fn parse_csv_arg(args: &[String], flag: &str) -> Option<Vec<String>> {
    let raw = arg_value(args, flag)?;
    let values = raw
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .collect::<Vec<_>>();
    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}

fn parse_decimal_flag(args: &[String], flag: &str) -> Option<Decimal> {
    arg_value(args, flag).and_then(|raw| raw.parse::<Decimal>().ok())
}

fn parse_i64_flag(args: &[String], flag: &str) -> Option<i64> {
    arg_value(args, flag).and_then(|raw| raw.parse::<i64>().ok())
}

fn parse_u64_flag(args: &[String], flag: &str) -> Option<u64> {
    arg_value(args, flag).and_then(|raw| raw.parse::<u64>().ok())
}

fn parse_usize_flag(args: &[String], flag: &str) -> Option<usize> {
    arg_value(args, flag).and_then(|raw| raw.parse::<usize>().ok())
}

fn parse_csv_format(raw: &str) -> Result<CsvFormat, String> {
    match raw.to_ascii_lowercase().as_str() {
        "mt" | "metatrader" => Ok(CsvFormat::MetaTrader),
        "ctrader" | "ct" => Ok(CsvFormat::CTrader),
        "unix" => Ok(CsvFormat::Unix),
        other => Err(format!(
            "Invalid CSV format {other}. Use metatrader, ctrader, or unix."
        )),
    }
}

fn parse_timeframe(raw: &str) -> Result<Timeframe, String> {
    match raw.to_uppercase().as_str() {
        "M1" => Ok(Timeframe::M1),
        "M5" => Ok(Timeframe::M5),
        "M15" => Ok(Timeframe::M15),
        "H1" => Ok(Timeframe::H1),
        "H4" => Ok(Timeframe::H4),
        "D1" => Ok(Timeframe::D1),
        _ => Err(format!(
            "Invalid timeframe: {raw}. Use M1, M5, M15, H1, H4, D1"
        )),
    }
}
