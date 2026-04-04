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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonteCarloConfig {
    pub num_paths: usize,
    pub ruin_dd_pct: Decimal,
}

impl Default for MonteCarloConfig {
    fn default() -> Self {
        Self {
            num_paths: 10_000,
            ruin_dd_pct: dec!(6.0),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonteCarloResult {
    pub paths_run: usize,
    pub ruin_count: usize,
    pub ruin_probability: Decimal,
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

    for _ in 0..config.num_paths {
        let mut equity = starting_balance;
        let mut peak = starting_balance;
        let mut max_dd_pct = Decimal::ZERO;
        let mut ruined = false;

        for _ in 0..trades.len() {
            let idx = rng.gen_range(0..trades.len());
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

    MonteCarloResult {
        paths_run: config.num_paths,
        ruin_count,
        ruin_probability: Decimal::from(ruin_count) / Decimal::from(config.num_paths),
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
}
