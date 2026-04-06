//! Stress test parameter tuner
//!
//! Runs stress tests with varying parameters to find robust configurations.

use gadarah_backtest::{run_replay, run_stress_test, ReplayConfig, StressConfig};
use gadarah_core::{
    heads::{
        asian_range::{AsianRangeConfig, AsianRangeHead},
        breakout::{BreakoutConfig, BreakoutHead},
        momentum::{MomentumConfig, MomentumHead},
    },
    Timeframe,
};
use gadarah_data::{load_all_bars, Database};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;

/// Tuning result for a single parameter set
#[derive(Debug)]
pub struct TuneResult {
    pub loss_mult: Decimal,
    pub wr_reduction: Decimal,
    pub slippage: Decimal,
    pub stressed_pf: Decimal,
    pub stressed_dd: Decimal,
    pub pass: bool,
}

/// Run stress test tuning across multiple symbols and parameter ranges
pub fn tune_stress_params(db_path: &str, symbols: &[&str], _iterations: usize) -> Vec<TuneResult> {
    let mut results = Vec::new();

    // Parameter ranges to test
    let loss_mults = vec![dec!(1.0), dec!(1.2), dec!(1.3), dec!(1.4), dec!(1.5)];
    let wr_reductions = vec![dec!(0.0), dec!(0.05), dec!(0.08), dec!(0.10)];
    let slippages = vec![dec!(0.0), dec!(1.0), dec!(1.5), dec!(2.0)];

    let db = Database::open(db_path).expect("Failed to open DB");

    for symbol in symbols {
        println!("\n=== Testing {} ===", symbol);

        let bars = match load_all_bars(db.conn(), symbol, Timeframe::M15) {
            Ok(b) => b,
            Err(e) => {
                println!("  Skipping {}: {}", symbol, e);
                continue;
            }
        };

        if bars.is_empty() {
            println!("  No bars for {}", symbol);
            continue;
        }

        let config = make_replay_config(symbol, dec!(10000));
        let mut heads = make_heads(symbol);

        // Run backtest once
        let result = run_replay(&bars, &mut heads, &config).expect("Replay failed");

        if result.trades.len() < 10 {
            println!("  Skipping {}: only {} trades", symbol, result.trades.len());
            continue;
        }

        println!(
            "  Backtest: {} trades, PF={:.2}",
            result.trades.len(),
            result.stats.profit_factor
        );

        // Test each parameter combination
        for loss_mult in &loss_mults {
            for wr_red in &wr_reductions {
                for slip in &slippages {
                    let stress_cfg = StressConfig {
                        loss_multiplier: *loss_mult,
                        win_rate_reduction: *wr_red,
                        extra_slippage_usd: *slip,
                    };

                    let stress = run_stress_test(&result.trades, dec!(10000), &stress_cfg, None);

                    let pass = stress.stressed_stats.profit_factor > dec!(1.0)
                        && stress.stressed_stats.max_drawdown_pct < dec!(10.0);

                    results.push(TuneResult {
                        loss_mult: *loss_mult,
                        wr_reduction: *wr_red,
                        slippage: *slip,
                        stressed_pf: stress.stressed_stats.profit_factor,
                        stressed_dd: stress.stressed_stats.max_drawdown_pct,
                        pass,
                    });
                }
            }
        }
    }

    results
}

/// Find the most robust stress parameters (passes most symbols)
pub fn find_robust_params(results: &[TuneResult]) -> StressConfig {
    // Group by parameters
    let mut param_groups: HashMap<(Decimal, Decimal, Decimal), Vec<bool>> = HashMap::new();

    for r in results {
        let key = (r.loss_mult, r.wr_reduction, r.slippage);
        param_groups.entry(key).or_default().push(r.pass);
        if !r.pass {
            tracing::debug!(
                "FAIL: loss_mult={} wr_red={} slip={} pf={:.2} dd={:.2}%",
                r.loss_mult, r.wr_reduction, r.slippage, r.stressed_pf, r.stressed_dd
            );
        }
    }

    // Find params with highest pass rate
    let mut best = None;
    let mut best_pass_rate = dec!(0.0);

    for ((lm, wr, slip), passes) in &param_groups {
        let pass_count: usize = passes.iter().map(|p| *p as usize).sum();
        let pass_rate = Decimal::from(pass_count) / Decimal::from(passes.len());

        if pass_rate > best_pass_rate {
            best_pass_rate = pass_rate;
            best = Some((lm, wr, slip, pass_rate));
        }
    }

    if let Some((lm, wr, slip, rate)) = best {
        println!(
            "\nBest stress params: loss_mult={}, wr_reduction={}, slippage={} (pass rate: {:.1}%)",
            lm,
            wr,
            slip,
            rate * dec!(100)
        );
        StressConfig {
            loss_multiplier: *lm,
            win_rate_reduction: *wr,
            extra_slippage_usd: *slip,
        }
    } else {
        StressConfig::default()
    }
}

fn make_replay_config(symbol: &str, balance: Decimal) -> ReplayConfig {
    ReplayConfig {
        symbol: symbol.to_string(),
        pip_size: if symbol.contains("JPY") {
            dec!(0.01)
        } else {
            dec!(0.0001)
        },
        pip_value_per_lot: dec!(10.0),
        starting_balance: balance,
        risk_pct: dec!(0.74),
        daily_dd_limit_pct: dec!(4.0),
        max_dd_limit_pct: dec!(6.0),
        max_positions: 3,
        min_rr: dec!(1.5),
        max_spread_pips: dec!(3.0),
        mock_config: gadarah_broker::MockConfig::default(),
        consecutive_loss_halt: 5,
    }
}

fn make_heads(symbol: &str) -> Vec<Box<dyn gadarah_core::Head>> {
    let pip_size = if symbol.contains("JPY") {
        dec!(0.01)
    } else {
        dec!(0.0001)
    };
    vec![
        Box::new(BreakoutHead::new(BreakoutConfig {
            squeeze_pctile: dec!(10.0),
            expansion_pctile: dec!(90.0),
            min_squeeze_bars: 4,
            volume_mult: dec!(1.2),
            tp1_atr_mult: dec!(1.5),
            tp2_atr_mult: dec!(2.5),
            min_rr: dec!(1.5),
            fakeout_bars: 3,
            base_confidence: dec!(0.5),
            symbol: symbol.to_string(),
        })),
        Box::new(MomentumHead::new(MomentumConfig {
            min_rr: dec!(1.5),
            base_confidence: dec!(0.5),
            first_hour_bars: 4,
            min_range_pips: dec!(10.0),
            breakout_buffer_pips: dec!(5.0),
            pip_size,
            symbol: symbol.to_string(),
        })),
        Box::new(AsianRangeHead::new(
            AsianRangeConfig {
                asian_start_utc: 0,
                asian_end_utc: 4,
                entry_window_end: 9,
                min_range_pips: dec!(15.0),
                max_range_pips: dec!(60.0),
                sl_buffer_pips: dec!(5.0),
                tp1_multiplier: dec!(1.5),
                tp2_multiplier: dec!(2.5),
                min_rr: dec!(1.5),
                max_trades_per_day: 3,
                symbol: symbol.to_string(),
                base_confidence: dec!(0.5),
            },
            pip_size,
        )),
    ]
}
