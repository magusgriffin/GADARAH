use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use crate::challenge_sim::{simulate_challenge, ChallengeRules, ChallengeSimResult};
use crate::stats::{BacktestStats, TradeResult};

// ---------------------------------------------------------------------------
// Stress test: degrade trade results and re-simulate
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StressConfig {
    /// Multiply all losses by this factor (e.g. 1.5 = losses are 50% worse).
    pub loss_multiplier: Decimal,
    /// Reduce win rate by flipping this fraction of winners to losers.
    pub win_rate_reduction: Decimal,
    /// Additional slippage cost per trade in USD.
    pub extra_slippage_usd: Decimal,
}

impl Default for StressConfig {
    fn default() -> Self {
        Self {
            loss_multiplier: dec!(1.5),
            win_rate_reduction: dec!(0.10),
            extra_slippage_usd: dec!(2.0),
        }
    }
}

#[derive(Debug, Clone)]
pub struct StressResult {
    pub config: StressConfig,
    pub original_stats: BacktestStats,
    pub stressed_stats: BacktestStats,
    pub challenge_result: Option<ChallengeSimResult>,
}

/// Apply stress to trade results and recompute statistics.
///
/// This tests whether the strategy survives degraded conditions:
/// - Losses 1.5× worse (spread/slippage reality)
/// - 10% fewer winners (execution quality, market changes)
/// - Extra slippage cost per trade
pub fn run_stress_test(
    trades: &[TradeResult],
    starting_balance: Decimal,
    config: &StressConfig,
    challenge_rules: Option<&ChallengeRules>,
) -> StressResult {
    let original_stats = BacktestStats::compute(trades, starting_balance);

    let stressed_trades = apply_stress(trades, config);
    let stressed_stats = BacktestStats::compute(&stressed_trades, starting_balance);

    let challenge_result =
        challenge_rules.map(|rules| simulate_challenge(&stressed_trades, starting_balance, rules));

    StressResult {
        config: config.clone(),
        original_stats,
        stressed_stats,
        challenge_result,
    }
}

fn apply_stress(trades: &[TradeResult], config: &StressConfig) -> Vec<TradeResult> {
    let total = trades.len();
    let flip_count = (Decimal::from(total) * config.win_rate_reduction)
        .floor()
        .to_string()
        .parse::<usize>()
        .unwrap_or(0);

    let mut result: Vec<TradeResult> = trades.to_vec();

    // Step 1: Flip some winners to losers (evenly distributed)
    let mut flipped = 0usize;
    if flip_count > 0 {
        let winner_indices: Vec<usize> = result
            .iter()
            .enumerate()
            .filter(|(_, t)| t.is_winner)
            .map(|(i, _)| i)
            .collect();

        // Flip evenly spaced winners
        let step = if flip_count > 0 && winner_indices.len() > flip_count {
            winner_indices.len() / flip_count
        } else {
            1
        };

        for &idx in winner_indices.iter().step_by(step) {
            if flipped >= flip_count {
                break;
            }
            let t = &mut result[idx];
            t.is_winner = false;
            t.pnl = -t.pnl.abs();
            t.r_multiple = -t.r_multiple.abs();
            flipped += 1;
        }
    }

    // Step 2: Multiply losses
    for t in &mut result {
        if !t.is_winner {
            t.pnl *= config.loss_multiplier;
            t.r_multiple *= config.loss_multiplier;
        }
    }

    // Step 3: Apply extra slippage to all trades
    for t in &mut result {
        t.pnl -= config.extra_slippage_usd;
        // A trade that was marginally profitable might now be a loser
        if t.pnl <= Decimal::ZERO && t.is_winner {
            t.is_winner = false;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use gadarah_core::HeadId;

    fn sample_trades() -> Vec<TradeResult> {
        (0..20)
            .map(|i| {
                let is_winner = i % 3 != 0; // ~67% win rate
                TradeResult {
                    head: HeadId::Momentum,
                    pnl: if is_winner { dec!(100) } else { dec!(-50) },
                    r_multiple: if is_winner { dec!(2.0) } else { dec!(-1.0) },
                    opened_at: 1700000000 + i * 86400,
                    closed_at: 1700000000 + i * 86400 + 3600,
                    is_winner,
                }
            })
            .collect()
    }

    #[test]
    fn stress_degrades_performance() {
        let trades = sample_trades();
        let result = run_stress_test(&trades, dec!(10000), &StressConfig::default(), None);

        // Stressed should be worse
        assert!(result.stressed_stats.total_pnl < result.original_stats.total_pnl);
        assert!(result.stressed_stats.win_rate <= result.original_stats.win_rate);
    }

    #[test]
    fn stress_with_challenge() {
        let trades = sample_trades();
        let rules = ChallengeRules::ftmo_1step();
        let result = run_stress_test(&trades, dec!(10000), &StressConfig::default(), Some(&rules));

        assert!(result.challenge_result.is_some());
    }
}
