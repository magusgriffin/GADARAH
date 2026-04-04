use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use gadarah_core::HeadId;

// ---------------------------------------------------------------------------
// Trade result (lightweight, for stats computation)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeResult {
    pub head: HeadId,
    pub pnl: Decimal,
    pub r_multiple: Decimal,
    pub opened_at: i64,
    pub closed_at: i64,
    pub is_winner: bool,
}

// ---------------------------------------------------------------------------
// Backtest statistics
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BacktestStats {
    pub total_trades: usize,
    pub winners: usize,
    pub losers: usize,
    pub win_rate: Decimal,
    pub total_pnl: Decimal,
    pub avg_winner: Decimal,
    pub avg_loser: Decimal,
    pub avg_r_multiple: Decimal,
    pub max_r: Decimal,
    pub min_r: Decimal,
    pub profit_factor: Decimal,
    pub max_consecutive_wins: usize,
    pub max_consecutive_losses: usize,
    pub max_drawdown_pct: Decimal,
    pub max_drawdown_usd: Decimal,
    pub sharpe_ratio: Decimal,
    pub expectancy_r: Decimal,
    pub starting_balance: Decimal,
    pub ending_balance: Decimal,
    pub return_pct: Decimal,
    pub trading_days: usize,
}

impl BacktestStats {
    /// Compute statistics from a sequence of trade results.
    pub fn compute(trades: &[TradeResult], starting_balance: Decimal) -> Self {
        if trades.is_empty() {
            return Self {
                starting_balance,
                ending_balance: starting_balance,
                ..Default::default()
            };
        }

        let total_trades = trades.len();
        let winners: Vec<_> = trades.iter().filter(|t| t.is_winner).collect();
        let losers: Vec<_> = trades.iter().filter(|t| !t.is_winner).collect();
        let win_count = winners.len();
        let loss_count = losers.len();

        let total_pnl: Decimal = trades.iter().map(|t| t.pnl).sum();
        let total_r: Decimal = trades.iter().map(|t| t.r_multiple).sum();

        let avg_winner = if win_count > 0 {
            winners.iter().map(|t| t.pnl).sum::<Decimal>() / Decimal::from(win_count)
        } else {
            Decimal::ZERO
        };

        let avg_loser = if loss_count > 0 {
            losers.iter().map(|t| t.pnl).sum::<Decimal>() / Decimal::from(loss_count)
        } else {
            Decimal::ZERO
        };

        let win_rate = if total_trades > 0 {
            Decimal::from(win_count) / Decimal::from(total_trades)
        } else {
            Decimal::ZERO
        };

        let gross_profit: Decimal = winners.iter().map(|t| t.pnl).sum();
        let gross_loss: Decimal = losers.iter().map(|t| t.pnl.abs()).sum();
        let profit_factor = if gross_loss > Decimal::ZERO {
            gross_profit / gross_loss
        } else if gross_profit > Decimal::ZERO {
            dec!(999.99)
        } else {
            Decimal::ZERO
        };

        let max_r = trades
            .iter()
            .map(|t| t.r_multiple)
            .max()
            .unwrap_or(Decimal::ZERO);
        let min_r = trades
            .iter()
            .map(|t| t.r_multiple)
            .min()
            .unwrap_or(Decimal::ZERO);
        let avg_r = total_r / Decimal::from(total_trades);

        // Consecutive wins/losses
        let (max_con_wins, max_con_losses) = consecutive_streaks(trades);

        // Drawdown
        let (max_dd_pct, max_dd_usd) = compute_drawdown(trades, starting_balance);

        // Sharpe ratio — annualised using daily PnL returns.
        // Trades are grouped by the calendar day they were opened; the daily
        // return is that day's net PnL divided by the running equity at the
        // start of that day. The ratio is then annualised by √252.
        let sharpe = compute_sharpe_daily(trades, starting_balance);

        // Expectancy in R
        let avg_win_r = if win_count > 0 {
            winners.iter().map(|t| t.r_multiple).sum::<Decimal>() / Decimal::from(win_count)
        } else {
            Decimal::ZERO
        };
        let avg_loss_r = if loss_count > 0 {
            losers.iter().map(|t| t.r_multiple.abs()).sum::<Decimal>() / Decimal::from(loss_count)
        } else {
            Decimal::ZERO
        };
        let expectancy_r = win_rate * avg_win_r - (dec!(1) - win_rate) * avg_loss_r;

        let ending_balance = starting_balance + total_pnl;
        let return_pct = if starting_balance > Decimal::ZERO {
            total_pnl / starting_balance * dec!(100)
        } else {
            Decimal::ZERO
        };

        // Trading days (unique days)
        let trading_days = count_unique_days(trades);

        BacktestStats {
            total_trades,
            winners: win_count,
            losers: loss_count,
            win_rate,
            total_pnl,
            avg_winner,
            avg_loser,
            avg_r_multiple: avg_r,
            max_r,
            min_r,
            profit_factor,
            max_consecutive_wins: max_con_wins,
            max_consecutive_losses: max_con_losses,
            max_drawdown_pct: max_dd_pct,
            max_drawdown_usd: max_dd_usd,
            sharpe_ratio: sharpe,
            expectancy_r,
            starting_balance,
            ending_balance,
            return_pct,
            trading_days,
        }
    }
}

fn consecutive_streaks(trades: &[TradeResult]) -> (usize, usize) {
    let mut max_wins = 0usize;
    let mut max_losses = 0usize;
    let mut cur_wins = 0usize;
    let mut cur_losses = 0usize;

    for t in trades {
        if t.is_winner {
            cur_wins += 1;
            cur_losses = 0;
            max_wins = max_wins.max(cur_wins);
        } else {
            cur_losses += 1;
            cur_wins = 0;
            max_losses = max_losses.max(cur_losses);
        }
    }

    (max_wins, max_losses)
}

fn compute_drawdown(trades: &[TradeResult], starting_balance: Decimal) -> (Decimal, Decimal) {
    let mut equity = starting_balance;
    let mut peak = starting_balance;
    let mut max_dd_usd = Decimal::ZERO;
    let mut max_dd_pct = Decimal::ZERO;

    for t in trades {
        equity += t.pnl;
        if equity > peak {
            peak = equity;
        }
        let dd_usd = peak - equity;
        if dd_usd > max_dd_usd {
            max_dd_usd = dd_usd;
        }
        if peak > Decimal::ZERO {
            let dd_pct = dd_usd / peak * dec!(100);
            if dd_pct > max_dd_pct {
                max_dd_pct = dd_pct;
            }
        }
    }

    (max_dd_pct, max_dd_usd)
}

/// Annualised Sharpe ratio using daily return series.
///
/// Groups trades by the calendar day they were opened (UTC), computes each
/// day's net PnL relative to the running equity at that day's open, then
/// applies the standard Sharpe formula × √252.
///
/// Falls back to Decimal::ZERO if fewer than 2 distinct trading days exist.
fn compute_sharpe_daily(trades: &[TradeResult], starting_balance: Decimal) -> Decimal {
    if trades.is_empty() {
        return Decimal::ZERO;
    }

    // Aggregate PnL per day (unix day number = timestamp / 86400).
    let mut day_pnl: std::collections::BTreeMap<i64, Decimal> = std::collections::BTreeMap::new();
    for t in trades {
        let day = t.opened_at / 86400;
        *day_pnl.entry(day).or_insert(Decimal::ZERO) += t.pnl;
    }

    if day_pnl.len() < 2 {
        return Decimal::ZERO;
    }

    // Compute daily returns as PnL / running_equity_at_day_start.
    let mut daily_returns: Vec<Decimal> = Vec::with_capacity(day_pnl.len());
    let mut running_equity = starting_balance;
    for (_day, pnl) in &day_pnl {
        if running_equity.is_zero() {
            break;
        }
        daily_returns.push(*pnl / running_equity);
        running_equity += *pnl;
    }

    let n = Decimal::from(daily_returns.len());
    if n < dec!(2) {
        return Decimal::ZERO;
    }

    let mean = daily_returns.iter().sum::<Decimal>() / n;
    let variance = daily_returns
        .iter()
        .map(|r| (*r - mean) * (*r - mean))
        .sum::<Decimal>()
        / (n - dec!(1));

    if variance <= Decimal::ZERO {
        return Decimal::ZERO;
    }

    let std_dev = gadarah_core::decimal_sqrt(variance);
    if std_dev.is_zero() {
        return Decimal::ZERO;
    }

    // Annualise: multiply by √252 (trading days per year).
    let sqrt_252 = gadarah_core::decimal_sqrt(dec!(252));
    (mean / std_dev) * sqrt_252
}

fn count_unique_days(trades: &[TradeResult]) -> usize {
    let mut days: Vec<i64> = trades.iter().map(|t| t.opened_at / 86400).collect();
    days.sort_unstable();
    days.dedup();
    days.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gadarah_core::HeadId;

    fn trade(pnl: Decimal, r: Decimal, winner: bool, day_offset: i64) -> TradeResult {
        TradeResult {
            head: HeadId::Momentum,
            pnl,
            r_multiple: r,
            opened_at: 1700000000 + day_offset * 86400,
            closed_at: 1700000000 + day_offset * 86400 + 3600,
            is_winner: winner,
        }
    }

    #[test]
    fn stats_basic() {
        let trades = vec![
            trade(dec!(100), dec!(2.0), true, 0),
            trade(dec!(-50), dec!(-1.0), false, 1),
            trade(dec!(75), dec!(1.5), true, 2),
            trade(dec!(-50), dec!(-1.0), false, 3),
            trade(dec!(150), dec!(3.0), true, 4),
        ];

        let stats = BacktestStats::compute(&trades, dec!(10000));
        assert_eq!(stats.total_trades, 5);
        assert_eq!(stats.winners, 3);
        assert_eq!(stats.losers, 2);
        assert!(stats.win_rate > dec!(0.59) && stats.win_rate < dec!(0.61));
        assert_eq!(stats.total_pnl, dec!(225));
        assert!(stats.profit_factor > dec!(1));
        assert_eq!(stats.max_consecutive_wins, 1);
        assert_eq!(stats.max_consecutive_losses, 1);
        assert_eq!(stats.trading_days, 5);
        assert_eq!(stats.ending_balance, dec!(10225));
    }

    #[test]
    fn drawdown_calculation() {
        let trades = vec![
            trade(dec!(200), dec!(2.0), true, 0),
            trade(dec!(-100), dec!(-1.0), false, 1),
            trade(dec!(-100), dec!(-1.0), false, 2),
            trade(dec!(300), dec!(3.0), true, 3),
        ];

        let stats = BacktestStats::compute(&trades, dec!(10000));
        // Peak after first trade: 10200. DD after losses: 10000. DD = 200
        assert_eq!(stats.max_drawdown_usd, dec!(200));
    }

    #[test]
    fn empty_trades() {
        let stats = BacktestStats::compute(&[], dec!(10000));
        assert_eq!(stats.total_trades, 0);
        assert_eq!(stats.ending_balance, dec!(10000));
    }
}
