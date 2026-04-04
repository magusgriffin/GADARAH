use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};
use std::io::Write as IoWrite;

use rusqlite::params;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tracing::{info, warn};
use serde_json;

use gadarah_backtest::{
    run_engine, run_monte_carlo, run_stress_test, run_walk_forward_engine, simulate_challenge,
    simulate_challenge_batch, ChallengeBatchResult, ChallengeRules, ChallengeSimResult,
    EngineConfig as BacktestEngineConfig, MonteCarloConfig, StressConfig, WalkForwardConfig,
};
use gadarah_broker::{
    Broker, BrokerAccountInfo, CloseRequest, CtraderClient, CtraderConfig, MockConfig, OrderRequest,
    OrderType,
};
use gadarah_core::{
    heads::{
        asian_range::{AsianRangeConfig, AsianRangeHead},
        breakout::{BreakoutConfig, BreakoutHead},
        momentum::{MomentumConfig, MomentumHead},
    },
    utc_hour, Bar, Head, HeadId, Regime9, RegimeClassifier, RegimeSignal9, Session, SessionProfile,
    SignalKind, Timeframe, TradeSignal,
};
use gadarah_data::{
    audit_bars, build_dataset_readiness_report, close_trade, insert_equity_snapshot, insert_trade,
    load_all_bars, load_unclosed_trade_count, DataAuditResult, Database, DatasetRequirements,
    EquitySnapshot, TradeClose, TradeRecord,
};
use gadarah_feed::{BarStreamer, Tick as FeedTick};
use gadarah_risk::{
    calculate_lots, can_add_pyramid, create_pyramid_layer, ComplianceBlackoutWindow,
    ComplianceOpenExposure, DayState, ExecutionConfig as RiskExecutionConfig,
    ExecutionEngine as RiskExecutionEngine, FillRecord, FundingPipsComplianceManager,
    PyramidAddCandidate, PyramidConfig, PyramidState, RiskPercent, SizingInputs,
};

use crate::config::{load_config, load_firm_config, FirmConfigFile, GadarahConfig};

const DEFAULT_CONFIG_PATH: &str = "config/gadarah.toml";
const DEFAULT_FIRM_PATH: &str = "config/firms/the5ers_hypergrowth.toml";
const MIN_SIGNAL_CONFIDENCE: Decimal = dec!(0.55);
const PHASE1_MIN_HISTORY_DAYS: i64 = 730;

#[derive(Debug, Clone)]
struct Phase1Context {
    config: GadarahConfig,
    firm_file: FirmConfigFile,
    config_path: String,
    firm_path: String,
    db_path: String,
    /// Primary symbol (first configured or `--symbol` override).
    symbol: String,
    /// All configured symbols for multi-symbol backtest/validate.
    symbols: Vec<String>,
    balance: Decimal,
    compliance_blackout_windows: Vec<ComplianceBlackoutWindow>,
}

#[derive(Debug, Clone)]
struct LivePositionState {
    position_id: u64,
    trade_id: Option<i64>,
    signal: TradeSignal,
    lots: Decimal,
    risk_pct: Decimal,
    opened_at: i64,
    pyramid: Option<PyramidState>,
}

struct LiveRuntime {
    execution: RiskExecutionEngine,
    compliance: FundingPipsComplianceManager,
    /// Set to true when a DD-based halt condition is detected. Notified via Discord once.
    kill_switch_active: bool,
    account_id: i64,
    positions_by_head: HashMap<HeadId, LivePositionState>,
    pyramid_config: PyramidConfig,
    pyramid_enabled: bool,
}

pub fn run_backtest(args: &[String]) {
    let ctx = match load_context(args) {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("{err}");
            return;
        }
    };

    let symbols = &ctx.symbols;
    let positions_per_symbol =
        (ctx.firm_file.firm.max_positions as usize + symbols.len() - 1) / symbols.len();

    println!("{}", "=".repeat(60));
    println!("PHASE 1 BACKTEST");
    println!("{}", "=".repeat(60));
    println!("Config:      {}", ctx.config_path);
    println!("Firm:        {}", ctx.firm_path);
    print_fundingpips_compliance_status(&ctx);
    println!(
        "Symbols:     {} (max {} pos/symbol)",
        symbols.join(", "),
        positions_per_symbol
    );
    println!("Balance:     ${:.2}", ctx.balance);

    let start = Instant::now();
    let mut all_trades: Vec<gadarah_backtest::TradeResult> = Vec::new();
    let mut symbols_run = 0usize;

    for symbol in symbols {
        let bars = match load_bars_for(&ctx.db_path, symbol, Timeframe::M15) {
            Ok(bars) if !bars.is_empty() => bars,
            Ok(_) => {
                eprintln!("  {} — no bars, skipping", symbol);
                continue;
            }
            Err(err) => {
                eprintln!("  {} — {}, skipping", symbol, err);
                continue;
            }
        };

        let engine_config = build_engine_config_for(&ctx, symbol, positions_per_symbol);
        let mut heads = make_phase1_heads(symbol);

        let result = match run_engine(&bars, &mut heads, &engine_config) {
            Ok(result) => result,
            Err(err) => {
                eprintln!("  {} — backtest failed: {err}", symbol);
                continue;
            }
        };

        symbols_run += 1;
        println!("\n--- {} ({} bars) ---", symbol, bars.len());
        print_stats_report(&result.stats, 0.0);
        println!(
            "Signals:      {} generated / {} rejected",
            result.signals_generated, result.signals_rejected
        );
        println!(
            "Blocked bars: regime={} kill={} drift={} phase={} dd={} daily={} protect={} consistency={} spread={}",
            result.diagnostics.bars_without_regime,
            result.diagnostics.blocked_kill_switch,
            result.diagnostics.blocked_drift_halt,
            result.diagnostics.blocked_phase,
            result.diagnostics.blocked_dd_distance,
            result.diagnostics.blocked_daily_stop,
            result.diagnostics.blocked_temporal_protect,
            result.diagnostics.blocked_consistency,
            result.diagnostics.blocked_spread
        );
        print_engine_diagnostics(&result.diagnostics);
        print_head_breakdown(&result.trades);

        match persist_engine_run_for(&ctx, symbol, "backtest", &result) {
            Ok((account_id, trades, snapshots)) => println!(
                "Persisted:   account={} trades={} snapshots={}",
                account_id, trades, snapshots
            ),
            Err(err) => eprintln!("Persistence failed: {err}"),
        }

        all_trades.extend(result.trades);
    }

    if symbols_run == 0 {
        eprintln!("No symbols had data. Nothing to report.");
        return;
    }

    // Combined report (only meaningful when > 1 symbol ran)
    if symbols_run > 1 {
        all_trades.sort_by_key(|t| t.closed_at);
        let combined = gadarah_backtest::BacktestStats::compute(&all_trades, ctx.balance);
        println!("\n{}", "=".repeat(60));
        println!("COMBINED ({} symbols)", symbols_run);
        println!("{}", "=".repeat(60));
        print_stats_report(&combined, start.elapsed().as_secs_f64());
        print_head_breakdown(&all_trades);
    } else {
        println!("\nReplay time: {:.2}s", start.elapsed().as_secs_f64());
    }
}

pub fn run_audit_data(args: &[String]) {
    let ctx = match load_context(args) {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("{err}");
            return;
        }
    };

    let timeframe = match arg_value(args, "--timeframe") {
        Some(raw) => match parse_timeframe(&raw) {
            Ok(tf) => tf,
            Err(err) => {
                eprintln!("{err}");
                return;
            }
        },
        None => Timeframe::M15,
    };

    let bars = match load_phase1_bars(&ctx, timeframe) {
        Ok(bars) => bars,
        Err(err) => {
            eprintln!("{err}");
            return;
        }
    };
    let audit = audit_bars(&bars, timeframe);
    let min_history_days = parse_i64_arg(args, "--min-history-days", PHASE1_MIN_HISTORY_DAYS);
    let readiness = match load_series_readiness(&ctx, timeframe, min_history_days) {
        Ok(report) => report,
        Err(err) => {
            eprintln!("{err}");
            return;
        }
    };

    println!("{}", "=".repeat(60));
    println!("DATA AUDIT");
    println!("{}", "=".repeat(60));
    println!("Config:       {}", ctx.config_path);
    println!("DB:           {}", ctx.db_path);
    println!("Symbol:       {}", ctx.symbol);
    println!("Timeframe:    {:?}", timeframe);
    print_data_audit(&audit);
    print_single_series_readiness(&readiness);
    print_fundingpips_compliance_status(&ctx);
    println!(
        "Verdict:      {}",
        if audit.passed() && readiness.history_passed {
            "PASS"
        } else {
            "FAIL"
        }
    );
}

pub fn run_validate(args: &[String]) {
    let ctx = match load_context(args) {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("{err}");
            return;
        }
    };

    // Gate 0: data audit on primary symbol
    let bars = match load_phase1_bars(&ctx, Timeframe::M15) {
        Ok(bars) => bars,
        Err(err) => {
            eprintln!("{err}");
            return;
        }
    };
    let audit = audit_bars(&bars, Timeframe::M15);
    let readiness = match load_series_readiness(&ctx, Timeframe::M15, PHASE1_MIN_HISTORY_DAYS) {
        Ok(report) => report,
        Err(err) => {
            eprintln!("{err}");
            return;
        }
    };
    let data_gate_pass = audit.passed() && readiness.history_passed;

    println!("\n{}", "=".repeat(60));
    println!("DATA AUDIT ({})", ctx.symbol);
    println!("{}", "=".repeat(60));
    print_fundingpips_compliance_status(&ctx);
    print_data_audit(&audit);
    print_single_series_readiness(&readiness);
    println!(
        "Gate 0 verdict:     {}",
        if data_gate_pass { "PASS" } else { "FAIL" }
    );

    // Run engine per symbol and merge trades
    let symbols = &ctx.symbols;
    let positions_per_symbol =
        (ctx.firm_file.firm.max_positions as usize + symbols.len() - 1) / symbols.len();

    let mut all_trades: Vec<gadarah_backtest::TradeResult> = Vec::new();
    let mut primary_result: Option<gadarah_backtest::EngineResult> = None;
    let mut primary_bars: Option<Vec<Bar>> = None;

    for symbol in symbols {
        let sym_bars = match load_bars_for(&ctx.db_path, symbol, Timeframe::M15) {
            Ok(b) if !b.is_empty() => b,
            Ok(_) => {
                eprintln!("  {} — no bars, skipping", symbol);
                continue;
            }
            Err(err) => {
                eprintln!("  {} — {}, skipping", symbol, err);
                continue;
            }
        };

        let engine_config = build_engine_config_for(&ctx, symbol, positions_per_symbol);
        let mut heads = make_phase1_heads(symbol);
        let result = match run_engine(&sym_bars, &mut heads, &engine_config) {
            Ok(r) => r,
            Err(err) => {
                eprintln!("  {} — backtest failed: {err}", symbol);
                continue;
            }
        };

        println!("\n--- {} ({} bars, {} trades) ---", symbol, sym_bars.len(), result.stats.total_trades);
        print_stats_report(&result.stats, 0.0);
        print_engine_diagnostics(&result.diagnostics);

        match persist_engine_run_for(&ctx, symbol, "validate", &result) {
            Ok((account_id, trades, snapshots)) => println!(
                "Persisted:          account={} trades={} snapshots={}",
                account_id, trades, snapshots
            ),
            Err(err) => eprintln!("Persistence failed: {err}"),
        }

        all_trades.extend(result.trades.clone());

        // Keep primary symbol result and bars for walk-forward
        if *symbol == ctx.symbol {
            primary_bars = Some(sym_bars);
            primary_result = Some(result);
        }
    }

    all_trades.sort_by_key(|t| t.closed_at);
    let combined = gadarah_backtest::BacktestStats::compute(&all_trades, ctx.balance);

    let profitable_day_rate = profitable_day_rate(&all_trades);
    let gate1_pass = combined.return_pct >= dec!(8.0)
        && combined.max_drawdown_pct < dec!(5.0)
        && combined.win_rate >= dec!(0.45)
        && combined.profit_factor >= dec!(1.30)
        && combined.total_trades >= 150
        && profitable_day_rate >= dec!(0.55);

    println!("\n{}", "=".repeat(60));
    println!(
        "COMBINED BACKTEST ({} symbols, {} trades)",
        symbols.len(),
        combined.total_trades
    );
    println!("{}", "=".repeat(60));
    print_stats_report(&combined, 0.0);
    println!(
        "Profitable days:    {:.1}%",
        profitable_day_rate * dec!(100)
    );
    print_head_breakdown(&all_trades);
    println!(
        "Gate 1 verdict:     {}",
        if gate1_pass { "PASS" } else { "FAIL" }
    );

    // Walk-forward on primary symbol only (multi-symbol WF is future work)
    let (wf_bars, wf_engine_config) = match (&primary_bars, primary_result.as_ref()) {
        (Some(b), Some(_)) => (
            b.as_slice(),
            build_engine_config_for(&ctx, &ctx.symbol, positions_per_symbol),
        ),
        _ => {
            eprintln!("Primary symbol {} had no data; cannot run walk-forward", ctx.symbol);
            return;
        }
    };

    println!("\n{}", "=".repeat(60));
    println!("WALK-FORWARD (5 FOLDS, {} only)", ctx.symbol);
    println!("{}", "=".repeat(60));
    let walk_forward = match run_walk_forward_engine(
        wf_bars,
        || make_phase1_heads(&ctx.symbol),
        &wf_engine_config,
        &WalkForwardConfig::default(),
    ) {
        Ok(result) => result,
        Err(err) => {
            eprintln!("Walk-forward failed: {err}");
            return;
        }
    };
    for fold in &walk_forward.folds {
        println!(
            "Fold {}: OOS PF {:.2} | OOS Max DD {:.2}% | trades {}",
            fold.fold_index + 1,
            fold.out_of_sample_result.stats.profit_factor,
            fold.out_of_sample_result.stats.max_drawdown_pct,
            fold.out_of_sample_result.stats.total_trades
        );
    }
    println!(
        "Combined OOS PF:     {:.2}",
        walk_forward.combined_oos_stats.profit_factor
    );
    println!(
        "OOS degradation:     {:.1}%",
        walk_forward.oos_degradation_pct
    );
    println!(
        "Gate 2 verdict:      {}",
        if walk_forward.passed { "PASS" } else { "FAIL" }
    );

    // Monte Carlo, Challenge Sim, Stress on merged trades
    println!("\n{}", "=".repeat(60));
    println!("MONTE CARLO (10,000 BOOTSTRAP PATHS)");
    println!("{}", "=".repeat(60));
    let mc = run_monte_carlo(
        &all_trades,
        ctx.balance,
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
    println!("P5 final balance:   ${:.2}", mc.p5_final_balance);
    println!("Median final bal:   ${:.2}", mc.median_final_balance);
    println!("P95 final balance:  ${:.2}", mc.p95_final_balance);
    println!("Median DD:          {:.2}%", mc.median_drawdown_pct);
    println!("P95 DD:             {:.2}%", mc.p95_drawdown_pct);
    println!("Worst DD:           {:.2}%", mc.worst_drawdown_pct);
    let gate3_pass = mc.p5_final_balance >= ctx.balance
        && mc.p95_drawdown_pct < dec!(5.5)
        && mc.ruin_probability < dec!(0.05);
    println!(
        "Gate 3 verdict:      {}",
        if gate3_pass { "PASS" } else { "FAIL" }
    );

    let selected_rules = challenge_rules_for(&ctx.firm_file);
    let challenge_result = simulate_challenge(&all_trades, ctx.balance, &selected_rules);
    let batch_result =
        simulate_challenge_batch(&all_trades, ctx.balance, &selected_rules, 100, 42);
    println!("\n{}", "=".repeat(60));
    println!("CHALLENGE SIMULATION — {}", selected_rules.name);
    println!("{}", "=".repeat(60));
    print_challenge_result(&challenge_result);
    print_challenge_batch(&batch_result);
    let gate4_pass = batch_result.pass_rate >= dec!(0.65)
        && batch_result.dd_breach_rate <= dec!(0.05)
        && batch_result
            .avg_days_to_pass
            .map(|days| days <= dec!(25))
            .unwrap_or(challenge_result.passed);
    println!(
        "Gate 4 verdict:      {}",
        if gate4_pass { "PASS" } else { "FAIL" }
    );

    println!("\n{}", "=".repeat(60));
    println!("STRESS TEST");
    println!("{}", "=".repeat(60));
    let stress = run_stress_test(
        &all_trades,
        ctx.balance,
        &StressConfig::default(),
        Some(&selected_rules),
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
    println!(
        "Stressed total PnL: ${:.2}",
        stress.stressed_stats.total_pnl
    );

    println!("\n{}", "=".repeat(60));
    println!("OVERALL VALIDATION");
    println!("{}", "=".repeat(60));
    println!("Gate 0 Data Audit:   {}", verdict(data_gate_pass));
    println!("Gate 1 Backtest:     {}", verdict(gate1_pass));
    println!("Gate 2 Walk-forward: {}", verdict(walk_forward.passed));
    println!("Gate 3 Monte Carlo:  {}", verdict(gate3_pass));
    println!("Gate 4 Challenge:    {}", verdict(gate4_pass));

    let ready_for_demo =
        data_gate_pass && gate1_pass && walk_forward.passed && gate3_pass && gate4_pass;
    println!(
        "OVERALL:             {}",
        if ready_for_demo {
            "READY FOR DEMO"
        } else {
            "NOT READY FOR DEMO"
        }
    );
}

pub fn run_benchmarks(args: &[String]) {
    let ctx = match load_context(args) {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("{err}");
            return;
        }
    };

    let bars = match load_phase1_bars(&ctx, Timeframe::M15) {
        Ok(bars) => bars,
        Err(err) => {
            eprintln!("{err}");
            return;
        }
    };

    println!(
        "Running Phase 1 engine on {} bars to derive DriftBenchmarks...",
        bars.len()
    );
    let engine_config = build_engine_config(&ctx);
    let mut heads = make_phase1_heads(&ctx.symbol);
    let result = match run_engine(&bars, &mut heads, &engine_config) {
        Ok(result) => result,
        Err(err) => {
            eprintln!("Benchmark derivation failed: {err}");
            return;
        }
    };
    let stats = &result.stats;
    let max_consec = stats.max_consecutive_losses.min(255) as u8;

    println!();
    println!("{}", "=".repeat(60));
    println!("DRIFT BENCHMARKS");
    println!("{}", "=".repeat(60));
    print_fundingpips_compliance_status(&ctx);
    println!("[drift_benchmarks]");
    println!(
        "# Derived from Phase 1 engine on {} ({})",
        ctx.symbol, ctx.firm_file.firm.name
    );
    println!("expected_win_rate      = {:.4}", stats.win_rate);
    println!("expected_avg_r         = {:.4}", stats.avg_r_multiple);
    println!("expected_profit_factor = {:.4}", stats.profit_factor);
    println!("max_consecutive_losses = {}", max_consec);
    println!("# Populate after demo-forward");
    println!("expected_avg_slippage  = 0.0");
    println!("{}", "=".repeat(60));
}

pub fn run_live(args: &[String]) {
    let ctx = match load_context(args) {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("{err}");
            return;
        }
    };

    let timeframe = match arg_value(args, "--timeframe") {
        Some(raw) => match parse_timeframe(&raw) {
            Ok(tf) => tf,
            Err(err) => {
                eprintln!("{err}");
                return;
            }
        },
        None => Timeframe::M15,
    };
    let warmup_bars = parse_usize_arg(args, "--warmup-bars", 300);
    let poll_ms = parse_u64_arg(args, "--poll-ms", 1000);
    let execute = has_flag(args, "--execute");
    let live_server = has_flag(args, "--live");

    let mut client = match build_ctrader_client(&ctx, live_server) {
        Ok(client) => client,
        Err(err) => {
            eprintln!("{err}");
            return;
        }
    };

    println!("{}", "=".repeat(60));
    println!("PHASE 1 LIVE LOOP");
    println!("{}", "=".repeat(60));
    println!("Config:       {}", ctx.config_path);
    println!("Firm:         {}", ctx.firm_file.firm.name);
    print_fundingpips_compliance_status(&ctx);
    println!("Symbol:       {}", ctx.symbol);
    println!("Timeframe:    {:?}", timeframe);
    println!(
        "Mode:         {}",
        if live_server { "LIVE" } else { "DEMO" }
    );
    println!(
        "Execution:    {}",
        if execute { "ENABLED" } else { "DRY RUN" }
    );
    println!("Warmup bars:  {}", warmup_bars);
    println!("Poll:         {} ms", poll_ms);

    if let Err(err) = client.connect_blocking() {
        eprintln!("cTrader connect failed: {err}");
        return;
    }
    if let Err(err) = client.reconcile_blocking() {
        warn!("Initial reconcile failed: {err}");
    }
    let account_id = match client.account_info() {
        Ok(info) => info.account_id,
        Err(err) => {
            eprintln!("Unable to fetch broker account info: {err}");
            return;
        }
    };
    let mut db = match Database::open(&ctx.db_path) {
        Ok(db) => db,
        Err(err) => {
            eprintln!("Failed to open database {}: {}", ctx.db_path, err);
            return;
        }
    };

    let history = match load_phase1_bars(&ctx, timeframe) {
        Ok(bars) => bars,
        Err(err) => {
            eprintln!("{err}");
            return;
        }
    };
    if history.is_empty() {
        eprintln!("No historical bars available to warm the live engine.");
        return;
    }

    let mut heads = make_phase1_heads(&ctx.symbol);
    let mut regime = RegimeClassifier::new();
    let warmup_slice = if history.len() > warmup_bars {
        &history[history.len() - warmup_bars..]
    } else {
        &history[..]
    };
    let mut last_bar_ts = warmup_slice.last().map_or(0, |bar| bar.timestamp);
    for bar in warmup_slice {
        let session_profile = SessionProfile::from_utc_hour(utc_hour(bar.timestamp));
        if let Some(regime_signal) = regime.update(bar) {
            for head in heads.iter_mut() {
                let _ = head.evaluate(bar, &session_profile, &regime_signal);
            }
        } else {
            let warmup_regime = synthetic_transition_regime(bar.timestamp);
            for head in heads.iter_mut() {
                let _ = head.evaluate(bar, &session_profile, &warmup_regime);
            }
        }
    }

    let mut streamer = BarStreamer::new(timeframe);
    let mut compliance =
        FundingPipsComplianceManager::for_firm(&ctx.firm_file.firm.to_firm_config());
    compliance.set_blackout_windows(ctx.compliance_blackout_windows.clone());
    let mut runtime = LiveRuntime {
        execution: build_execution_engine(&ctx),
        compliance,
        kill_switch_active: false,
        account_id,
        positions_by_head: HashMap::new(),
        pyramid_config: ctx.config.pyramid_config(),
        pyramid_enabled: ctx.config.pyramid.enabled,
    };

    // Orphan detection: cross-check DB unclosed trades against broker open positions.
    if let Ok(reconcile) = client.reconcile_blocking() {
        let unclosed = load_unclosed_trade_count(db.conn(), account_id)
            .unwrap_or(0);
        if unclosed > 0 && unclosed != reconcile.open_position_count {
            warn!(
                "ORPHAN DETECT: DB has {} unclosed trades but broker reports {} open positions. \
                 Manual review may be needed.",
                unclosed, reconcile.open_position_count
            );
            crate::notify_discord(&format!(
                "⚠️ GADARAH orphan detect: DB has {unclosed} unclosed trades, \
                 broker has {} open. Check positions.",
                reconcile.open_position_count
            ));
        }
    }

    crate::notify_discord(&format!(
        "🚀 GADARAH live started: {} {} {}",
        ctx.firm_file.firm.name,
        ctx.symbol,
        if live_server { "LIVE" } else { "DEMO" }
    ));

    let gui_state_path = arg_value(args, "--gui-state-file");
    let mut bars_since_account_refresh: usize = 0;
    println!("Warmup complete. Listening for closed bars...");

    loop {
        match client.get_tick(&ctx.symbol) {
            Ok(tick) => {
                runtime
                    .execution
                    .update_spread(tick.spread_pips(pip_size_for(&ctx.symbol)), tick.timestamp);
                let feed_tick = FeedTick {
                    symbol: tick.symbol,
                    bid: tick.bid,
                    ask: tick.ask,
                    volume: 0,
                    timestamp: tick.timestamp,
                };

                if let Some(bar) = streamer.process_tick(&feed_tick) {
                    if bar.timestamp <= last_bar_ts {
                        thread::sleep(Duration::from_millis(poll_ms));
                        continue;
                    }
                    last_bar_ts = bar.timestamp;
                    bars_since_account_refresh += 1;

                    if execute {
                        reconcile_open_positions(&mut client, &mut runtime.positions_by_head);

                        // Phase I: Periodic account balance refresh every 10 bars.
                        if bars_since_account_refresh >= 10 {
                            bars_since_account_refresh = 0;
                            // account_info is called inline in process_live_bar; refreshing
                            // here ensures the snapshot stays current even if no signals fire.
                            if let Ok(info) = client.account_info() {
                                info!(
                                    "Account refresh: balance={} equity={} margin={}",
                                    info.balance, info.equity, info.free_margin
                                );
                            }
                        }
                    }

                    process_live_bar(
                        &ctx,
                        &mut db,
                        &mut client,
                        &mut regime,
                        &mut heads,
                        &mut runtime,
                        &bar,
                        execute,
                        timeframe,
                        gui_state_path.as_deref(),
                    );
                }
            }
            Err(err) => warn!("Tick unavailable: {err}"),
        }

        thread::sleep(Duration::from_millis(poll_ms));
    }
}

fn process_live_bar(
    ctx: &Phase1Context,
    db: &mut Database,
    client: &mut CtraderClient,
    regime: &mut RegimeClassifier,
    heads: &mut [Box<dyn Head>],
    runtime: &mut LiveRuntime,
    bar: &Bar,
    execute: bool,
    timeframe: Timeframe,
    gui_state_path: Option<&str>,
) {
    let session_profile = SessionProfile::from_utc_hour(utc_hour(bar.timestamp));
    let Some(regime_signal) = regime.update(bar) else {
        let warmup_regime = synthetic_transition_regime(bar.timestamp);
        for head in heads.iter_mut() {
            let _ = head.evaluate(bar, &session_profile, &warmup_regime);
        }
        return;
    };

    // Phase E: Regime freshness check.
    let bar_duration_secs = timeframe_secs(timeframe);
    let regime_age = bar.timestamp - regime_signal.computed_at;
    if regime_age > bar_duration_secs * 3 {
        warn!(
            "bar={} regime is stale ({} s old, limit {} s) — skipping signal evaluation",
            bar.timestamp, regime_age, bar_duration_secs * 3
        );
        return;
    }

    let allowed = regime_signal.regime.allowed_heads();
    let mut signals = Vec::new();
    for head in heads.iter_mut() {
        let head_signals = head.evaluate(bar, &session_profile, &regime_signal);
        if allowed.contains(&head.id()) {
            signals.extend(head_signals);
        }
    }

    if signals.is_empty() {
        info!(
            "bar={} regime={:?} close={} no eligible signals",
            bar.timestamp, regime_signal.regime, bar.close
        );
        return;
    }

    for signal in signals {
        match signal.kind {
            SignalKind::Close => {
                if let Some(position) = runtime.positions_by_head.remove(&signal.head) {
                    if execute {
                        match client.close_position(&CloseRequest {
                            position_id: position.position_id,
                            lots: None,
                        }) {
                            Ok(report) => {
                                if let Some(trade_id) = position.trade_id {
                                    if let Err(err) = close_trade(
                                        db.conn(),
                                        &TradeClose {
                                            trade_id,
                                            closed_at: report.close_time,
                                            close_price: report.close_price,
                                            pnl_usd: report.pnl - report.commission,
                                            r_multiple: position
                                                .signal
                                                .rr_ratio()
                                                .unwrap_or(Decimal::ZERO),
                                            close_reason: "HeadClose".to_string(),
                                            slippage_pips: report.slippage_pips,
                                        },
                                    ) {
                                        warn!("Failed to persist live close: {err}");
                                    }
                                }
                                println!(
                                    "[LIVE CLOSE] {:?} pos={} price={} lots={}",
                                    signal.head,
                                    report.position_id,
                                    report.close_price,
                                    report.closed_lots
                                );
                            }
                            Err(err) => {
                                warn!("Close failed for {:?}: {err}", signal.head);
                                runtime.positions_by_head.insert(signal.head, position);
                            }
                        }
                    } else {
                        println!(
                            "[DRY RUN CLOSE] {:?} pos={}",
                            signal.head, position.position_id
                        );
                    }
                }
            }
            SignalKind::Open => {
                if signal.head_confidence < MIN_SIGNAL_CONFIDENCE {
                    continue;
                }
                if runtime.positions_by_head.len() >= ctx.firm_file.firm.max_positions as usize {
                    warn!("Skipping signal: max positions reached");
                    continue;
                }
                if runtime.positions_by_head.contains_key(&signal.head) {
                    warn!(
                        "Skipping signal: existing position tracked for {:?}",
                        signal.head
                    );
                    continue;
                }
                if !passes_net_rr_gate(&runtime.execution, &signal) {
                    warn!("Skipping signal: spread-adjusted R:R below threshold");
                    continue;
                }

                // Phase D: Firm spread limit (FundingPips ≤ 1.0 pip).
                let pip_size = pip_size_for(&ctx.symbol);
                let current_spread_pips = runtime.execution.current_spread();
                if let Some(max_spread) = runtime.compliance.max_spread_pips() {
                    if current_spread_pips > max_spread {
                        warn!(
                            "Skipping signal: spread {:.2} pips exceeds firm limit {:.2}",
                            current_spread_pips, max_spread
                        );
                        continue;
                    }
                }

                // Phase A: Stale tick gate — reject if price data is too old.
                if runtime.execution.is_stale(bar.timestamp) {
                    warn!("Skipping signal: stale price data");
                    continue;
                }

                let risk_pct = RiskPercent::clamped(ctx.config.risk.base_risk_pct);
                let account_equity = if execute {
                    match client.account_info() {
                        Ok(info) => info.equity,
                        Err(err) => {
                            warn!("Skipping execution, account info unavailable: {err}");
                            continue;
                        }
                    }
                } else {
                    match client.account_info() {
                        Ok(info) => info.equity,
                        Err(err) => {
                            warn!(
                                "Account info unavailable during dry run, using configured balance: {err}"
                            );
                            ctx.balance
                        }
                    }
                };

                let lots = match calculate_lots(&SizingInputs {
                    risk_pct,
                    account_equity,
                    sl_distance_price: (signal.entry - signal.stop_loss).abs(),
                    pip_size,
                    pip_value_per_lot: pip_value_for(&ctx.symbol),
                    min_lot: dec!(0.01),
                    max_lot: dec!(50.0),
                    lot_step: dec!(0.01),
                }) {
                    Ok(lots) => lots,
                    Err(err) => {
                        warn!("Skipping execution, sizing failed: {err}");
                        continue;
                    }
                };

                let active_exposures = runtime
                    .positions_by_head
                    .values()
                    .map(|position| ComplianceOpenExposure {
                        symbol: position.signal.symbol.clone(),
                        direction: position.signal.direction,
                        risk_pct: position.risk_pct,
                        lots: position.lots,
                        opened_at: position.opened_at,
                    })
                    .collect::<Vec<_>>();
                if let Err(rejection) = runtime.compliance.evaluate_entry(
                    &signal,
                    risk_pct.inner(),
                    lots,
                    &active_exposures,
                    bar.timestamp,
                ) {
                    warn!(
                        "Skipping signal for {:?}: compliance rule blocked entry ({})",
                        signal.head, rejection.detail
                    );
                    continue;
                }

                if !execute {
                    println!(
                        "[DRY RUN OPEN] {:?} {:?} entry={} sl={} tp={} lots={} rr={:.2}",
                        signal.head,
                        signal.direction,
                        signal.entry,
                        signal.stop_loss,
                        signal.take_profit,
                        lots,
                        signal.rr_ratio().unwrap_or(Decimal::ZERO)
                    );
                    continue;
                }

                match client.send_order(&OrderRequest {
                    symbol: ctx.symbol.clone(),
                    direction: signal.direction,
                    lots,
                    order_type: OrderType::Market,
                    stop_loss: signal.stop_loss,
                    take_profit: signal.take_profit,
                    comment: format!("GADARAH {:?}", signal.head),
                }) {
                    Ok(fill) => {
                        // Phase A: Compute actual slippage from fill vs signal entry.
                        let actual_slippage = if !pip_size.is_zero() {
                            (fill.fill_price - signal.entry).abs() / pip_size
                        } else {
                            Decimal::ZERO
                        };
                        let fill = gadarah_broker::FillReport {
                            slippage_pips: actual_slippage,
                            ..fill
                        };

                        runtime.execution.record_fill(FillRecord {
                            order_id: fill.position_id as i64,
                            symbol: ctx.symbol.clone(),
                            direction: signal.direction,
                            requested_price: signal.entry,
                            fill_price: fill.fill_price,
                            slippage_pips: actual_slippage,
                            filled_at: fill.fill_time,
                            retries: 0,
                        });

                        let trade_id = persist_live_open(
                            db,
                            runtime.account_id,
                            &signal,
                            lots,
                            risk_pct.inner(),
                            &fill,
                        )
                        .map_err(|err| {
                            warn!("Failed to persist live open: {err}");
                            err
                        })
                        .ok();

                        // Phase F: Build initial PyramidState for this position.
                        let pip_val = pip_value_for(&ctx.symbol);
                        let risk_usd =
                            lots * ((signal.entry - signal.stop_loss).abs() / pip_size) * pip_val;
                        let pyramid_state = PyramidState::new(
                            lots,
                            fill.fill_price,
                            signal.stop_loss,
                            risk_usd,
                            signal.direction,
                            signal.regime,
                        );

                        runtime.positions_by_head.insert(
                            signal.head,
                            LivePositionState {
                                position_id: fill.position_id,
                                trade_id,
                                signal: signal.clone(),
                                lots,
                                risk_pct: risk_pct.inner(),
                                opened_at: fill.fill_time,
                                pyramid: Some(pyramid_state),
                            },
                        );
                        runtime.compliance.record_entry(
                            &signal,
                            risk_pct.inner(),
                            lots,
                            fill.fill_time,
                        );
                        println!(
                            "[LIVE OPEN] {:?} {:?} pos={} fill={} lots={}",
                            signal.head,
                            signal.direction,
                            fill.position_id,
                            fill.fill_price,
                            fill.filled_lots
                        );
                    }
                    Err(err) => warn!("Order failed for {:?}: {err}", signal.head),
                }
            }
            _ => {}
        }
    }

    snapshot_live_equity(db, runtime.account_id, client, ctx.balance, bar.timestamp);

    // Phase C: Check DD-based kill switch. Notify Discord on first activation.
    if execute {
        if let Ok(info) = client.account_info() {
            let daily_dd_pct = ctx.firm_file.firm.daily_dd_limit_pct;
            let total_dd_pct = ctx.firm_file.firm.max_dd_limit_pct;
            let balance = ctx.balance;
            let total_dd_used = if balance > Decimal::ZERO {
                (balance - info.equity) / balance * dec!(100)
            } else {
                Decimal::ZERO
            };
            // Trigger if within 95% of either limit.
            let daily_trigger = daily_dd_pct * dec!(0.95);
            let total_trigger = total_dd_pct * dec!(0.95);
            let should_halt = total_dd_used >= total_trigger;
            if should_halt && !runtime.kill_switch_active {
                runtime.kill_switch_active = true;
                let msg = format!(
                    "🚨 GADARAH KILL SWITCH: total DD {:.2}% hit {:.2}% trigger on {} {}",
                    total_dd_used, total_trigger, ctx.symbol, ctx.firm_file.firm.name
                );
                warn!("{}", msg);
                crate::notify_discord(&msg);
            } else if !should_halt && runtime.kill_switch_active {
                runtime.kill_switch_active = false;
            }
            if runtime.kill_switch_active {
                warn!(
                    "Kill switch active (total DD {:.2}%) — skipping pyramid and new entries",
                    total_dd_used
                );
                // Write GUI state and return early — no pyramid adds when halted.
                if let Some(path) = gui_state_path {
                    write_gui_state(path, &info, &runtime, &regime_signal, &ctx.symbol);
                }
                return;
            }
            let _ = daily_trigger; // used in future per-day DD tracking
        }
    }

    // Phase F: Pyramid — check open positions for eligible adds.
    if execute && runtime.pyramid_enabled {
        let pip_size = pip_size_for(&ctx.symbol);
        let pip_val = pip_value_for(&ctx.symbol);
        let pyramid_cfg = runtime.pyramid_config.clone();
        let heads_with_pyramid: Vec<HeadId> = runtime
            .positions_by_head
            .iter()
            .filter_map(|(head_id, pos)| {
                let state = pos.pyramid.as_ref()?;
                let candidate = PyramidAddCandidate {
                    current_price: bar.close,
                    current_regime: regime_signal.regime,
                    day_state: DayState::Normal, // conservative default
                    pip_value_per_lot: pip_val,
                    pip_size,
                    take_profit: pos.signal.take_profit,
                };
                if can_add_pyramid(&pyramid_cfg, state, &candidate) {
                    Some(*head_id)
                } else {
                    None
                }
            })
            .collect();

        for head_id in heads_with_pyramid {
            if let Some(pos) = runtime.positions_by_head.get_mut(&head_id) {
                let state = match pos.pyramid.as_mut() {
                    Some(s) => s,
                    None => continue,
                };
                let layer = create_pyramid_layer(&runtime.pyramid_config.clone(), state, bar.close, bar.timestamp);
                let order = OrderRequest {
                    symbol: ctx.symbol.clone(),
                    direction: pos.signal.direction,
                    lots: layer.lots,
                    order_type: OrderType::Market,
                    stop_loss: state.initial_entry, // SL at breakeven
                    take_profit: pos.signal.take_profit,
                    comment: format!("GADARAH Pyramid {:?}", head_id),
                };
                match client.send_order(&order) {
                    Ok(fill) => {
                        println!(
                            "[PYRAMID ADD] {:?} pos={} fill={} lots={}",
                            head_id, fill.position_id, fill.fill_price, fill.filled_lots
                        );
                    }
                    Err(err) => warn!("Pyramid order failed for {:?}: {err}", head_id),
                }
            }
        }
    }

    // Phase G: Write GUI state snapshot file if configured.
    if let Some(path) = gui_state_path {
        if let Ok(info) = client.account_info() {
            write_gui_state(path, &info, &runtime, &regime_signal, &ctx.symbol);
        }
    }
}

fn write_gui_state(
    path: &str,
    info: &gadarah_broker::BrokerAccountInfo,
    runtime: &LiveRuntime,
    regime_signal: &RegimeSignal9,
    symbol: &str,
) {
    let positions: Vec<serde_json::Value> = runtime
        .positions_by_head
        .iter()
        .map(|(head, pos)| {
            serde_json::json!({
                "head": format!("{:?}", head),
                "position_id": pos.position_id,
                "direction": format!("{:?}", pos.signal.direction),
                "symbol": pos.signal.symbol,
                "entry": pos.signal.entry.to_string(),
                "sl": pos.signal.stop_loss.to_string(),
                "tp": pos.signal.take_profit.to_string(),
                "lots": pos.lots.to_string(),
                "opened_at": pos.opened_at,
            })
        })
        .collect();

    let snapshot = serde_json::json!({
        "balance": info.balance.to_string(),
        "equity": info.equity.to_string(),
        "free_margin": info.free_margin.to_string(),
        "daily_pnl": (info.equity - info.balance).to_string(),
        "kill_switch_active": runtime.kill_switch_active,
        "regime": format!("{:?}", regime_signal.regime),
        "symbol": symbol,
        "positions": positions,
        "updated_at": chrono::Utc::now().timestamp(),
    });

    if let Ok(mut f) = std::fs::File::create(path) {
        let _ = write!(f, "{}", snapshot);
    }
}

fn reconcile_open_positions(
    client: &mut CtraderClient,
    positions_by_head: &mut HashMap<HeadId, LivePositionState>,
) {
    match client.reconcile_blocking() {
        Ok(reconcile) if reconcile.open_position_count == 0 => {
            positions_by_head.clear();
        }
        Ok(reconcile) if reconcile.open_position_count < positions_by_head.len() => {
            warn!(
                "Broker reports {} open positions but {} are tracked locally; clearing local cache",
                reconcile.open_position_count,
                positions_by_head.len()
            );
            positions_by_head.clear();
        }
        Ok(_) => {}
        Err(err) => warn!("Reconcile failed: {err}"),
    }
}

fn load_context(args: &[String]) -> Result<Phase1Context, String> {
    let config_path =
        arg_value(args, "--config").unwrap_or_else(|| DEFAULT_CONFIG_PATH.to_string());
    let firm_path = arg_value(args, "--firm").unwrap_or_else(|| DEFAULT_FIRM_PATH.to_string());
    let config = load_config(Path::new(&config_path))?;
    let firm_file = load_firm_config(Path::new(&firm_path))?;
    let compliance_blackout_windows =
        config.fundingpips_blackout_windows(Path::new(&config_path))?;

    let db_path = arg_value(args, "--db").unwrap_or_else(|| config.engine.db_path.clone());

    // --symbol overrides config; otherwise use all configured symbols.
    let explicit_symbol = arg_value(args, "--symbol");
    let symbols = if let Some(sym) = &explicit_symbol {
        vec![sym.clone()]
    } else {
        config.engine.symbols.clone()
    };
    let symbol = explicit_symbol
        .or_else(|| config.engine.symbols.first().cloned())
        .ok_or_else(|| "No symbol configured. Pass --symbol or set [engine].symbols".to_string())?;

    let balance = match arg_value(args, "--balance") {
        Some(raw) => raw
            .parse::<Decimal>()
            .map_err(|err| format!("Invalid --balance value {raw}: {err}"))?,
        None => default_balance_for_firm(&firm_file.firm.name),
    };

    Ok(Phase1Context {
        config,
        firm_file,
        config_path,
        firm_path,
        db_path,
        symbol,
        symbols,
        balance,
        compliance_blackout_windows,
    })
}

fn load_phase1_bars(ctx: &Phase1Context, timeframe: Timeframe) -> Result<Vec<Bar>, String> {
    let db = Database::open(&ctx.db_path)
        .map_err(|err| format!("Failed to open database {}: {err}", ctx.db_path))?;
    let bars = load_all_bars(db.conn(), &ctx.symbol, timeframe)
        .map_err(|err| format!("Failed to load {} {:?} bars: {err}", ctx.symbol, timeframe))?;
    if bars.is_empty() {
        return Err(format!(
            "No {:?} bars found for {} in {}",
            timeframe, ctx.symbol, ctx.db_path
        ));
    }
    Ok(bars)
}

fn load_series_readiness(
    ctx: &Phase1Context,
    timeframe: Timeframe,
    min_history_days: i64,
) -> Result<gadarah_data::DatasetSeriesReport, String> {
    let db = Database::open(&ctx.db_path)
        .map_err(|err| format!("Failed to open database {}: {err}", ctx.db_path))?;
    let report = build_dataset_readiness_report(
        db.conn(),
        &DatasetRequirements {
            required_symbols: vec![ctx.symbol.clone()],
            required_timeframes: vec![timeframe],
            min_history_days,
        },
    )
    .map_err(|err| format!("Failed to audit dataset readiness: {err}"))?;

    report
        .series
        .into_iter()
        .next()
        .ok_or_else(|| "Dataset readiness report returned no series".to_string())
}

fn build_engine_config(ctx: &Phase1Context) -> BacktestEngineConfig {
    let typical_spread = typical_spread_pips(&ctx.symbol);
    BacktestEngineConfig {
        symbol: ctx.symbol.clone(),
        pip_size: pip_size_for(&ctx.symbol),
        pip_value_per_lot: pip_value_for(&ctx.symbol),
        starting_balance: ctx.balance,
        base_risk_pct: ctx.config.risk.base_risk_pct,
        min_rr: dec!(1.0),
        max_spread_pips: (typical_spread * dec!(2.5)).round_dp(2),
        max_positions: ctx.firm_file.firm.max_positions as usize,
        mock_config: MockConfig {
            slippage_pips: dec!(0.5),
            commission_per_lot: dec!(3.50),
            spread_pips: typical_spread,
        },
        firm: ctx.firm_file.firm.to_firm_config(),
        daily_pnl: ctx.config.daily_pnl_config(),
        equity_curve: ctx.config.equity_curve_filter_config(),
        pyramid: ctx.config.pyramid_config(),
        pyramid_enabled: ctx.config.pyramid.enabled,
        drift: ctx.config.drift_config(),
        drift_benchmarks: ctx.config.default_drift_benchmarks(),
        trade_manager: ctx.config.trade_manager_config(),
        compliance_blackout_windows: ctx.compliance_blackout_windows.clone(),
    }
}

/// Build an engine config for a specific symbol (used by multi-symbol loop).
fn build_engine_config_for(
    ctx: &Phase1Context,
    symbol: &str,
    max_positions: usize,
) -> BacktestEngineConfig {
    let typical_spread = typical_spread_pips(symbol);
    BacktestEngineConfig {
        symbol: symbol.to_string(),
        pip_size: pip_size_for(symbol),
        pip_value_per_lot: pip_value_for(symbol),
        starting_balance: ctx.balance,
        base_risk_pct: ctx.config.risk.base_risk_pct,
        min_rr: dec!(1.0),
        max_spread_pips: (typical_spread * dec!(2.5)).round_dp(2),
        max_positions,
        mock_config: MockConfig {
            slippage_pips: dec!(0.5),
            commission_per_lot: dec!(3.50),
            spread_pips: typical_spread,
        },
        firm: ctx.firm_file.firm.to_firm_config(),
        daily_pnl: ctx.config.daily_pnl_config(),
        equity_curve: ctx.config.equity_curve_filter_config(),
        pyramid: ctx.config.pyramid_config(),
        pyramid_enabled: ctx.config.pyramid.enabled,
        drift: ctx.config.drift_config(),
        drift_benchmarks: ctx.config.default_drift_benchmarks(),
        trade_manager: ctx.config.trade_manager_config(),
        compliance_blackout_windows: ctx.compliance_blackout_windows.clone(),
    }
}

/// Load M15 bars for a specific symbol from the database.
fn load_bars_for(db_path: &str, symbol: &str, timeframe: Timeframe) -> Result<Vec<Bar>, String> {
    let db = Database::open(db_path)
        .map_err(|err| format!("Failed to open database {}: {err}", db_path))?;
    let bars = load_all_bars(db.conn(), symbol, timeframe)
        .map_err(|err| format!("Failed to load {} {:?} bars: {err}", symbol, timeframe))?;
    Ok(bars)
}

fn build_execution_engine(ctx: &Phase1Context) -> RiskExecutionEngine {
    RiskExecutionEngine::new(
        RiskExecutionConfig {
            max_spread_atr_ratio: ctx.config.execution.max_spread_atr_ratio,
            stale_price_threshold: ctx.config.execution.stale_price_seconds,
            min_rr_after_spread: ctx.config.execution.min_net_rr,
            ..RiskExecutionConfig::default()
        },
        typical_spread_pips(&ctx.symbol),
    )
}

fn persist_engine_run_for(
    ctx: &Phase1Context,
    symbol: &str,
    label: &str,
    result: &gadarah_backtest::EngineResult,
) -> Result<(i64, usize, usize), String> {
    let db = Database::open(&ctx.db_path)
        .map_err(|err| format!("Failed to open database {}: {err}", ctx.db_path))?;
    let account_id = synthetic_account_id_for(symbol, &ctx.firm_file.firm.name, label);
    db.conn()
        .execute(
            "DELETE FROM trades WHERE account_id = ?1",
            params![account_id],
        )
        .map_err(|err| format!("Failed to clear trades for {}: {}", account_id, err))?;
    db.conn()
        .execute(
            "DELETE FROM equity_snapshots WHERE account_id = ?1",
            params![account_id],
        )
        .map_err(|err| {
            format!(
                "Failed to clear equity snapshots for {}: {}",
                account_id, err
            )
        })?;

    for trade in &result.trade_log {
        insert_trade(
            db.conn(),
            &TradeRecord {
                id: None,
                account_id,
                symbol: trade.symbol.clone(),
                direction: format!("{:?}", trade.direction),
                head: format!("{:?}", trade.head),
                regime: format!("{:?}", trade.regime),
                session: format!("{:?}", trade.session),
                entry_price: trade.entry_price,
                sl_price: trade.stop_loss,
                tp_price: trade.take_profit,
                lots: trade.lots,
                risk_pct: trade.risk_pct,
                pyramid_level: i32::from(trade.pyramid_level),
                opened_at: trade.opened_at,
                closed_at: Some(trade.closed_at),
                close_price: Some(trade.close_price),
                pnl_usd: Some(trade.pnl),
                r_multiple: Some(trade.r_multiple),
                close_reason: Some(trade.close_reason.clone()),
                slippage_pips: Some(trade.slippage_pips),
            },
        )
        .map_err(|err| format!("Failed to persist backtest trade: {err}"))?;
    }

    let mut peak = ctx.balance;
    let mut day_open_equity = ctx.balance;
    let mut last_day = i64::MIN;

    for (timestamp, equity) in &result.equity_curve {
        let day = timestamp.div_euclid(86_400);
        if day != last_day {
            day_open_equity = *equity;
            last_day = day;
        }
        if *equity > peak {
            peak = *equity;
        }
        let total_dd_pct = if peak > Decimal::ZERO {
            ((peak - *equity) / peak * dec!(100)).max(Decimal::ZERO)
        } else {
            Decimal::ZERO
        };

        insert_equity_snapshot(
            db.conn(),
            &EquitySnapshot {
                id: None,
                account_id,
                balance: *equity,
                equity: *equity,
                daily_pnl_usd: *equity - day_open_equity,
                daily_dd_pct: Decimal::ZERO,
                total_dd_pct,
                day_state: "Backtest".to_string(),
                snapshotted_at: *timestamp,
            },
        )
        .map_err(|err| format!("Failed to persist backtest equity snapshot: {err}"))?;
    }

    Ok((
        account_id,
        result.trade_log.len(),
        result.equity_curve.len(),
    ))
}

fn persist_live_open(
    db: &Database,
    account_id: i64,
    signal: &TradeSignal,
    lots: Decimal,
    risk_pct: Decimal,
    fill: &gadarah_broker::FillReport,
) -> Result<i64, String> {
    insert_trade(
        db.conn(),
        &TradeRecord {
            id: None,
            account_id,
            symbol: signal.symbol.clone(),
            direction: format!("{:?}", signal.direction),
            head: format!("{:?}", signal.head),
            regime: format!("{:?}", signal.regime),
            session: format!("{:?}", signal.session),
            entry_price: fill.fill_price,
            sl_price: signal.stop_loss,
            tp_price: signal.take_profit,
            lots,
            risk_pct,
            pyramid_level: i32::from(signal.pyramid_level),
            opened_at: fill.fill_time,
            closed_at: None,
            close_price: None,
            pnl_usd: None,
            r_multiple: None,
            close_reason: None,
            slippage_pips: Some(fill.slippage_pips),
        },
    )
    .map_err(|err| format!("Failed to persist live trade: {err}"))
}

fn snapshot_live_equity(
    db: &Database,
    account_id: i64,
    client: &CtraderClient,
    starting_balance: Decimal,
    timestamp: i64,
) {
    let info = match client.account_info() {
        Ok(info) => info,
        Err(err) => {
            warn!("Skipping equity snapshot, account info unavailable: {err}");
            return;
        }
    };

    let total_dd_pct = if starting_balance > Decimal::ZERO && info.equity < starting_balance {
        (starting_balance - info.equity) / starting_balance * dec!(100)
    } else {
        Decimal::ZERO
    };

    if let Err(err) = insert_equity_snapshot(
        db.conn(),
        &EquitySnapshot {
            id: None,
            account_id,
            balance: info.balance,
            equity: info.equity,
            daily_pnl_usd: info.equity - info.balance,
            daily_dd_pct: Decimal::ZERO,
            total_dd_pct,
            day_state: "Live".to_string(),
            snapshotted_at: timestamp,
        },
    ) {
        warn!("Failed to persist equity snapshot: {err}");
    }
}

fn synthetic_account_id_for(symbol: &str, firm_name: &str, label: &str) -> i64 {
    let mut hasher = DefaultHasher::new();
    label.hash(&mut hasher);
    symbol.hash(&mut hasher);
    firm_name.hash(&mut hasher);
    let raw = (hasher.finish() & (i64::MAX as u64)) as i64;
    -raw.max(1)
}

fn make_phase1_heads(symbol: &str) -> Vec<Box<dyn Head>> {
    let pip_size = pip_size_for(symbol);
    vec![
        Box::new(MomentumHead::new(MomentumConfig {
            symbol: symbol.to_string(),
            pip_size,
            first_hour_bars: 4,
            breakout_buffer_pips: Decimal::ZERO,
            ..MomentumConfig::default()
        })),
        Box::new(AsianRangeHead::new(
            AsianRangeConfig {
                symbol: symbol.to_string(),
                entry_window_end: 9,
                min_range_pips: dec!(15.0),
                max_range_pips: dec!(80.0),
                sl_buffer_pips: dec!(5.0),
                ..AsianRangeConfig::default()
            },
            pip_size,
        )),
        Box::new(BreakoutHead::new(BreakoutConfig {
            symbol: symbol.to_string(),
            squeeze_pctile: dec!(0.30),
            expansion_pctile: dec!(0.50),
            tp1_atr_mult: dec!(2.0),
            tp2_atr_mult: dec!(3.0),
            min_rr: dec!(1.8),
            fakeout_bars: 2,
            ..BreakoutConfig::default()
        })),
    ]
}

fn build_ctrader_client(ctx: &Phase1Context, live_server: bool) -> Result<CtraderClient, String> {
    let broker_cfg = &ctx.firm_file.broker;
    let client_id = std::env::var(&broker_cfg.client_id_env).map_err(|_| {
        format!(
            "Missing cTrader client id env var {}",
            broker_cfg.client_id_env
        )
    })?;
    let client_secret = std::env::var(&broker_cfg.client_secret_env).map_err(|_| {
        format!(
            "Missing cTrader client secret env var {}",
            broker_cfg.client_secret_env
        )
    })?;
    let access_token_env = broker_cfg.access_token_env_name();
    let access_token = std::env::var(&access_token_env)
        .map_err(|_| format!("Missing cTrader access token env var {}", access_token_env))?;
    let account_id_env = broker_cfg.account_id_env_name();
    let account_id_raw = std::env::var(&account_id_env)
        .map_err(|_| format!("Missing cTrader account id env var {}", account_id_env))?;
    let account_id = account_id_raw
        .parse::<i64>()
        .map_err(|err| format!("Invalid cTrader account id {}: {}", account_id_raw, err))?;

    let mut config =
        CtraderConfig::new(client_id, client_secret).with_account(access_token, account_id);
    if live_server {
        config = config.live();
    }
    Ok(CtraderClient::new(config))
}

fn challenge_rules_for(firm: &FirmConfigFile) -> ChallengeRules {
    let name = firm.firm.name.to_lowercase();
    let ctype = firm.firm.challenge_type.to_lowercase();

    if name.contains("ftmo") || ctype.starts_with("ftmo") {
        if name.contains("2-step") || name.contains("2 step") || ctype.contains("2step") {
            ChallengeRules::ftmo_2step()
        } else {
            ChallengeRules::ftmo_1step()
        }
    } else if name.contains("alpha capital")
        || name.contains("alpha one")
        || ctype == "alpha_one"
    {
        ChallengeRules::alpha_one()
    } else if name.contains("fundingpips") && (name.contains("zero") || ctype == "fundingpips_zero")
    {
        ChallengeRules::fundingpips_zero()
    } else if name.contains("fundingpips")
        && (name.contains("1-step") || name.contains("1 step") || ctype == "fundingpips_1step")
    {
        ChallengeRules::fundingpips_1step()
    } else if name.contains("the5ers")
        || name.contains("hyper growth")
        || ctype == "hyper_growth"
    {
        ChallengeRules::the5ers_hyper_growth()
    } else if name.contains("blue guardian") || ctype == "instant" {
        ChallengeRules::blue_guardian_instant()
    } else if name.contains("brightfunded") {
        ChallengeRules::brightfunded_evaluation()
    } else {
        ChallengeRules::two_step_pro()
    }
}

fn profitable_day_rate(trades: &[gadarah_backtest::TradeResult]) -> Decimal {
    if trades.is_empty() {
        return Decimal::ZERO;
    }

    let mut by_day: BTreeMap<i64, Decimal> = BTreeMap::new();
    for trade in trades {
        *by_day
            .entry(trade.closed_at.div_euclid(86_400))
            .or_default() += trade.pnl;
    }

    let profitable_days = by_day.values().filter(|pnl| **pnl > Decimal::ZERO).count();
    Decimal::from(profitable_days) / Decimal::from(by_day.len())
}

fn synthetic_transition_regime(timestamp: i64) -> RegimeSignal9 {
    RegimeSignal9 {
        regime: Regime9::Transitioning,
        confidence: Decimal::ZERO,
        adx: Decimal::ZERO,
        hurst: Decimal::ZERO,
        atr_ratio: Decimal::ZERO,
        bb_width_pctile: Decimal::ZERO,
        choppiness_index: Decimal::ZERO,
        computed_at: timestamp,
    }
}

fn passes_net_rr_gate(execution: &RiskExecutionEngine, signal: &TradeSignal) -> bool {
    execution
        .adjusted_rr(signal)
        .map(|rr| rr >= dec!(1.2))
        .unwrap_or(false)
}

fn timeframe_secs(tf: Timeframe) -> i64 {
    match tf {
        Timeframe::M1 => 60,
        Timeframe::M5 => 300,
        Timeframe::M15 => 900,
        Timeframe::H1 => 3600,
        Timeframe::H4 => 14400,
        Timeframe::D1 => 86400,
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

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].clone())
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|arg| arg == flag)
}

fn parse_usize_arg(args: &[String], flag: &str, default: usize) -> usize {
    arg_value(args, flag)
        .and_then(|raw| raw.parse::<usize>().ok())
        .unwrap_or(default)
}

fn parse_u64_arg(args: &[String], flag: &str, default: u64) -> u64 {
    arg_value(args, flag)
        .and_then(|raw| raw.parse::<u64>().ok())
        .unwrap_or(default)
}

fn parse_i64_arg(args: &[String], flag: &str, default: i64) -> i64 {
    arg_value(args, flag)
        .and_then(|raw| raw.parse::<i64>().ok())
        .unwrap_or(default)
}

fn default_balance_for_firm(name: &str) -> Decimal {
    let lower = name.to_lowercase();
    // FTMO standard entry accounts start at $10k
    if lower.contains("ftmo") {
        dec!(10000)
    } else if lower.contains("alpha capital")
        || lower.contains("alpha one")
        || lower.contains("the5ers")
        || lower.contains("hyper growth")
        || lower.contains("blue guardian")
        || lower.contains("fundingpips")
    {
        dec!(5000)
    } else {
        dec!(10000)
    }
}

fn print_fundingpips_compliance_status(ctx: &Phase1Context) {
    let firm_config = ctx.firm_file.firm.to_firm_config();
    let manager = gadarah_risk::PropFirmComplianceManager::for_firm(&firm_config);
    if !manager.is_enabled() {
        return;
    }

    println!(
        "Compliance:  {} — blackout windows loaded={}",
        manager.program_label(),
        ctx.compliance_blackout_windows.len()
    );
}

fn pip_size_for(symbol: &str) -> Decimal {
    if symbol.ends_with("JPY") || symbol == "XAUUSD" {
        dec!(0.01)
    } else {
        dec!(0.0001)
    }
}

fn pip_value_for(symbol: &str) -> Decimal {
    if symbol == "XAUUSD" {
        dec!(1.0)
    } else if symbol.ends_with("JPY") {
        dec!(9.5)
    } else {
        dec!(10.0)
    }
}

fn typical_spread_pips(symbol: &str) -> Decimal {
    if symbol == "XAUUSD" {
        dec!(20.0)
    } else if symbol.ends_with("JPY") {
        dec!(1.0)
    } else {
        dec!(1.2)
    }
}

fn print_stats_report(stats: &gadarah_backtest::BacktestStats, elapsed_secs: f64) {
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

fn print_engine_diagnostics(diag: &gadarah_backtest::engine::EngineDiagnostics) {
    if diag.bars_with_regime == 0 && diag.head_signals.is_empty() {
        return;
    }

    println!("\n--- Regime Distribution ---");
    println!("{:<18} {:>8} {:>9}", "Regime", "Bars", "Eligible");
    println!("{}", "-".repeat(39));
    for regime in regime_order() {
        let bars = diag.bars_by_regime.get(&regime).copied().unwrap_or(0);
        let eligible = diag
            .eligible_bars_by_regime
            .get(&regime)
            .copied()
            .unwrap_or(0);
        if bars == 0 && eligible == 0 {
            continue;
        }
        println!("{:<18} {:>8} {:>9}", format!("{regime:?}"), bars, eligible);
    }

    println!("\n--- Session Distribution ---");
    println!("{:<10} {:>8} {:>9}", "Session", "Bars", "Eligible");
    println!("{}", "-".repeat(31));
    for session in session_order() {
        let bars = diag.bars_by_session.get(&session).copied().unwrap_or(0);
        let eligible = diag
            .eligible_bars_by_session
            .get(&session)
            .copied()
            .unwrap_or(0);
        if bars == 0 && eligible == 0 {
            continue;
        }
        println!("{:<10} {:>8} {:>9}", format!("{session:?}"), bars, eligible);
    }

    if diag.head_signals.is_empty() {
        return;
    }

    println!("\n--- Per-Head Signal Funnel ---");
    for head in head_order() {
        let stats = diag.head_signals.get(&head).cloned().unwrap_or_default();
        println!(
            "{:?}: cand={} blocked={} pass={} exec={} close={} close_exec={}",
            head,
            stats.open_candidates,
            stats.blocked_open_signals,
            stats.passed_filters,
            stats.executed_entries,
            stats.close_signals,
            stats.executed_closes
        );

        let mut reasons = Vec::new();
        push_reason(&mut reasons, "min_rr", stats.rejected_min_rr);
        push_reason(&mut reasons, "sl", stats.rejected_sl_distance);
        push_reason(&mut reasons, "conf", stats.rejected_confidence);
        push_reason(&mut reasons, "segment", stats.rejected_segment);
        push_reason(&mut reasons, "risk0", stats.rejected_effective_risk);
        push_reason(&mut reasons, "slots", stats.rejected_max_positions);
        push_reason(&mut reasons, "size", stats.rejected_sizing);
        push_reason(&mut reasons, "net_rr", stats.rejected_net_rr);
        push_reason(&mut reasons, "firm", stats.rejected_compliance);
        push_reason(&mut reasons, "broker", stats.rejected_order_error);
        if !reasons.is_empty() || stats.blocked_close_signals > 0 {
            if stats.blocked_close_signals > 0 {
                reasons.push(format!("blocked_close={}", stats.blocked_close_signals));
            }
            println!("  rejects: {}", reasons.join(", "));
        }
    }

    let mut segments: Vec<_> = diag.segment_signals.iter().collect();
    segments.sort_by(|a, b| {
        b.1.open_candidates
            .cmp(&a.1.open_candidates)
            .then_with(|| b.1.executed_entries.cmp(&a.1.executed_entries))
            .then_with(|| format!("{:?}", a.0).cmp(&format!("{:?}", b.0)))
    });

    if !segments.is_empty() {
        println!("\n--- Top Segments ---");
        for ((head, regime, session), stats) in segments.into_iter().take(8) {
            if stats.open_candidates == 0 && stats.blocked_open_signals == 0 {
                continue;
            }
            println!(
                "{:?} / {:?} / {:?}: cand={} blocked={} pass={} exec={}",
                head,
                regime,
                session,
                stats.open_candidates,
                stats.blocked_open_signals,
                stats.passed_filters,
                stats.executed_entries
            );
        }
    }
}

fn print_head_breakdown(trades: &[gadarah_backtest::TradeResult]) {
    let mut by_head: HashMap<HeadId, (usize, usize, Decimal)> = HashMap::new();
    for trade in trades {
        let entry = by_head.entry(trade.head).or_insert((0, 0, Decimal::ZERO));
        entry.0 += 1;
        if trade.is_winner {
            entry.1 += 1;
        }
        entry.2 += trade.pnl;
    }

    if by_head.is_empty() {
        return;
    }

    println!("\n--- Per-Head Breakdown ---");
    println!("{:<15} {:>6} {:>8} {:>10}", "Head", "Trades", "Win%", "PnL");
    println!("{}", "-".repeat(42));

    let mut heads: Vec<_> = by_head.into_iter().collect();
    heads.sort_by_key(|(head, _)| format!("{:?}", head));

    for (head, (total, wins, pnl)) in heads {
        let win_rate = if total == 0 {
            Decimal::ZERO
        } else {
            Decimal::from(wins) / Decimal::from(total) * dec!(100)
        };
        println!(
            "{:<15} {:>6} {:>7.1}% ${:>9.2}",
            format!("{:?}", head),
            total,
            win_rate,
            pnl
        );
    }
}

fn print_challenge_result(result: &ChallengeSimResult) {
    println!("--- {} ---", result.rules.name);
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
            "    {}:   {:.2}% / {:.1}% limit → {}",
            stage.stage_rules.daily_limit_label(),
            stage.max_daily_dd_pct,
            stage.stage_rules.daily_dd_limit_pct,
            if stage.daily_limit_hit {
                if stage.stage_rules.daily_limit_label() == "Daily Pause" {
                    "TRIGGERED"
                } else {
                    "BREACHED"
                }
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
}

fn print_challenge_batch(batch: &ChallengeBatchResult) {
    println!();
    println!("  100-run batch:");
    println!("    Pass rate:       {:.1}%", batch.pass_rate * dec!(100));
    println!(
        "    DD breach rate:  {:.1}%",
        batch.dd_breach_rate * dec!(100)
    );
    if let Some(days) = batch.avg_days_to_pass {
        println!("    Avg days:        {:.1}", days);
    }
    if let Some(days) = batch.median_days_to_pass {
        println!("    Median days:     {}", days);
    }
}

fn print_data_audit(audit: &DataAuditResult) {
    println!("Bars:               {}", audit.total_bars);
    if let Some(start_ts) = audit.start_ts {
        println!("Start ts:           {}", start_ts);
    }
    if let Some(end_ts) = audit.end_ts {
        println!("End ts:             {}", end_ts);
    }
    println!("Duplicates:         {}", audit.duplicate_timestamps);
    println!("Out of order:       {}", audit.out_of_order_bars);
    println!("Misaligned:         {}", audit.misaligned_timestamps);
    println!("Unexpected gaps:    {}", audit.unexpected_gap_count);
    println!("Missing bars est.:  {}", audit.missing_bar_estimate);
    println!("Largest gap secs:   {}", audit.largest_unexpected_gap_secs);
    println!("Invalid OHLC bars:  {}", audit.invalid_price_bars);
    println!("Zero-volume bars:   {}", audit.zero_volume_bars);
}

fn print_single_series_readiness(series: &gadarah_data::DatasetSeriesReport) {
    println!("History days:       {}", series.history_span_days);
    println!(
        "History gate:       {}",
        if series.history_passed {
            "PASS"
        } else {
            "FAIL"
        }
    );
    println!(
        "Zero-volume series: {}",
        if series.all_zero_volume { "YES" } else { "NO" }
    );
}

fn verdict(pass: bool) -> &'static str {
    if pass {
        "PASS"
    } else {
        "FAIL"
    }
}

fn push_reason(reasons: &mut Vec<String>, label: &str, count: usize) {
    if count > 0 {
        reasons.push(format!("{label}={count}"));
    }
}

fn head_order() -> [HeadId; 3] {
    [HeadId::Momentum, HeadId::AsianRange, HeadId::Breakout]
}

fn session_order() -> [Session; 5] {
    [
        Session::Asian,
        Session::London,
        Session::Overlap,
        Session::NyPm,
        Session::Dead,
    ]
}

fn regime_order() -> [Regime9; 9] {
    [
        Regime9::StrongTrendUp,
        Regime9::StrongTrendDown,
        Regime9::WeakTrendUp,
        Regime9::WeakTrendDown,
        Regime9::RangingTight,
        Regime9::RangingWide,
        Regime9::Choppy,
        Regime9::BreakoutPending,
        Regime9::Transitioning,
    ]
}
