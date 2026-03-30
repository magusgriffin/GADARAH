mod config;
mod synth;

use std::time::Instant;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use std::collections::HashMap;

use gadarah_backtest::{
    run_monte_carlo, run_replay, run_stress_test, simulate_challenges, BacktestStats,
    ChallengeRules, ChallengeSimResult, MonteCarloConfig, ReplayConfig, StressConfig,
};
use gadarah_broker::MockConfig;
use gadarah_feed::{
    binance::BinanceFeed, types::FeedMessage, BarStreamer, Feed,
};
use gadarah_core::{
    heads::{
        asian_range::{AsianRangeConfig, AsianRangeHead},
        breakout::{BreakoutConfig, BreakoutHead},
        momentum::{MomentumConfig, MomentumHead},
    },
    utc_hour, BBWidthPercentile, BollingerBands, Head, RegimeClassifier, SessionProfile, Timeframe,
};
use gadarah_data::{
    aggregate_bars, import_csv, insert_bars, list_symbols, load_all_bars, load_bars, CsvFormat,
    Database,
};

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
        "backtest" => cmd_backtest(&args[2..]),
        "diagnose" => cmd_diagnose(&args[2..]),
        "validate" => cmd_validate(&args[2..]),
        "portfolio" => cmd_portfolio(&args[2..]),
        "synth" => cmd_synth(&args[2..]),
        "full" => cmd_full(&args[2..]),
        "live" => cmd_live(&args[2..]),
        _ => print_help(),
    }
}

fn print_help() {
    println!("GADARAH — Prop Trading Engine CLI");
    println!();
    println!("Usage: gadarah <command> [options]");
    println!();
    println!("Commands:");
    println!("  import       <csv_file> <symbol> <timeframe> [format]   Import bars from CSV");
    println!(
        "  bulk-import  <dir> [--db <path>]                        Import all CSVs from directory"
    );
    println!(
        "  aggregate    [--db <path>] [--symbol <sym>] <from> <to> Aggregate timeframes in DB"
    );
    println!(
        "  backtest     [--db <path>] [--symbol <sym>] [--balance <bal>]  Run backtest replay"
    );
    println!("  validate     [--db <path>] [--symbol <sym>]             Monte Carlo + challenge + stress");
    println!("  portfolio    [--db <path>] [--symbols <csv>] [--balance <bal>]  Portfolio replay + validation");
    println!("  synth        [--bars <n>] [--seed <s>] [--db <path>]    Generate synthetic data");
    println!(
        "  full         [--seed <s>] [--balance <bal>]             Synth + backtest + validate"
    );
    println!(
        "  live         [--feed binance] [--symbol <sym>] [--timeframe <tf>]  Run live trading"
    );
    println!();
    println!("Formats: mt (MetaTrader), ct (cTrader), unix (default)");
    println!();
    println!("Examples:");
    println!("  gadarah bulk-import /path/to/HYDRA/data/candles/");
    println!("  gadarah aggregate --symbol EURUSD M5 M15");
    println!("  gadarah backtest --db data/gadarah.db --symbol EURUSD");
    println!("  gadarah portfolio --symbols EURUSD --risk 0.74");
    println!("  gadarah import data/EURUSD_M15.csv EURUSD M15 unix");
    println!("  gadarah live --feed binance --symbol EURUSDT --timeframe M1");
}

#[derive(Debug, Clone)]
struct PortfolioOptions {
    db_path: String,
    symbols_str: String,
    balance: Decimal,
    risk_pct: Decimal,
    daily_dd_limit_pct: Decimal,
    max_dd_limit_pct: Decimal,
    momentum_enabled: bool,
    asian_enabled: bool,
    breakout_enabled: bool,
    momentum_first_hour_bars: u32,
    asian_sl_buffer_pips: Decimal,
    breakout_squeeze_pctile: Decimal,
    breakout_expansion_pctile: Decimal,
    from_ts: i64,
    to_ts: i64,
}

impl Default for PortfolioOptions {
    fn default() -> Self {
        Self {
            db_path: "data/gadarah.db".to_string(),
            symbols_str: "EURUSD".to_string(),
            balance: dec!(10000),
            risk_pct: dec!(0.74),
            daily_dd_limit_pct: dec!(1.75),
            max_dd_limit_pct: dec!(6.0),
            momentum_enabled: true,
            asian_enabled: true,
            breakout_enabled: true,
            momentum_first_hour_bars: 10,
            asian_sl_buffer_pips: dec!(5.0),
            breakout_squeeze_pctile: dec!(0.25),
            breakout_expansion_pctile: dec!(0.50),
            from_ts: 0,
            to_ts: i64::MAX,
        }
    }
}

// ---------------------------------------------------------------------------
// Import command
// ---------------------------------------------------------------------------

fn cmd_import(args: &[String]) {
    if args.len() < 3 {
        println!("Usage: gadarah import <csv_file> <symbol> <timeframe> [format]");
        println!("  format: mt | ct | unix (default: unix)");
        return;
    }

    let csv_path = &args[0];
    let symbol = &args[1];
    let tf_str = &args[2];
    let format_str = args.get(3).map(|s| s.as_str()).unwrap_or("unix");

    let tf = match tf_str.to_uppercase().as_str() {
        "M1" => Timeframe::M1,
        "M5" => Timeframe::M5,
        "M15" => Timeframe::M15,
        "H1" => Timeframe::H1,
        "H4" => Timeframe::H4,
        "D1" => Timeframe::D1,
        _ => {
            println!("Invalid timeframe: {tf_str}. Use: M1, M5, M15, H1, H4, D1");
            return;
        }
    };

    let format = match format_str {
        "mt" => CsvFormat::MetaTrader,
        "ct" => CsvFormat::CTrader,
        "unix" => CsvFormat::Unix,
        _ => {
            println!("Invalid format: {format_str}. Use: mt, ct, unix");
            return;
        }
    };

    let db_path = "data/gadarah.db";
    std::fs::create_dir_all("data").ok();
    let mut db = Database::open(db_path).expect("Failed to open database");

    let file = std::fs::File::open(csv_path).expect("Failed to open CSV file");
    let reader = std::io::BufReader::new(file);

    let start = Instant::now();
    let count =
        import_csv(db.conn_mut(), reader, symbol, tf, format).expect("Failed to import CSV");

    println!(
        "Imported {count} bars for {symbol} {tf_str} in {:.2}s",
        start.elapsed().as_secs_f64()
    );
    println!("Database: {db_path}");
}

// ---------------------------------------------------------------------------
// Bulk-import command: import all CSVs from a directory
// ---------------------------------------------------------------------------

fn cmd_bulk_import(args: &[String]) {
    if args.is_empty() {
        println!("Usage: gadarah bulk-import <directory> [--db <path>]");
        println!("  Scans for files named SYMBOL_TIMEFRAME.csv (e.g. EURUSD_M15.csv)");
        return;
    }

    let dir = &args[0];
    let mut db_path = "data/gadarah.db".to_string();

    let mut i = 1;
    while i < args.len() {
        if args[i] == "--db" {
            db_path = args.get(i + 1).cloned().unwrap_or(db_path);
            i += 2;
        } else {
            i += 1;
        }
    }

    std::fs::create_dir_all("data").ok();
    let mut db = Database::open(&db_path).expect("Failed to open database");

    let entries: Vec<_> = std::fs::read_dir(dir)
        .expect("Failed to read directory")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "csv"))
        .collect();

    if entries.is_empty() {
        println!("No CSV files found in {dir}");
        return;
    }

    let mut total_imported = 0usize;
    let start = Instant::now();

    for entry in &entries {
        let path = entry.path();
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");

        // Parse SYMBOL_TIMEFRAME from filename
        let parts: Vec<&str> = stem.rsplitn(2, '_').collect();
        if parts.len() != 2 {
            println!("  SKIP {}: can't parse SYMBOL_TIMEFRAME", path.display());
            continue;
        }
        let tf_str = parts[0];
        let symbol = parts[1];

        let tf = match tf_str.to_uppercase().as_str() {
            "M1" => Timeframe::M1,
            "M5" => Timeframe::M5,
            "M15" => Timeframe::M15,
            "H1" => Timeframe::H1,
            "H4" => Timeframe::H4,
            "D1" => Timeframe::D1,
            _ => {
                println!("  SKIP {}: unknown timeframe '{tf_str}'", path.display());
                continue;
            }
        };

        let file = std::fs::File::open(&path).expect("Failed to open CSV");
        let reader = std::io::BufReader::new(file);

        match import_csv(db.conn_mut(), reader, symbol, tf, CsvFormat::Unix) {
            Ok(count) => {
                println!("  {symbol} {tf_str}: {count} bars imported");
                total_imported += count;
            }
            Err(e) => {
                println!("  ERROR {}: {e}", path.display());
            }
        }
    }

    println!(
        "\nImported {total_imported} total bars from {} files in {:.2}s",
        entries.len(),
        start.elapsed().as_secs_f64()
    );
    println!("Database: {db_path}");
}

// ---------------------------------------------------------------------------
// Aggregate command: aggregate stored bars to a higher timeframe
// ---------------------------------------------------------------------------

fn cmd_aggregate(args: &[String]) {
    let mut db_path = "data/gadarah.db".to_string();
    let mut symbol: Option<String> = None;
    let mut from_tf: Option<Timeframe> = None;
    let mut to_tf: Option<Timeframe> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--db" => {
                db_path = args.get(i + 1).cloned().unwrap_or(db_path);
                i += 2;
            }
            "--symbol" => {
                symbol = args.get(i + 1).cloned();
                i += 2;
            }
            other => {
                // Positional: from_tf and to_tf
                if let Some(tf) = parse_timeframe(other) {
                    if from_tf.is_none() {
                        from_tf = Some(tf);
                    } else {
                        to_tf = Some(tf);
                    }
                }
                i += 1;
            }
        }
    }

    let (from_tf, to_tf) = match (from_tf, to_tf) {
        (Some(f), Some(t)) => (f, t),
        _ => {
            println!("Usage: gadarah aggregate [--db <path>] [--symbol <sym>] <from_tf> <to_tf>");
            println!("  Example: gadarah aggregate --symbol EURUSD M5 M15");
            return;
        }
    };

    let mut db = Database::open(&db_path).expect("Failed to open database");

    // If symbol specified, aggregate just that symbol; otherwise all symbols
    let symbols = if let Some(s) = symbol {
        vec![s]
    } else {
        list_symbols(db.conn()).unwrap_or_default()
    };

    let start = Instant::now();
    let mut total = 0usize;

    for sym in &symbols {
        let bars = load_all_bars(db.conn(), sym, from_tf).unwrap_or_default();
        if bars.is_empty() {
            continue;
        }

        match aggregate_bars(&bars, to_tf) {
            Ok(agg) => {
                let count = agg.len();
                insert_bars(db.conn_mut(), sym, &agg).expect("Failed to insert aggregated bars");
                println!(
                    "  {sym}: {count} {to_tf:?} bars from {} {from_tf:?} bars",
                    bars.len()
                );
                total += count;
            }
            Err(e) => {
                println!("  ERROR {sym}: {e}");
            }
        }
    }

    println!(
        "\nAggregated {total} bars in {:.2}s",
        start.elapsed().as_secs_f64()
    );
}

fn parse_timeframe(s: &str) -> Option<Timeframe> {
    match s.to_uppercase().as_str() {
        "M1" => Some(Timeframe::M1),
        "M5" => Some(Timeframe::M5),
        "M15" => Some(Timeframe::M15),
        "H1" => Some(Timeframe::H1),
        "H4" => Some(Timeframe::H4),
        "D1" => Some(Timeframe::D1),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Diagnose command: trace where signals are blocked
// ---------------------------------------------------------------------------

fn cmd_diagnose(args: &[String]) {
    let mut db_path = "data/gadarah.db".to_string();
    let mut symbol = "EURUSD".to_string();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--db" => {
                db_path = args.get(i + 1).cloned().unwrap_or(db_path);
                i += 2;
            }
            "--symbol" => {
                symbol = args.get(i + 1).cloned().unwrap_or(symbol);
                i += 2;
            }
            _ => {
                i += 1;
            }
        }
    }

    let db = Database::open(&db_path).expect("Failed to open database");
    let bars = load_all_bars(db.conn(), &symbol, Timeframe::M15).expect("Failed to load bars");

    if bars.is_empty() {
        println!("No M15 bars for {symbol}");
        return;
    }

    println!("Diagnosing {} M15 bars for {symbol}...\n", bars.len());

    let mut regime = RegimeClassifier::new();
    let mut heads = make_heads(&symbol);

    // Counters
    let mut warmup_bars = 0usize;
    let mut regime_counts: HashMap<String, usize> = HashMap::new();
    let mut head_regime_blocked: HashMap<String, usize> = HashMap::new();
    let mut head_signals_generated: HashMap<String, usize> = HashMap::new();
    let mut head_evaluated: HashMap<String, usize> = HashMap::new();
    let mut volume_zero_bars = 0usize;
    let mut total_signals = 0usize;

    for bar in &bars {
        if bar.volume == 0 {
            volume_zero_bars += 1;
        }

        let regime_signal = match regime.update(bar) {
            Some(rs) => rs,
            None => {
                warmup_bars += 1;
                continue;
            }
        };

        let regime_name = format!("{:?}", regime_signal.regime);
        *regime_counts.entry(regime_name).or_default() += 1;

        let session_profile = SessionProfile::from_utc_hour(utc_hour(bar.timestamp));

        for head in heads.iter_mut() {
            let head_name = format!("{:?}", head.id());

            if !head.regime_allowed(&regime_signal) {
                *head_regime_blocked.entry(head_name.clone()).or_default() += 1;
                let _ = head.evaluate(bar, &session_profile, &regime_signal);
                continue;
            }

            *head_evaluated.entry(head_name.clone()).or_default() += 1;
            let signals = head.evaluate(bar, &session_profile, &regime_signal);

            if !signals.is_empty() {
                *head_signals_generated.entry(head_name).or_default() += signals.len();
                total_signals += signals.len();
                for s in &signals {
                    println!(
                        "  SIGNAL: {:?} {:?} @ {:.5} SL={:.5} TP={:.5} R:R={} [{}]",
                        s.head,
                        s.direction,
                        s.entry,
                        s.stop_loss,
                        s.take_profit,
                        s.rr_ratio()
                            .map(|r| format!("{:.2}", r))
                            .unwrap_or("N/A".into()),
                        s.comment
                    );
                }
            }
        }
    }

    println!("\n{}", "=".repeat(60));
    println!("DIAGNOSIS RESULTS");
    println!("{}", "=".repeat(60));

    println!("\nData:");
    println!("  Total bars:         {}", bars.len());
    println!("  Warmup bars:        {warmup_bars}");
    println!("  Bars after warmup:  {}", bars.len() - warmup_bars);
    println!(
        "  Zero-volume bars:   {volume_zero_bars} ({:.1}%)",
        volume_zero_bars as f64 / bars.len() as f64 * 100.0
    );

    println!("\nRegime distribution:");
    let mut regimes: Vec<_> = regime_counts.iter().collect();
    regimes.sort_by(|a, b| b.1.cmp(a.1));
    for (name, count) in &regimes {
        let pct = **count as f64 / (bars.len() - warmup_bars) as f64 * 100.0;
        let allowed = match name.as_str() {
            "StrongTrendUp" | "StrongTrendDown" => "Momentum, Breakout",
            "WeakTrendUp" | "WeakTrendDown" => "Momentum",
            "RangingTight" => "AsianRange",
            "RangingWide" => "AsianRange, Breakout",
            "BreakoutPending" => "Breakout",
            "Choppy" | "Transitioning" => "(none)",
            _ => "?",
        };
        println!(
            "  {:<20} {:>5} ({:>5.1}%)  allows: {}",
            name, count, pct, allowed
        );
    }

    println!("\nPer-head analysis:");
    for head_name in &["Momentum", "AsianRange", "Breakout"] {
        let hn = head_name.to_string();
        let blocked = head_regime_blocked.get(&hn).copied().unwrap_or(0);
        let evaluated = head_evaluated.get(&hn).copied().unwrap_or(0);
        let signals = head_signals_generated.get(&hn).copied().unwrap_or(0);
        println!("  {head_name}:");
        println!("    Regime blocked:    {blocked}");
        println!("    Regime allowed:    {evaluated}");
        println!("    Signals generated: {signals}");
    }

    // Breakout-specific deep analysis: trace squeeze/expansion independently
    let mut bo_regime = RegimeClassifier::new();
    let mut bo_bb = BollingerBands::new(20, dec!(2.0));
    let mut bo_pctile = BBWidthPercentile::new(100);
    let mut bo_squeeze: u32 = 0;
    let mut max_squeeze: u32 = 0;
    let mut squeeze_events = 0usize;
    let mut expansion_while_squeezed = 0usize;
    let mut close_outside_bb = 0usize;

    for bar in &bars {
        let _ = bo_regime.update(bar);

        if let Some(bb) = bo_bb.update(bar.close) {
            let pctile = bo_pctile.update(bb.width);

            if pctile < dec!(0.30) {
                bo_squeeze += 1;
                if bo_squeeze > max_squeeze {
                    max_squeeze = bo_squeeze;
                }
            }

            if bo_squeeze >= 10 && pctile > dec!(0.50) {
                expansion_while_squeezed += 1;
                let outside = bar.close > bb.upper || bar.close < bb.lower;
                if outside {
                    close_outside_bb += 1;
                }
                let dir_str = if bar.close > bb.upper {
                    "BUY"
                } else if bar.close < bb.lower {
                    "SELL"
                } else {
                    "inside"
                };
                let sl_dist = if bar.close > bb.upper {
                    bar.close - bb.lower
                } else {
                    bb.upper - bar.close
                };
                let _risk_rr = if !sl_dist.is_zero() {
                    dec!(2) * dec!(0.001) / sl_dist
                } else {
                    dec!(0)
                };
                println!("  BO candidate: ts={} pctile={:.3} squeeze={} dir={} close={:.5} upper={:.5} lower={:.5} width={:.5}",
                    bar.timestamp, pctile, bo_squeeze, dir_str, bar.close, bb.upper, bb.lower, bb.width);
            }

            if pctile >= dec!(0.30) {
                if bo_squeeze >= 10 {
                    squeeze_events += 1;
                }
                bo_squeeze = 0;
            }
        }
    }

    println!("\nBreakout deep analysis:");
    println!("  Max consecutive squeeze bars: {max_squeeze}");
    println!("  Squeeze events (10+ bars):    {squeeze_events}");
    println!("  Expansion bars while squeezed: {expansion_while_squeezed}");
    println!("  Close outside BB on expansion: {close_outside_bb}");

    println!("\nTotal signals: {total_signals}");
    if total_signals == 0 {
        println!("\nPOSSIBLE ISSUES:");
        if volume_zero_bars > bars.len() / 2 {
            println!(
                "  - {:.0}% zero-volume bars → BreakoutHead volume filter will never pass",
                volume_zero_bars as f64 / bars.len() as f64 * 100.0
            );
        }
        let choppy = regime_counts.get("Choppy").copied().unwrap_or(0);
        let transitioning = regime_counts.get("Transitioning").copied().unwrap_or(0);
        let no_head_pct =
            (choppy + transitioning) as f64 / (bars.len() - warmup_bars).max(1) as f64 * 100.0;
        if no_head_pct > 50.0 {
            println!(
                "  - {:.0}% of bars in Choppy/Transitioning (no heads allowed)",
                no_head_pct
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Synth command
// ---------------------------------------------------------------------------

fn cmd_synth(args: &[String]) {
    let mut num_bars = 50_000usize;
    let mut seed = 42u64;
    let mut db_path = "data/gadarah.db".to_string();
    let symbol = "EURUSD";

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--bars" => {
                num_bars = args
                    .get(i + 1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(num_bars);
                i += 2;
            }
            "--seed" => {
                seed = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(seed);
                i += 2;
            }
            "--db" => {
                db_path = args.get(i + 1).cloned().unwrap_or(db_path);
                i += 2;
            }
            _ => {
                i += 1;
            }
        }
    }

    println!("Generating {num_bars} synthetic M15 bars for {symbol} (seed={seed})...");
    let start = Instant::now();

    let bars = if num_bars >= 40_000 {
        synth::generate_2y_bars(symbol, dec!(1.10000), seed)
    } else {
        synth::generate_bars(symbol, num_bars, dec!(1.10000), 1672531200, seed)
    };

    println!(
        "Generated {} bars in {:.2}s",
        bars.len(),
        start.elapsed().as_secs_f64()
    );
    println!(
        "Price range: {:.5} - {:.5}",
        bars.iter().map(|b| b.low).min().unwrap_or_default(),
        bars.iter().map(|b| b.high).max().unwrap_or_default(),
    );

    // Store to DB
    std::fs::create_dir_all("data").ok();
    let mut db = Database::open(&db_path).expect("Failed to open database");
    let count = insert_bars(db.conn_mut(), symbol, &bars).expect("Failed to insert bars");
    println!("Stored {count} bars to {db_path}");
}

// ---------------------------------------------------------------------------
// Backtest command
// ---------------------------------------------------------------------------

fn cmd_backtest(args: &[String]) {
    let mut db_path = "data/gadarah.db".to_string();
    let mut symbol = "EURUSD".to_string();
    let mut balance = dec!(10000);

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--db" => {
                db_path = args.get(i + 1).cloned().unwrap_or(db_path);
                i += 2;
            }
            "--symbol" => {
                symbol = args.get(i + 1).cloned().unwrap_or(symbol);
                i += 2;
            }
            "--balance" => {
                balance = args
                    .get(i + 1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(balance);
                i += 2;
            }
            _ => {
                i += 1;
            }
        }
    }

    let db = Database::open(&db_path).expect("Failed to open database");
    let bars = load_all_bars(db.conn(), &symbol, Timeframe::M15).expect("Failed to load bars");

    if bars.is_empty() {
        println!("No M15 bars found for {symbol} in {db_path}");
        println!("Run: gadarah synth    (to generate synthetic data)");
        println!("  or: gadarah import  (to import CSV data)");
        return;
    }

    println!("Loaded {} M15 bars for {symbol}", bars.len());

    let config = make_replay_config(&symbol, balance);
    let mut heads = make_heads(&symbol);

    let start = Instant::now();
    let result = run_replay(&bars, &mut heads, &config).expect("Replay failed");
    let elapsed = start.elapsed();

    print_stats_report(&result.stats, elapsed.as_secs_f64());
    println!("\nEquity curve: {} data points", result.equity_curve.len());

    // Per-head breakdown
    print_head_breakdown(&result.trades);
}

// ---------------------------------------------------------------------------
// Validate command
// ---------------------------------------------------------------------------

fn cmd_validate(args: &[String]) {
    let mut db_path = "data/gadarah.db".to_string();
    let mut symbol = "EURUSD".to_string();
    let mut balance = dec!(10000);

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--db" => {
                db_path = args.get(i + 1).cloned().unwrap_or(db_path);
                i += 2;
            }
            "--symbol" => {
                symbol = args.get(i + 1).cloned().unwrap_or(symbol);
                i += 2;
            }
            "--balance" => {
                balance = args
                    .get(i + 1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(balance);
                i += 2;
            }
            _ => {
                i += 1;
            }
        }
    }

    let db = Database::open(&db_path).expect("Failed to open database");
    let bars = load_all_bars(db.conn(), &symbol, Timeframe::M15).expect("Failed to load bars");

    if bars.is_empty() {
        println!("No bars found. Run: gadarah synth");
        return;
    }

    let config = make_replay_config(&symbol, balance);
    let mut heads = make_heads(&symbol);

    println!("Running backtest on {} bars...", bars.len());
    let result = run_replay(&bars, &mut heads, &config).expect("Replay failed");

    println!("\n{}", "=".repeat(60));
    println!("BACKTEST RESULTS");
    println!("{}", "=".repeat(60));
    print_stats_report(&result.stats, 0.0);

    // Monte Carlo
    println!("\n{}", "=".repeat(60));
    println!("MONTE CARLO SIMULATION (10,000 paths)");
    println!("{}", "=".repeat(60));

    let mc = run_monte_carlo(
        &result.trades,
        balance,
        &MonteCarloConfig {
            num_paths: 10_000,
            ruin_dd_pct: dec!(6.0),
        },
        42,
    );

    println!("Paths:              {}", mc.paths_run);
    println!(
        "Ruin probability:   {:.2}% ({} / {})",
        mc.ruin_probability * dec!(100),
        mc.ruin_count,
        mc.paths_run
    );
    println!("Median final bal:   ${:.2}", mc.median_final_balance);
    println!("P5  final bal:      ${:.2}", mc.p5_final_balance);
    println!("P25 final bal:      ${:.2}", mc.p25_final_balance);
    println!("P75 final bal:      ${:.2}", mc.p75_final_balance);
    println!("P95 final bal:      ${:.2}", mc.p95_final_balance);
    println!("Worst DD:           {:.2}%", mc.worst_drawdown_pct);
    println!("Median DD:          {:.2}%", mc.median_drawdown_pct);

    let mc_pass = mc.ruin_probability < dec!(0.05);
    println!(
        "MC VERDICT:         {}",
        if mc_pass {
            "PASS (ruin < 5%)"
        } else {
            "FAIL (ruin >= 5%)"
        }
    );

    // Challenge Simulation
    println!("\n{}", "=".repeat(60));
    println!("CHALLENGE SIMULATION");
    println!("{}", "=".repeat(60));

    let rules = vec![
        ChallengeRules::ftmo_1step(),
        ChallengeRules::brightfunded_evaluation(),
        ChallengeRules::two_step_pro(),
    ];

    let sim_results = simulate_challenges(&result.trades, balance, &rules);

    for sr in &sim_results {
        print_challenge_result(sr);
    }

    // Stress Test
    println!("\n{}", "=".repeat(60));
    println!("STRESS TEST (1.5x losses, -10% win rate, +$2 slippage)");
    println!("{}", "=".repeat(60));

    let stress = run_stress_test(
        &result.trades,
        balance,
        &StressConfig::default(),
        Some(&ChallengeRules::ftmo_1step()),
    );

    println!(
        "Original PnL:       ${:.2}",
        stress.original_stats.total_pnl
    );
    println!(
        "Stressed PnL:       ${:.2}",
        stress.stressed_stats.total_pnl
    );
    println!(
        "Original win rate:  {:.1}%",
        stress.original_stats.win_rate * dec!(100)
    );
    println!(
        "Stressed win rate:  {:.1}%",
        stress.stressed_stats.win_rate * dec!(100)
    );
    println!(
        "Original PF:        {:.2}",
        stress.original_stats.profit_factor
    );
    println!(
        "Stressed PF:        {:.2}",
        stress.stressed_stats.profit_factor
    );
    println!(
        "Original max DD:    {:.2}%",
        stress.original_stats.max_drawdown_pct
    );
    println!(
        "Stressed max DD:    {:.2}%",
        stress.stressed_stats.max_drawdown_pct
    );

    if let Some(ref cr) = stress.challenge_result {
        println!(
            "Stress FTMO:        {}",
            if cr.passed { "PASS" } else { "FAIL" }
        );
    }

    // Overall verdict
    println!("\n{}", "=".repeat(60));
    println!("OVERALL VALIDATION");
    println!("{}", "=".repeat(60));

    let backtest_pass = result.stats.profit_factor > dec!(1.0) && result.stats.total_trades > 0;
    let all_pass = backtest_pass && mc_pass;

    println!(
        "Backtest PF > 1.0:  {}",
        if backtest_pass { "PASS" } else { "FAIL" }
    );
    println!(
        "Monte Carlo < 5%:   {}",
        if mc_pass { "PASS" } else { "FAIL" }
    );
    println!(
        "OVERALL:            {}",
        if all_pass {
            "READY FOR DEMO"
        } else {
            "NEEDS TUNING"
        }
    );
}

// ---------------------------------------------------------------------------
// Portfolio command — multi-symbol backtest + validation
// ---------------------------------------------------------------------------

fn cmd_portfolio(args: &[String]) {
    let mut opts = PortfolioOptions::default();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--db" => {
                opts.db_path = args
                    .get(i + 1)
                    .cloned()
                    .unwrap_or_else(|| opts.db_path.clone());
                i += 2;
            }
            "--symbols" => {
                opts.symbols_str = args
                    .get(i + 1)
                    .cloned()
                    .unwrap_or_else(|| opts.symbols_str.clone());
                i += 2;
            }
            "--balance" => {
                opts.balance = args
                    .get(i + 1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(opts.balance);
                i += 2;
            }
            "--risk" => {
                opts.risk_pct = args
                    .get(i + 1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(opts.risk_pct);
                i += 2;
            }
            "--daily-dd" => {
                opts.daily_dd_limit_pct = args
                    .get(i + 1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(opts.daily_dd_limit_pct);
                i += 2;
            }
            "--max-dd" => {
                opts.max_dd_limit_pct = args
                    .get(i + 1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(opts.max_dd_limit_pct);
                i += 2;
            }
            "--momentum-bars" => {
                opts.momentum_first_hour_bars = args
                    .get(i + 1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(opts.momentum_first_hour_bars);
                i += 2;
            }
            "--asian-buffer" => {
                opts.asian_sl_buffer_pips = args
                    .get(i + 1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(opts.asian_sl_buffer_pips);
                i += 2;
            }
            "--breakout-squeeze" => {
                opts.breakout_squeeze_pctile = args
                    .get(i + 1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(opts.breakout_squeeze_pctile);
                i += 2;
            }
            "--breakout-expansion" => {
                opts.breakout_expansion_pctile = args
                    .get(i + 1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(opts.breakout_expansion_pctile);
                i += 2;
            }
            "--from-ts" => {
                opts.from_ts = args
                    .get(i + 1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(opts.from_ts);
                i += 2;
            }
            "--to-ts" => {
                opts.to_ts = args
                    .get(i + 1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(opts.to_ts);
                i += 2;
            }
            "--disable-momentum" => {
                opts.momentum_enabled = false;
                i += 1;
            }
            "--disable-asian" => {
                opts.asian_enabled = false;
                i += 1;
            }
            "--disable-breakout" => {
                opts.breakout_enabled = false;
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }

    let symbols: Vec<&str> = opts.symbols_str.split(',').collect();
    let db = Database::open(&opts.db_path).expect("Failed to open database");

    println!("{}", "=".repeat(60));
    println!("PORTFOLIO BACKTEST — {} symbols", symbols.len());
    println!("Symbols: {}", opts.symbols_str);
    println!("Balance: ${:.2}", opts.balance);
    println!(
        "Risk / DD: {}% / {}% daily / {}% total",
        opts.risk_pct, opts.daily_dd_limit_pct, opts.max_dd_limit_pct
    );
    println!(
        "Heads: momentum={} asian={} breakout={}",
        opts.momentum_enabled, opts.asian_enabled, opts.breakout_enabled
    );
    println!(
        "Params: momentum_bars={} asian_buffer={} squeeze={} expansion={}",
        opts.momentum_first_hour_bars,
        opts.asian_sl_buffer_pips,
        opts.breakout_squeeze_pctile,
        opts.breakout_expansion_pctile
    );
    if opts.from_ts > 0 || opts.to_ts < i64::MAX {
        println!("Window: {}..{}", opts.from_ts, opts.to_ts);
    }
    println!("{}", "=".repeat(60));

    let mut all_trades: Vec<gadarah_backtest::TradeResult> = Vec::new();
    let mut total_bars = 0usize;
    let mut combined_head_map: HashMap<String, (usize, usize, Decimal)> = HashMap::new();

    for &sym in &symbols {
        let bars = if opts.from_ts == 0 && opts.to_ts == i64::MAX {
            load_all_bars(db.conn(), sym, Timeframe::M15)
        } else {
            load_bars(db.conn(), sym, Timeframe::M15, opts.from_ts, opts.to_ts)
        }
        .expect("Failed to load bars");
        if bars.is_empty() {
            println!("\n{}: No M15 data, skipping", sym);
            continue;
        }
        total_bars += bars.len();

        let config = make_replay_config_with_options(sym, opts.balance, &opts);
        let mut heads = make_heads_with_options(sym, &opts);
        let result = run_replay(&bars, &mut heads, &config).expect("Replay failed");

        println!(
            "\n--- {} ({} bars, {} trades) ---",
            sym,
            bars.len(),
            result.stats.total_trades
        );
        println!(
            "  PnL: ${:.2}  WR: {:.1}%  PF: {:.2}  MaxDD: {:.2}%",
            result.stats.total_pnl,
            result.stats.win_rate * dec!(100),
            result.stats.profit_factor,
            result.stats.max_drawdown_pct
        );

        // Per-head breakdown inline
        let mut head_map: HashMap<String, (usize, usize, Decimal)> = HashMap::new();
        for t in &result.trades {
            let key = format!("{:?}", t.head);
            let entry = head_map.entry(key).or_insert((0, 0, Decimal::ZERO));
            entry.0 += 1;
            if t.is_winner {
                entry.1 += 1;
            }
            entry.2 += t.pnl;

            let combined_entry =
                combined_head_map
                    .entry(format!("{:?}", t.head))
                    .or_insert((0, 0, Decimal::ZERO));
            combined_entry.0 += 1;
            if t.is_winner {
                combined_entry.1 += 1;
            }
            combined_entry.2 += t.pnl;
        }
        let mut head_rows: Vec<_> = head_map.into_iter().collect();
        head_rows.sort_by(|a, b| a.0.cmp(&b.0));
        for (head, (total, wins, pnl)) in &head_rows {
            let wr = if *total > 0 {
                *wins as f64 / *total as f64 * 100.0
            } else {
                0.0
            };
            println!(
                "    {:<12} {} trades, {:.0}% WR, ${:.2}",
                head, total, wr, pnl
            );
        }
        println!(
            "SUMMARY_SYMBOL|symbol={}|trades={}|win_rate_pct={:.2}|pnl={:.2}|profit_factor={:.2}|max_dd_pct={:.2}",
            sym,
            result.stats.total_trades,
            result.stats.win_rate * dec!(100),
            result.stats.total_pnl,
            result.stats.profit_factor,
            result.stats.max_drawdown_pct
        );
        for (head, (total, wins, pnl)) in &head_rows {
            let wr = if *total > 0 {
                Decimal::from(*wins * 100) / Decimal::from(*total)
            } else {
                Decimal::ZERO
            };
            println!(
                "SUMMARY_HEAD|scope=symbol|symbol={}|head={}|trades={}|win_rate_pct={:.2}|pnl={:.2}",
                sym, head, total, wr, pnl
            );
        }

        all_trades.extend(result.trades);
    }

    // Sort all trades by close time for proper sequential ordering
    all_trades.sort_by_key(|t| t.closed_at);

    println!("\n{}", "=".repeat(60));
    println!("COMBINED PORTFOLIO RESULTS");
    println!("{}", "=".repeat(60));

    let total = all_trades.len();
    let winners = all_trades.iter().filter(|t| t.is_winner).count();
    let losers = total - winners;
    let total_pnl: Decimal = all_trades.iter().map(|t| t.pnl).sum();
    let wr = if total > 0 {
        Decimal::from(winners * 100) / Decimal::from(total)
    } else {
        Decimal::ZERO
    };
    let gross_wins: Decimal = all_trades
        .iter()
        .filter(|t| t.is_winner)
        .map(|t| t.pnl)
        .sum();
    let gross_losses: Decimal = all_trades
        .iter()
        .filter(|t| !t.is_winner)
        .map(|t| t.pnl.abs())
        .sum();
    let pf = if gross_losses > Decimal::ZERO {
        gross_wins / gross_losses
    } else {
        Decimal::ZERO
    };

    // Compute portfolio drawdown from sequential trade PnLs
    let mut equity = opts.balance;
    let mut peak = equity;
    let mut max_dd = Decimal::ZERO;
    let mut max_dd_pct = Decimal::ZERO;
    for t in &all_trades {
        equity += t.pnl;
        if equity > peak {
            peak = equity;
        }
        let dd = peak - equity;
        if dd > max_dd {
            max_dd = dd;
        }
        let dd_pct = if peak > Decimal::ZERO {
            dd * dec!(100) / peak
        } else {
            Decimal::ZERO
        };
        if dd_pct > max_dd_pct {
            max_dd_pct = dd_pct;
        }
    }

    println!("Total trades:       {}", total);
    println!("Winners / Losers:   {} / {}", winners, losers);
    println!("Win rate:           {:.1}%", wr);
    println!("Total PnL:          ${:.2}", total_pnl);
    println!(
        "Return:             {:.2}%",
        total_pnl * dec!(100) / opts.balance
    );
    println!("Profit factor:      {:.2}", pf);
    println!("Max DD:             {:.2}% (${:.2})", max_dd_pct, max_dd);
    println!("Ending balance:     ${:.2}", equity);
    println!("Total bars:         {}", total_bars);
    let mut combined_head_rows: Vec<_> = combined_head_map.into_iter().collect();
    combined_head_rows.sort_by(|a, b| a.0.cmp(&b.0));
    if !combined_head_rows.is_empty() {
        println!("\nCombined head breakdown:");
        for (head, (trades, wins, pnl)) in &combined_head_rows {
            let wr = if *trades > 0 {
                Decimal::from(*wins * 100) / Decimal::from(*trades)
            } else {
                Decimal::ZERO
            };
            println!(
                "  {:<12} {} trades, {:.1}% WR, ${:.2}",
                head, trades, wr, pnl
            );
        }
    }

    if all_trades.is_empty() {
        println!("\nNo trades to validate.");
        return;
    }

    // Monte Carlo
    println!("\n{}", "=".repeat(60));
    println!("MONTE CARLO SIMULATION (10,000 paths)");
    println!("{}", "=".repeat(60));

    let mc = run_monte_carlo(
        &all_trades,
        opts.balance,
        &MonteCarloConfig {
            num_paths: 10_000,
            ruin_dd_pct: opts.max_dd_limit_pct,
        },
        42,
    );

    println!(
        "Ruin probability:   {:.2}%",
        mc.ruin_probability * dec!(100)
    );
    println!("Median final bal:   ${:.2}", mc.median_final_balance);
    println!(
        "P5 / P95:           ${:.2} / ${:.2}",
        mc.p5_final_balance, mc.p95_final_balance
    );
    println!("Worst DD:           {:.2}%", mc.worst_drawdown_pct);
    println!("Median DD:          {:.2}%", mc.median_drawdown_pct);

    let mc_pass = mc.ruin_probability < dec!(0.05);
    println!(
        "MC VERDICT:         {}",
        if mc_pass {
            "PASS (ruin < 5%)"
        } else {
            "FAIL (ruin >= 5%)"
        }
    );

    // Challenge Simulation
    println!("\n{}", "=".repeat(60));
    println!("CHALLENGE SIMULATION");
    println!("{}", "=".repeat(60));

    let rules = vec![
        ChallengeRules::ftmo_1step(),
        ChallengeRules::brightfunded_evaluation(),
        ChallengeRules::two_step_pro(),
    ];

    let sim_results = simulate_challenges(&all_trades, opts.balance, &rules);

    for sr in &sim_results {
        print_challenge_result(sr);
    }

    // Stress Test
    println!("\n{}", "=".repeat(60));
    println!("STRESS TEST (1.5x losses, -10% WR, +$2 slippage)");
    println!("{}", "=".repeat(60));

    let stress = run_stress_test(
        &all_trades,
        opts.balance,
        &StressConfig::default(),
        Some(&ChallengeRules::ftmo_1step()),
    );

    println!(
        "Original PnL:       ${:.2}",
        stress.original_stats.total_pnl
    );
    println!(
        "Stressed PnL:       ${:.2}",
        stress.stressed_stats.total_pnl
    );
    println!(
        "Original WR:        {:.1}%",
        stress.original_stats.win_rate * dec!(100)
    );
    println!(
        "Stressed WR:        {:.1}%",
        stress.stressed_stats.win_rate * dec!(100)
    );
    println!(
        "Original PF:        {:.2}",
        stress.original_stats.profit_factor
    );
    println!(
        "Stressed PF:        {:.2}",
        stress.stressed_stats.profit_factor
    );
    println!(
        "Original DD:        {:.2}%",
        stress.original_stats.max_drawdown_pct
    );
    println!(
        "Stressed DD:        {:.2}%",
        stress.stressed_stats.max_drawdown_pct
    );

    if let Some(ref cr) = stress.challenge_result {
        println!(
            "Stress FTMO:        {}",
            if cr.passed { "PASS" } else { "FAIL" }
        );
    }

    // Overall verdict
    println!("\n{}", "=".repeat(60));
    let backtest_pass = pf > dec!(1.0) && total >= 10;
    let all_pass = backtest_pass && mc_pass;
    println!(
        "Backtest PF > 1.0:  {}",
        if backtest_pass { "PASS" } else { "FAIL" }
    );
    println!(
        "Monte Carlo < 5%:   {}",
        if mc_pass { "PASS" } else { "FAIL" }
    );
    println!(
        "OVERALL:            {}",
        if all_pass {
            "READY FOR DEMO"
        } else {
            "NEEDS TUNING"
        }
    );
    println!("{}", "=".repeat(60));

    println!(
        "SUMMARY_RUN|symbols={}|risk_pct={}|trades={}|win_rate_pct={:.2}|return_pct={:.2}|profit_factor={:.2}|max_dd_pct={:.2}|mc_ruin_pct={:.2}|verdict={}",
        opts.symbols_str,
        opts.risk_pct,
        total,
        wr,
        total_pnl * dec!(100) / opts.balance,
        pf,
        max_dd_pct,
        mc.ruin_probability * dec!(100),
        if all_pass { "READY_FOR_DEMO" } else { "NEEDS_TUNING" }
    );
    for sr in &sim_results {
        println!(
            "SUMMARY_CHALLENGE|name={}|passed={}|profit_pct={:.2}|max_daily_dd_pct={:.2}|max_total_dd_pct={:.2}|trading_days={}",
            sr.rules.name.replace(' ', "_"),
            sr.passed,
            sr.profit_pct,
            sr.max_daily_dd_pct,
            sr.max_total_dd_pct,
            sr.trading_days
        );
    }
    for (head, (trades, wins, pnl)) in &combined_head_rows {
        let wr = if *trades > 0 {
            Decimal::from(*wins * 100) / Decimal::from(*trades)
        } else {
            Decimal::ZERO
        };
        println!(
            "SUMMARY_HEAD|scope=combined|head={}|trades={}|win_rate_pct={:.2}|pnl={:.2}",
            head, trades, wr, pnl
        );
    }
}

// ---------------------------------------------------------------------------
// Full command (synth + backtest + validate)
// ---------------------------------------------------------------------------

fn cmd_full(args: &[String]) {
    let mut seed = 42u64;
    let mut balance = dec!(10000);

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--seed" => {
                seed = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(seed);
                i += 2;
            }
            "--balance" => {
                balance = args
                    .get(i + 1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(balance);
                i += 2;
            }
            _ => {
                i += 1;
            }
        }
    }

    println!("{}", "=".repeat(60));
    println!("GADARAH FULL PIPELINE");
    println!("{}", "=".repeat(60));

    // Step 1: Generate synthetic data
    println!("\n[1/3] Generating synthetic EURUSD M15 data (seed={seed})...");
    let bars = synth::generate_2y_bars("EURUSD", dec!(1.10000), seed);
    println!("  Generated {} bars", bars.len());
    println!(
        "  Price range: {:.5} - {:.5}",
        bars.iter().map(|b| b.low).min().unwrap_or_default(),
        bars.iter().map(|b| b.high).max().unwrap_or_default(),
    );

    // Step 2: Run backtest
    println!("\n[2/3] Running backtest...");
    let config = make_replay_config("EURUSD", balance);
    let mut heads = make_heads("EURUSD");

    let start = Instant::now();
    let result = run_replay(&bars, &mut heads, &config).expect("Replay failed");
    let elapsed = start.elapsed();

    print_stats_report(&result.stats, elapsed.as_secs_f64());
    print_head_breakdown(&result.trades);

    if result.trades.is_empty() {
        println!("\nNo trades generated — heads may need tuning for this data.");
        println!("This is expected with synthetic data that doesn't perfectly");
        println!("match the heads' entry conditions.");
        return;
    }

    // Step 3: Validate
    println!("\n[3/3] Validation suite...");

    // Monte Carlo
    println!("\n--- Monte Carlo (10,000 paths, 6% ruin threshold) ---");
    let mc = run_monte_carlo(
        &result.trades,
        balance,
        &MonteCarloConfig {
            num_paths: 10_000,
            ruin_dd_pct: dec!(6.0),
        },
        seed,
    );
    println!(
        "  Ruin probability: {:.2}%",
        mc.ruin_probability * dec!(100)
    );
    println!("  Median balance:   ${:.2}", mc.median_final_balance);
    println!(
        "  P5 / P95:         ${:.2} / ${:.2}",
        mc.p5_final_balance, mc.p95_final_balance
    );
    println!(
        "  Worst/Median DD:  {:.2}% / {:.2}%",
        mc.worst_drawdown_pct, mc.median_drawdown_pct
    );

    // Challenge sim
    println!("\n--- Challenge Simulation ---");
    let rules = vec![
        ChallengeRules::ftmo_1step(),
        ChallengeRules::brightfunded_evaluation(),
    ];
    let sims = simulate_challenges(&result.trades, balance, &rules);
    for sr in &sims {
        let status = if sr.passed { "PASS" } else { "FAIL" };
        println!(
            "  {:<25} {} ({})",
            sr.rules.name,
            status,
            format_challenge_summary(sr)
        );
    }

    // Stress test
    println!("\n--- Stress Test ---");
    let stress = run_stress_test(&result.trades, balance, &StressConfig::default(), None);
    println!(
        "  PnL degradation:  ${:.2} → ${:.2}",
        stress.original_stats.total_pnl, stress.stressed_stats.total_pnl
    );
    println!(
        "  PF degradation:   {:.2} → {:.2}",
        stress.original_stats.profit_factor, stress.stressed_stats.profit_factor
    );
    println!(
        "  WR degradation:   {:.1}% → {:.1}%",
        stress.original_stats.win_rate * dec!(100),
        stress.stressed_stats.win_rate * dec!(100)
    );

    // Verdict
    let pass = result.stats.profit_factor > dec!(1.0)
        && mc.ruin_probability < dec!(0.05)
        && result.stats.total_trades >= 10;

    println!("\n{}", "=".repeat(60));
    println!(
        "VERDICT: {}",
        if pass {
            "SYSTEM VIABLE — Ready for demo forward test"
        } else {
            "NEEDS TUNING — Review head parameters"
        }
    );
    println!("{}", "=".repeat(60));
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn is_jpy_pair(symbol: &str) -> bool {
    symbol.ends_with("JPY")
}

fn pip_size_for(symbol: &str) -> Decimal {
    if is_jpy_pair(symbol) {
        dec!(0.01)
    } else {
        dec!(0.0001)
    }
}

fn pip_value_for(symbol: &str) -> Decimal {
    // Per standard lot (100k units):
    // JPY pairs: ~$6.50-7.50 per pip depending on rate, use $7
    // Most USD-quoted pairs: $10 per pip
    // Cross pairs (EURGBP etc): varies, ~$10-13, use $10 as approximation
    if is_jpy_pair(symbol) {
        dec!(7.0)
    } else {
        dec!(10.0)
    }
}

fn make_heads(symbol: &str) -> Vec<Box<dyn Head>> {
    let opts = PortfolioOptions::default();
    make_heads_with_options(symbol, &opts)
}

fn make_heads_with_options(symbol: &str, opts: &PortfolioOptions) -> Vec<Box<dyn Head>> {
    let ps = pip_size_for(symbol);
    let mut heads: Vec<Box<dyn Head>> = Vec::new();

    if opts.momentum_enabled {
        heads.push(Box::new(MomentumHead::new(MomentumConfig {
            symbol: symbol.to_string(),
            pip_size: ps,
            first_hour_bars: opts.momentum_first_hour_bars,
            ..MomentumConfig::default()
        })));
    }

    if opts.asian_enabled {
        heads.push(Box::new(AsianRangeHead::new(
            AsianRangeConfig {
                symbol: symbol.to_string(),
                sl_buffer_pips: opts.asian_sl_buffer_pips,
                ..AsianRangeConfig::default()
            },
            ps,
        )));
    }

    if opts.breakout_enabled {
        heads.push(Box::new(BreakoutHead::new(BreakoutConfig {
            symbol: symbol.to_string(),
            squeeze_pctile: opts.breakout_squeeze_pctile,
            expansion_pctile: opts.breakout_expansion_pctile,
            ..BreakoutConfig::default()
        })));
    }

    heads
}

fn make_replay_config(symbol: &str, balance: Decimal) -> ReplayConfig {
    let opts = PortfolioOptions::default();
    make_replay_config_with_options(symbol, balance, &opts)
}

fn make_replay_config_with_options(
    symbol: &str,
    balance: Decimal,
    opts: &PortfolioOptions,
) -> ReplayConfig {
    ReplayConfig {
        symbol: symbol.to_string(),
        pip_size: pip_size_for(symbol),
        pip_value_per_lot: pip_value_for(symbol),
        starting_balance: balance,
        risk_pct: opts.risk_pct,
        daily_dd_limit_pct: opts.daily_dd_limit_pct,
        max_dd_limit_pct: opts.max_dd_limit_pct,
        max_positions: 3,
        min_rr: dec!(1.0),
        max_spread_pips: dec!(3.0),
        mock_config: MockConfig {
            slippage_pips: dec!(0.5),
            commission_per_lot: dec!(3.50),
            spread_pips: dec!(1.5),
        },
        consecutive_loss_halt: 5,
    }
}

fn print_challenge_result(result: &ChallengeSimResult) {
    println!("\n--- {} ---", result.rules.name);

    for stage in &result.stage_results {
        println!("  [{}]", stage.stage_rules.name);
        println!(
            "    Target:     {:.1}% → {}",
            stage.stage_rules.target_pct,
            if stage.target_reached {
                format!("REACHED ({:.2}%)", stage.profit_pct)
            } else {
                format!("NOT REACHED ({:.2}%)", stage.profit_pct)
            }
        );
        println!(
            "    Daily DD:   {:.2}% / {:.1}% limit → {}",
            stage.max_daily_dd_pct,
            stage.stage_rules.daily_dd_limit_pct,
            if stage.daily_dd_breached {
                "BREACHED"
            } else {
                "OK"
            }
        );
        println!(
            "    Total DD:   {:.2}% / {:.1}% limit → {}",
            stage.max_total_dd_pct,
            stage.stage_rules.max_dd_limit_pct,
            if stage.max_dd_breached {
                "BREACHED"
            } else {
                "OK"
            }
        );
        println!(
            "    Min days:   {} / {} → {}",
            stage.trading_days,
            stage.stage_rules.min_trading_days,
            if stage.min_days_met { "OK" } else { "NOT MET" }
        );
        if stage.stage_rules.consistency_cap_pct > Decimal::ZERO {
            println!(
                "    Consistency: {} cap → {}",
                stage.stage_rules.consistency_cap_pct,
                if stage.consistency_met { "OK" } else { "FAIL" }
            );
        }
        if let Some(days) = stage.days_to_target {
            println!("    Days to target: {}", days);
        }
        if let Some(reason) = &stage.breach_reason {
            println!("    Breach: {}", reason);
        }
        println!(
            "    Stage verdict: {}",
            if stage.passed { "PASS" } else { "FAIL" }
        );
    }

    println!(
        "  VERDICT:    {} ({}/{})",
        if result.passed { "PASS" } else { "FAIL" },
        result.completed_stages,
        result.rules.stages.len()
    );
}

fn format_challenge_summary(result: &ChallengeSimResult) -> String {
    if result.passed {
        return format!(
            "{} stages cleared in {} days",
            result.completed_stages,
            result.days_to_target.unwrap_or(0)
        );
    }

    result
        .breach_reason
        .clone()
        .unwrap_or_else(|| "requirements not met".into())
}

fn print_stats_report(stats: &BacktestStats, elapsed_secs: f64) {
    println!("\n--- Backtest Statistics ---");
    println!("Total trades:       {}", stats.total_trades);
    println!("Winners / Losers:   {} / {}", stats.winners, stats.losers);
    println!("Win rate:           {:.1}%", stats.win_rate * dec!(100));
    println!("Total PnL:          ${:.2}", stats.total_pnl);
    println!("Return:             {:.2}%", stats.return_pct);
    println!("Avg winner:         ${:.2}", stats.avg_winner);
    println!("Avg loser:          ${:.2}", stats.avg_loser);
    println!("Avg R-multiple:     {:.2}R", stats.avg_r_multiple);
    println!(
        "Max R / Min R:      {:.2}R / {:.2}R",
        stats.max_r, stats.min_r
    );
    println!("Profit factor:      {:.2}", stats.profit_factor);
    println!("Expectancy:         {:.2}R", stats.expectancy_r);
    println!("Sharpe ratio:       {:.2}", stats.sharpe_ratio);
    println!(
        "Max DD:             {:.2}% (${:.2})",
        stats.max_drawdown_pct, stats.max_drawdown_usd
    );
    println!("Max con. wins:      {}", stats.max_consecutive_wins);
    println!("Max con. losses:    {}", stats.max_consecutive_losses);
    println!("Trading days:       {}", stats.trading_days);
    println!("Starting balance:   ${:.2}", stats.starting_balance);
    println!("Ending balance:     ${:.2}", stats.ending_balance);
    if elapsed_secs > 0.0 {
        println!("Replay time:        {:.2}s", elapsed_secs);
    }
}

fn print_head_breakdown(trades: &[gadarah_backtest::TradeResult]) {
    use gadarah_core::HeadId;
    use std::collections::HashMap;

    let mut by_head: HashMap<HeadId, (usize, usize, Decimal)> = HashMap::new();
    for t in trades {
        let entry = by_head.entry(t.head).or_insert((0, 0, Decimal::ZERO));
        entry.0 += 1;
        if t.is_winner {
            entry.1 += 1;
        }
        entry.2 += t.pnl;
    }

    if by_head.is_empty() {
        return;
    }

    println!("\n--- Per-Head Breakdown ---");
    println!("{:<15} {:>6} {:>8} {:>10}", "Head", "Trades", "Win%", "PnL");
    println!("{}", "-".repeat(42));

    let mut heads: Vec<_> = by_head.into_iter().collect();
    heads.sort_by_key(|(h, _)| format!("{:?}", h));

    for (head, (total, wins, pnl)) in heads {
        let wr = if total > 0 {
            Decimal::from(wins) / Decimal::from(total) * dec!(100)
        } else {
            Decimal::ZERO
        };
        println!(
            "{:<15} {:>6} {:>7.1}% ${:>9.2}",
            format!("{:?}", head),
            total,
            wr,
            pnl
        );
    }
}

// ---------------------------------------------------------------------------
// Live trading command
// --------------------------------------------------------------------------

fn cmd_live(args: &[String]) {
    let mut feed_type = "binance".to_string();
    let mut symbol = "EURUSDT".to_string();
    let mut timeframe = "M1".to_string();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--feed" => {
                feed_type = args.get(i + 1).cloned().unwrap_or(feed_type);
                i += 2;
            }
            "--symbol" => {
                symbol = args.get(i + 1).cloned().unwrap_or(symbol);
                i += 2;
            }
            "--timeframe" => {
                timeframe = args.get(i + 1).cloned().unwrap_or(timeframe);
                i += 2;
            }
            _ => {
                i += 1;
            }
        }
    }

    let tf = match timeframe.to_uppercase().as_str() {
        "M1" => Timeframe::M1,
        "M5" => Timeframe::M5,
        "M15" => Timeframe::M15,
        "H1" => Timeframe::H1,
        "H4" => Timeframe::H4,
        "D1" => Timeframe::D1,
        _ => {
            println!("Invalid timeframe: {}. Use M1, M5, M15, H1, H4, D1", timeframe);
            return;
        }
    };

    println!("{}", "=".repeat(60));
    println!("GADARAH LIVE TRADING");
    println!("{}", "=".repeat(60));
    println!("Feed:        {}", feed_type);
    println!("Symbol:      {}", symbol);
    println!("Timeframe:   {}", timeframe);
    println!("{}", "=".repeat(60));

    // Create the feed
    let feed: Box<dyn Feed> = match feed_type.as_str() {
        "binance" => {
            match BinanceFeed::new(vec![symbol.clone()], tf) {
                Ok(f) => Box::new(f),
                Err(e) => {
                    println!("Failed to create Binance feed: {}", e);
                    return;
                }
            }
        }
        _ => {
            println!("Unknown feed type: {}. Use 'binance'", feed_type);
            return;
        }
    };

    println!("Connecting to {}...", feed.name());

    // Subscribe to the feed (this creates the async task in a new runtime)
    let mut receiver = feed.subscribe();

    // Create a bar streamer
    let mut streamer = BarStreamer::new(tf);

    println!("Waiting for market data... (Ctrl+C to exit)");
    println!("{}", "=".repeat(60));

    // Process incoming messages (blocking receive)
    let mut bars_received: usize = 0;
    let start = std::time::Instant::now();

    loop {
        // Use blocking recv since we're not in an async context
        match receiver.blocking_recv() {
            Some(FeedMessage::Tick(tick)) => {
                if let Some(bar) = streamer.process_tick(&tick) {
                    bars_received += 1;
                    println!(
                        "[{}] BAR #{} | {} | O:{:.5} H:{:.5} L:{:.5} C:{:.5} V:{}",
                        start.elapsed().as_secs(),
                        bars_received,
                        bar.timestamp,
                        bar.open, bar.high, bar.low, bar.close, bar.volume
                    );
                }
            }
            Some(FeedMessage::Connected) => {
                println!("✓ Connected to {}", feed.name());
            }
            Some(FeedMessage::Disconnected) => {
                println!("✗ Disconnected from {}", feed.name());
            }
            Some(FeedMessage::Error(e)) => {
                println!("ERROR: {}", e);
            }
            Some(FeedMessage::Heartbeat) => {
                // Keep-alive
            }
            Some(FeedMessage::Bar(bar)) => {
                bars_received += 1;
                println!(
                    "[{}] BAR #{} | {} | O:{:.5} H:{:.5} L:{:.5} C:{:.5} V:{}",
                    start.elapsed().as_secs(),
                    bars_received,
                    bar.timestamp,
                    bar.open, bar.high, bar.low, bar.close, bar.volume
                );
            }
            None => {
                println!("Feed channel closed");
                break;
            }
        }
    }
}

async fn run_live_feed(feed: Box<dyn Feed>, tf: Timeframe) {
    // Subscribe to the feed
    let mut receiver = feed.subscribe();

    // Create a bar streamer
    let mut streamer = BarStreamer::new(tf);

    println!("Waiting for market data... (Ctrl+C to exit)");
    println!("{}", "=".repeat(60));

    // Process incoming messages
    let mut bars_received: usize = 0;
    let start = std::time::Instant::now();

    loop {
        match receiver.recv().await {
            Some(FeedMessage::Tick(tick)) => {
                if let Some(bar) = streamer.process_tick(&tick) {
                    bars_received += 1;
                    println!(
                        "[{}] BAR #{} | {} | O:{:.5} H:{:.5} L:{:.5} C:{:.5} V:{}",
                        start.elapsed().as_secs(),
                        bars_received,
                        bar.timestamp,
                        bar.open, bar.high, bar.low, bar.close, bar.volume
                    );
                }
            }
            Some(FeedMessage::Connected) => {
                println!("✓ Connected to {}", feed.name());
            }
            Some(FeedMessage::Disconnected) => {
                println!("✗ Disconnected from {}", feed.name());
            }
            Some(FeedMessage::Error(e)) => {
                println!("ERROR: {}", e);
            }
            Some(FeedMessage::Heartbeat) => {
                // Keep-alive
            }
            Some(FeedMessage::Bar(bar)) => {
                bars_received += 1;
                println!(
                    "[{}] BAR #{} | {} | O:{:.5} H:{:.5} L:{:.5} C:{:.5} V:{}",
                    start.elapsed().as_secs(),
                    bars_received,
                    bar.timestamp,
                    bar.open, bar.high, bar.low, bar.close, bar.volume
                );
            }
            None => {
                println!("Feed channel closed");
                break;
            }
        }
    }
}
