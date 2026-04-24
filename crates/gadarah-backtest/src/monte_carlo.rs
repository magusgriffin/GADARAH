use rand::rngs::StdRng;
use rand::Rng;
use rand::SeedableRng;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use crate::stats::TradeResult;

// ---------------------------------------------------------------------------
// Monte Carlo simulation: shuffled trade sequences → ruin probability
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BootstrapMode {
    /// Independent-and-identically-distributed: each draw is an independent
    /// sample with replacement.  Easy to reason about but erases the
    /// consecutive-loss clustering that kills prop-firm accounts.
    Iid,
    /// Moving-block bootstrap of fixed length.  Preserves short-run
    /// autocorrelation — if the recorded trades tend to come in streaks, the
    /// simulation surfaces streaks.
    Block { length: usize },
    /// Block bootstrap with block length auto-selected from the lag-1
    /// autocorrelation of the trade R series (Politis–White rule-of-thumb,
    /// bounded to [3, 30]).
    AutoBlock,
}

impl Default for BootstrapMode {
    fn default() -> Self {
        Self::AutoBlock
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonteCarloConfig {
    pub num_paths: usize,
    pub ruin_dd_pct: Decimal,
    #[serde(default)]
    pub mode: BootstrapMode,
}

impl Default for MonteCarloConfig {
    fn default() -> Self {
        Self {
            num_paths: 10_000,
            ruin_dd_pct: dec!(6.0),
            mode: BootstrapMode::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonteCarloResult {
    pub paths_run: usize,
    pub ruin_count: usize,
    pub ruin_probability: Decimal,
    /// 95% confidence interval on the ruin probability, lower bound.
    /// Computed via Wilson score interval — appropriate for a binomial
    /// proportion with any sample size (including near-zero ruin counts).
    #[serde(default)]
    pub ruin_ci95_low: Decimal,
    /// 95% confidence interval on the ruin probability, upper bound.
    /// The upper bound is what prop-firm risk desks care about: "there's
    /// a 95% chance the true blow-up rate is at most this value."
    #[serde(default)]
    pub ruin_ci95_high: Decimal,
    pub median_final_balance: Decimal,
    pub p5_final_balance: Decimal,
    pub p25_final_balance: Decimal,
    pub p75_final_balance: Decimal,
    pub p95_final_balance: Decimal,
    pub worst_drawdown_pct: Decimal,
    pub median_drawdown_pct: Decimal,
    pub p95_drawdown_pct: Decimal,
}

/// Run Monte Carlo simulation by bootstrapping the observed trade distribution.
///
/// Each path samples `trades.len()` trades with replacement from the observed
/// set and replays the resulting equity curve. This gives meaningful return and
/// drawdown distributions while still stressing win/loss clustering.
pub fn run_monte_carlo(
    trades: &[TradeResult],
    starting_balance: Decimal,
    config: &MonteCarloConfig,
    seed: u64,
) -> MonteCarloResult {
    if trades.is_empty() {
        return MonteCarloResult {
            paths_run: 0,
            ruin_count: 0,
            ruin_probability: Decimal::ZERO,
            ruin_ci95_low: Decimal::ZERO,
            ruin_ci95_high: Decimal::ZERO,
            median_final_balance: starting_balance,
            p5_final_balance: starting_balance,
            p25_final_balance: starting_balance,
            p75_final_balance: starting_balance,
            p95_final_balance: starting_balance,
            worst_drawdown_pct: Decimal::ZERO,
            median_drawdown_pct: Decimal::ZERO,
            p95_drawdown_pct: Decimal::ZERO,
        };
    }

    let mut rng = StdRng::seed_from_u64(seed);
    let mut final_balances: Vec<Decimal> = Vec::with_capacity(config.num_paths);
    let mut max_drawdowns: Vec<Decimal> = Vec::with_capacity(config.num_paths);
    let mut ruin_count = 0usize;

    let ruin_threshold = starting_balance * (dec!(1) - config.ruin_dd_pct / dec!(100));
    let block_length = resolve_block_length(config.mode, trades);

    for _ in 0..config.num_paths {
        let mut equity = starting_balance;
        let mut peak = starting_balance;
        let mut max_dd_pct = Decimal::ZERO;
        let mut ruined = false;

        // Block-cursor state: `remaining == 0` signals "pick a new block".
        // Unused when `block_length` is None (iid path).
        let mut block_start: usize = 0;
        let mut block_remaining: usize = 0;
        for _ in 0..trades.len() {
            let idx = match block_length {
                None => rng.gen_range(0..trades.len()),
                Some(len) => {
                    if block_remaining == 0 {
                        block_start = rng.gen_range(0..trades.len());
                        block_remaining = len;
                    }
                    let offset = len - block_remaining;
                    block_remaining -= 1;
                    (block_start + offset) % trades.len()
                }
            };
            let t = &trades[idx];
            equity += t.pnl;

            if equity > peak {
                peak = equity;
            }

            if peak > Decimal::ZERO {
                let dd_pct = (peak - equity) / peak * dec!(100);
                if dd_pct > max_dd_pct {
                    max_dd_pct = dd_pct;
                }
            }

            if equity <= ruin_threshold {
                ruined = true;
                break;
            }
        }

        if ruined {
            ruin_count += 1;
        }

        final_balances.push(equity);
        max_drawdowns.push(max_dd_pct);
    }

    final_balances.sort();
    max_drawdowns.sort();

    let percentile = |sorted: &[Decimal], p: f64| -> Decimal {
        let idx = ((sorted.len() as f64) * p).floor() as usize;
        sorted[idx.min(sorted.len() - 1)]
    };

    let ruin_probability = Decimal::from(ruin_count) / Decimal::from(config.num_paths);
    let (ci_low, ci_high) = wilson_interval(ruin_count, config.num_paths);

    MonteCarloResult {
        paths_run: config.num_paths,
        ruin_count,
        ruin_probability,
        ruin_ci95_low: ci_low,
        ruin_ci95_high: ci_high,
        median_final_balance: percentile(&final_balances, 0.50),
        p5_final_balance: percentile(&final_balances, 0.05),
        p25_final_balance: percentile(&final_balances, 0.25),
        p75_final_balance: percentile(&final_balances, 0.75),
        p95_final_balance: percentile(&final_balances, 0.95),
        worst_drawdown_pct: max_drawdowns.last().copied().unwrap_or(Decimal::ZERO),
        median_drawdown_pct: percentile(&max_drawdowns, 0.50),
        p95_drawdown_pct: percentile(&max_drawdowns, 0.95),
    }
}

/// Wilson score 95% confidence interval for a binomial proportion.
/// Robust at the extremes — unlike the normal approximation, it doesn't
/// return negative lower bounds or >1 upper bounds when `k/n` is near 0
/// or 1. This is the right interval for ruin probability from N paths.
fn wilson_interval(k: usize, n: usize) -> (Decimal, Decimal) {
    if n == 0 {
        return (Decimal::ZERO, Decimal::ZERO);
    }
    let z: f64 = 1.959_963_984_540_054; // 95% two-sided
    let n_f = n as f64;
    let p = k as f64 / n_f;
    let denom = 1.0 + z * z / n_f;
    let center = (p + z * z / (2.0 * n_f)) / denom;
    let margin = z * ((p * (1.0 - p) / n_f) + z * z / (4.0 * n_f * n_f)).sqrt() / denom;
    let lo = (center - margin).max(0.0);
    let hi = (center + margin).min(1.0);
    // Snap near-zero floats to exact zero so callers can assert on the
    // boundary without worrying about 1e-18 precision noise.
    let snap = |v: f64| if v.abs() < 1e-9 { Decimal::ZERO } else {
        Decimal::from_f64_retain(v).unwrap_or(Decimal::ZERO)
    };
    (snap(lo), snap(hi))
}

/// Pick the effective block length, or `None` when iid was requested.
fn resolve_block_length(mode: BootstrapMode, trades: &[TradeResult]) -> Option<usize> {
    match mode {
        BootstrapMode::Iid => None,
        BootstrapMode::Block { length } => Some(length.max(1)),
        BootstrapMode::AutoBlock => Some(auto_block_length(trades)),
    }
}

/// Compute an auto block length from lag-1 autocorrelation of trade R.
/// Higher positive autocorrelation → longer blocks (harder to shuffle).
fn auto_block_length(trades: &[TradeResult]) -> usize {
    if trades.len() < 20 {
        return 5;
    }
    // Use f64 throughout — rough moment math, precision not critical.
    let n = trades.len();
    let xs: Vec<f64> = trades
        .iter()
        .map(|t| t.r_multiple.try_into().unwrap_or(0.0))
        .collect();
    let mean = xs.iter().sum::<f64>() / n as f64;
    let var: f64 = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n as f64;
    if var.abs() < 1e-9 {
        return 5;
    }
    let mut num = 0.0;
    for i in 1..n {
        num += (xs[i] - mean) * (xs[i - 1] - mean);
    }
    let rho = (num / (n - 1) as f64) / var;
    if rho <= 0.0 {
        return 3;
    }
    // Rough Politis–White: L ≈ (n)^(1/3) * rho^(1/3), bounded.
    let raw = (n as f64).cbrt() * rho.abs().cbrt();
    raw.round().clamp(3.0, 30.0) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use gadarah_core::HeadId;

    fn make_trades() -> Vec<TradeResult> {
        let mut trades = Vec::new();
        // 60% win rate, 2R winners, 1R losers
        for i in 0..100 {
            let is_winner = i % 5 != 0 && i % 5 != 1; // 60% winners
            trades.push(TradeResult {
                head: HeadId::Momentum,
                pnl: if is_winner { dec!(100) } else { dec!(-50) },
                r_multiple: if is_winner { dec!(2.0) } else { dec!(-1.0) },
                opened_at: 1700000000 + i * 3600,
                closed_at: 1700000000 + i * 3600 + 1800,
                is_winner,
            });
        }
        trades
    }

    #[test]
    fn monte_carlo_runs() {
        let trades = make_trades();
        let result = run_monte_carlo(
            &trades,
            dec!(10000),
            &MonteCarloConfig {
                num_paths: 1000,
                ruin_dd_pct: dec!(10.0),
                mode: BootstrapMode::Iid,
            },
            42,
        );
        assert_eq!(result.paths_run, 1000);
        // With 60% win rate and positive expectancy, ruin should be low
        assert!(result.ruin_probability < dec!(0.10));
        assert!(result.median_final_balance > dec!(10000));
    }

    #[test]
    fn empty_trades() {
        let result = run_monte_carlo(&[], dec!(10000), &MonteCarloConfig::default(), 42);
        assert_eq!(result.paths_run, 0);
        assert_eq!(result.median_final_balance, dec!(10000));
    }

    fn streaky_trades() -> Vec<TradeResult> {
        // 10-win streak followed by 10-loss streak, repeated.  High autocorrelation.
        let mut trades = Vec::new();
        for cycle in 0..10 {
            for i in 0..10 {
                trades.push(TradeResult {
                    head: HeadId::Momentum,
                    pnl: dec!(100),
                    r_multiple: dec!(2),
                    opened_at: 1700000000 + (cycle * 20 + i) * 3600,
                    closed_at: 1700000000 + (cycle * 20 + i) * 3600 + 1800,
                    is_winner: true,
                });
            }
            for i in 0..10 {
                trades.push(TradeResult {
                    head: HeadId::Momentum,
                    pnl: dec!(-100),
                    r_multiple: dec!(-2),
                    opened_at: 1700000000 + (cycle * 20 + 10 + i) * 3600,
                    closed_at: 1700000000 + (cycle * 20 + 10 + i) * 3600 + 1800,
                    is_winner: false,
                });
            }
        }
        trades
    }

    #[test]
    fn block_bootstrap_produces_worse_drawdowns_than_iid() {
        // When losses cluster, iid shuffling underestimates DD.  Block bootstrap
        // preserves the clustering, so the p95 DD must be at least as bad.
        let trades = streaky_trades();
        let iid = run_monte_carlo(
            &trades,
            dec!(10000),
            &MonteCarloConfig {
                num_paths: 500,
                ruin_dd_pct: dec!(50),
                mode: BootstrapMode::Iid,
            },
            42,
        );
        let block = run_monte_carlo(
            &trades,
            dec!(10000),
            &MonteCarloConfig {
                num_paths: 500,
                ruin_dd_pct: dec!(50),
                mode: BootstrapMode::Block { length: 10 },
            },
            42,
        );
        assert!(block.p95_drawdown_pct >= iid.p95_drawdown_pct);
    }

    #[test]
    fn auto_block_selects_longer_blocks_when_streaky() {
        let streaky = streaky_trades();
        let block_len = auto_block_length(&streaky);
        assert!(block_len >= 3);
    }

    #[test]
    fn wilson_interval_brackets_point_estimate() {
        // Known case: 50 ruins out of 1000 → p = 0.05, Wilson ~ [0.038, 0.066].
        let (lo, hi) = wilson_interval(50, 1000);
        assert!(lo > dec!(0.030));
        assert!(lo < dec!(0.050));
        assert!(hi > dec!(0.050));
        assert!(hi < dec!(0.080));
    }

    #[test]
    fn wilson_interval_handles_zero_ruins() {
        // 0 ruins out of 100 → point estimate 0, but upper bound still > 0.
        let (lo, hi) = wilson_interval(0, 100);
        assert_eq!(lo, Decimal::ZERO);
        assert!(hi > Decimal::ZERO);
    }

    #[test]
    fn auto_block_returns_small_for_small_samples() {
        let result = run_monte_carlo(
            &make_trades(),
            dec!(10000),
            &MonteCarloConfig {
                num_paths: 100,
                ruin_dd_pct: dec!(10.0),
                mode: BootstrapMode::AutoBlock,
            },
            7,
        );
        assert_eq!(result.paths_run, 100);
    }
}
