use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use gadarah_core::{Bar, Head};

use crate::engine::{run_engine, EngineConfig, EngineResult};
use crate::error::BacktestError;
use crate::replayer::{run_replay, ReplayConfig};
use crate::stats::BacktestStats;

// ---------------------------------------------------------------------------
// Walk-forward validation: rolling in-sample / out-of-sample splits
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalkForwardConfig {
    /// Number of folds (e.g. 5 for 5-fold).
    pub num_folds: usize,
    /// Fraction of each fold used for in-sample (the rest is out-of-sample).
    /// E.g. 0.70 means 70% in-sample, 30% out-of-sample.
    pub in_sample_ratio: f64,
}

impl Default for WalkForwardConfig {
    fn default() -> Self {
        Self {
            num_folds: 5,
            in_sample_ratio: 0.70,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FoldResult {
    pub fold_index: usize,
    pub in_sample_bars: usize,
    pub out_of_sample_bars: usize,
    pub in_sample_stats: BacktestStats,
    pub out_of_sample_stats: BacktestStats,
}

#[derive(Debug, Clone)]
pub struct WalkForwardResult {
    pub folds: Vec<FoldResult>,
    pub combined_oos_stats: BacktestStats,
    pub oos_degradation_pct: Decimal,
    pub passed: bool,
}

#[derive(Debug, Clone)]
pub struct EngineFoldResult {
    pub fold_index: usize,
    pub in_sample_bars: usize,
    pub out_of_sample_bars: usize,
    pub in_sample_result: EngineResult,
    pub out_of_sample_result: EngineResult,
}

#[derive(Debug, Clone)]
pub struct EngineWalkForwardResult {
    pub folds: Vec<EngineFoldResult>,
    pub combined_oos_stats: BacktestStats,
    pub oos_degradation_pct: Decimal,
    pub passed: bool,
}

/// Run walk-forward validation.
///
/// Splits the bar data into `num_folds` rolling windows. For each fold:
/// 1. Run backtest on in-sample portion (to establish baseline).
/// 2. Run backtest on out-of-sample portion (to validate).
///
/// The final result aggregates all OOS trades and computes degradation
/// (how much worse OOS is compared to IS).
///
/// `head_factory` is called for each fold to produce fresh heads with
/// reset state — this prevents lookahead bias.
pub fn run_walk_forward<F>(
    bars: &[Bar],
    head_factory: F,
    replay_config: &ReplayConfig,
    wf_config: &WalkForwardConfig,
) -> Result<WalkForwardResult, BacktestError>
where
    F: Fn() -> Vec<Box<dyn Head>>,
{
    if bars.is_empty() {
        return Err(BacktestError::NoBars);
    }
    if wf_config.num_folds < 2 {
        return Err(BacktestError::WalkForward(
            "need at least 2 folds".to_string(),
        ));
    }

    let total_bars = bars.len();
    let fold_size = total_bars / wf_config.num_folds;
    if fold_size < 200 {
        return Err(BacktestError::InsufficientBars {
            needed: 200 * wf_config.num_folds,
            got: total_bars,
        });
    }

    let is_size = (fold_size as f64 * wf_config.in_sample_ratio) as usize;
    let _oos_size = fold_size - is_size;

    let mut folds = Vec::with_capacity(wf_config.num_folds);
    let mut all_oos_trades = Vec::new();

    for fold_idx in 0..wf_config.num_folds {
        let start = fold_idx * fold_size;
        let is_end = start + is_size;
        let oos_end = (start + fold_size).min(total_bars);

        if is_end >= total_bars || oos_end > total_bars {
            break;
        }

        let is_bars = &bars[start..is_end];
        let oos_bars = &bars[is_end..oos_end];

        // In-sample run
        let mut is_heads = head_factory();
        let is_result = run_replay(is_bars, &mut is_heads, replay_config)?;

        // Out-of-sample run with fresh heads
        let mut oos_heads = head_factory();
        let oos_result = run_replay(oos_bars, &mut oos_heads, replay_config)?;

        all_oos_trades.extend(oos_result.trades.clone());

        folds.push(FoldResult {
            fold_index: fold_idx,
            in_sample_bars: is_bars.len(),
            out_of_sample_bars: oos_bars.len(),
            in_sample_stats: is_result.stats,
            out_of_sample_stats: oos_result.stats,
        });
    }

    let combined_oos_stats =
        BacktestStats::compute(&all_oos_trades, replay_config.starting_balance);

    // Compute degradation: avg IS profit factor vs OOS profit factor
    let avg_is_pf = if !folds.is_empty() {
        folds
            .iter()
            .map(|f| f.in_sample_stats.profit_factor)
            .sum::<Decimal>()
            / Decimal::from(folds.len())
    } else {
        Decimal::ONE
    };

    let oos_degradation_pct = if avg_is_pf > Decimal::ZERO {
        (avg_is_pf - combined_oos_stats.profit_factor) / avg_is_pf * dec!(100)
    } else {
        dec!(100)
    };

    // Pass criteria: OOS profit factor > 1.0 and degradation < 40%
    let passed = combined_oos_stats.profit_factor > Decimal::ONE && oos_degradation_pct < dec!(40);

    Ok(WalkForwardResult {
        folds,
        combined_oos_stats,
        oos_degradation_pct,
        passed,
    })
}

pub fn run_walk_forward_engine<F>(
    bars: &[Bar],
    head_factory: F,
    engine_config: &EngineConfig,
    wf_config: &WalkForwardConfig,
) -> Result<EngineWalkForwardResult, BacktestError>
where
    F: Fn() -> Vec<Box<dyn Head>>,
{
    if bars.is_empty() {
        return Err(BacktestError::NoBars);
    }
    if wf_config.num_folds < 2 {
        return Err(BacktestError::WalkForward(
            "need at least 2 folds".to_string(),
        ));
    }

    let total_bars = bars.len();
    let fold_size = total_bars / wf_config.num_folds;
    if fold_size < 200 {
        return Err(BacktestError::InsufficientBars {
            needed: 200 * wf_config.num_folds,
            got: total_bars,
        });
    }

    let is_size = (fold_size as f64 * wf_config.in_sample_ratio) as usize;
    let mut folds = Vec::with_capacity(wf_config.num_folds);
    let mut all_oos_trades = Vec::new();

    for fold_idx in 0..wf_config.num_folds {
        let start = fold_idx * fold_size;
        let is_end = start + is_size;
        let oos_end = (start + fold_size).min(total_bars);

        if is_end >= total_bars || oos_end > total_bars {
            break;
        }

        let is_bars = &bars[start..is_end];
        let oos_bars = &bars[is_end..oos_end];

        let mut is_heads = head_factory();
        let is_result = run_engine(is_bars, &mut is_heads, engine_config)?;

        let mut oos_heads = head_factory();
        let oos_result = run_engine(oos_bars, &mut oos_heads, engine_config)?;

        all_oos_trades.extend(oos_result.trades.clone());

        folds.push(EngineFoldResult {
            fold_index: fold_idx,
            in_sample_bars: is_bars.len(),
            out_of_sample_bars: oos_bars.len(),
            in_sample_result: is_result,
            out_of_sample_result: oos_result,
        });
    }

    let combined_oos_stats =
        BacktestStats::compute(&all_oos_trades, engine_config.starting_balance);

    let avg_is_pf = if !folds.is_empty() {
        folds
            .iter()
            .map(|f| f.in_sample_result.stats.profit_factor)
            .sum::<Decimal>()
            / Decimal::from(folds.len())
    } else {
        Decimal::ONE
    };

    let oos_degradation_pct = if avg_is_pf > Decimal::ZERO {
        (avg_is_pf - combined_oos_stats.profit_factor) / avg_is_pf * dec!(100)
    } else {
        dec!(100)
    };

    let passed = !folds.is_empty()
        && folds
            .iter()
            .filter(|fold| {
                fold.out_of_sample_result.stats.profit_factor >= dec!(1.20)
                    && fold.out_of_sample_result.stats.max_drawdown_pct < dec!(8.0)
            })
            .count()
            >= 4
        && combined_oos_stats.profit_factor >= dec!(1.20);

    Ok(EngineWalkForwardResult {
        folds,
        combined_oos_stats,
        oos_degradation_pct,
        passed,
    })
}
