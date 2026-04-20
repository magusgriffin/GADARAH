use std::ops::Range;

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
    /// Number of folds (e.g. 12 for 12-fold).
    pub num_folds: usize,
    /// Fraction of each fold used for in-sample (the rest is out-of-sample).
    /// E.g. 0.70 means 70% in-sample, 30% out-of-sample.
    pub in_sample_ratio: f64,
    /// Number of bars skipped between the in-sample and out-of-sample window.
    ///
    /// Defaults to one-week at M5 (7 * 24 * 12 = 2016 bars).  A non-zero
    /// embargo prevents overlapping signal state — a feature computed at the
    /// end of IS cannot bleed into the start of OOS, which is what makes
    /// walk-forward results honest on bar-by-bar strategies.
    pub embargo_bars: usize,
    /// Minimum OOS Sharpe a fold must hit to count as "passing".
    pub min_fold_sharpe: Decimal,
    /// Maximum IS→OOS degradation (as pct of IS profit factor) allowed.
    pub max_oos_degradation_pct: Decimal,
}

impl Default for WalkForwardConfig {
    fn default() -> Self {
        Self {
            num_folds: 12,
            in_sample_ratio: 0.70,
            embargo_bars: 7 * 24 * 12, // 1 week of M5
            min_fold_sharpe: dec!(0.5),
            max_oos_degradation_pct: dec!(40),
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

/// One (in-sample, out-of-sample) split produced by walking the bar buffer.
/// Embargo skips bars between IS and OOS so indicator state doesn't leak across
/// the boundary (Lopez de Prado §7.4.2 "purging with embargo").
struct FoldRanges {
    fold_index: usize,
    in_sample: Range<usize>,
    out_of_sample: Range<usize>,
}

fn compute_fold_ranges(
    total_bars: usize,
    wf: &WalkForwardConfig,
) -> Result<Vec<FoldRanges>, BacktestError> {
    if wf.num_folds < 2 {
        return Err(BacktestError::WalkForward(
            "need at least 2 folds".to_string(),
        ));
    }
    let fold_size = total_bars / wf.num_folds;
    if fold_size < 200 {
        return Err(BacktestError::InsufficientBars {
            needed: 200 * wf.num_folds,
            got: total_bars,
        });
    }
    let is_size = (fold_size as f64 * wf.in_sample_ratio) as usize;
    if is_size <= wf.embargo_bars {
        return Err(BacktestError::WalkForward(format!(
            "embargo ({}) exceeds in-sample size ({}); reduce embargo or grow folds",
            wf.embargo_bars, is_size,
        )));
    }

    let mut out = Vec::with_capacity(wf.num_folds);
    for fold_index in 0..wf.num_folds {
        let start = fold_index * fold_size;
        let is_end = start + is_size;
        let oos_start = is_end + wf.embargo_bars;
        let oos_end = (start + fold_size).min(total_bars);

        if is_end >= total_bars || oos_start >= total_bars || oos_end > total_bars {
            break;
        }
        if oos_start >= oos_end {
            break;
        }
        out.push(FoldRanges {
            fold_index,
            in_sample: start..is_end,
            out_of_sample: oos_start..oos_end,
        });
    }
    Ok(out)
}

fn oos_degradation(avg_is_pf: Decimal, combined_oos_pf: Decimal) -> Decimal {
    if avg_is_pf > Decimal::ZERO {
        (avg_is_pf - combined_oos_pf) / avg_is_pf * dec!(100)
    } else {
        dec!(100)
    }
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
    let ranges = compute_fold_ranges(bars.len(), wf_config)?;

    let mut folds = Vec::with_capacity(ranges.len());
    let mut all_oos_trades = Vec::new();

    for r in ranges {
        let is_bars = &bars[r.in_sample.clone()];
        let oos_bars = &bars[r.out_of_sample.clone()];

        let mut is_heads = head_factory();
        let is_result = run_replay(is_bars, &mut is_heads, replay_config)?;

        let mut oos_heads = head_factory();
        let oos_result = run_replay(oos_bars, &mut oos_heads, replay_config)?;

        all_oos_trades.extend(oos_result.trades.clone());

        folds.push(FoldResult {
            fold_index: r.fold_index,
            in_sample_bars: is_bars.len(),
            out_of_sample_bars: oos_bars.len(),
            in_sample_stats: is_result.stats,
            out_of_sample_stats: oos_result.stats,
        });
    }

    let combined_oos_stats =
        BacktestStats::compute(&all_oos_trades, replay_config.starting_balance);

    let avg_is_pf = if folds.is_empty() {
        Decimal::ONE
    } else {
        folds
            .iter()
            .map(|f| f.in_sample_stats.profit_factor)
            .sum::<Decimal>()
            / Decimal::from(folds.len())
    };
    let oos_degradation_pct = oos_degradation(avg_is_pf, combined_oos_stats.profit_factor);

    let fold_sharpes_ok = folds
        .iter()
        .all(|f| f.out_of_sample_stats.sharpe_ratio >= wf_config.min_fold_sharpe);
    let passed = !folds.is_empty()
        && combined_oos_stats.profit_factor > Decimal::ONE
        && oos_degradation_pct < wf_config.max_oos_degradation_pct
        && fold_sharpes_ok;

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
    let ranges = compute_fold_ranges(bars.len(), wf_config)?;

    let mut folds = Vec::with_capacity(ranges.len());
    let mut all_oos_trades = Vec::new();

    for r in ranges {
        let is_bars = &bars[r.in_sample.clone()];
        let oos_bars = &bars[r.out_of_sample.clone()];

        let mut is_heads = head_factory();
        let is_result = run_engine(is_bars, &mut is_heads, engine_config)?;

        let mut oos_heads = head_factory();
        let oos_result = run_engine(oos_bars, &mut oos_heads, engine_config)?;

        all_oos_trades.extend(oos_result.trades.clone());

        folds.push(EngineFoldResult {
            fold_index: r.fold_index,
            in_sample_bars: is_bars.len(),
            out_of_sample_bars: oos_bars.len(),
            in_sample_result: is_result,
            out_of_sample_result: oos_result,
        });
    }

    let combined_oos_stats =
        BacktestStats::compute(&all_oos_trades, engine_config.starting_balance);

    let avg_is_pf = if folds.is_empty() {
        Decimal::ONE
    } else {
        folds
            .iter()
            .map(|f| f.in_sample_result.stats.profit_factor)
            .sum::<Decimal>()
            / Decimal::from(folds.len())
    };
    let oos_degradation_pct = oos_degradation(avg_is_pf, combined_oos_stats.profit_factor);

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
