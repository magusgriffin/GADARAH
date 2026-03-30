use std::collections::{HashMap, HashSet};

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use gadarah_core::utc_day;

use crate::stats::TradeResult;

// ---------------------------------------------------------------------------
// Challenge Simulator: replay trades against prop firm rules
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DailyDrawdownMode {
    DayStartBalance,
    EndOfDayHighWatermark,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum OvernightEquityMode {
    ClosedTradesOnly,
    FullTradePnlAtRollover,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeStageRules {
    /// Stage name (e.g. "Challenge", "Verification").
    pub name: String,
    /// Target profit % to pass this stage.
    pub target_pct: Decimal,
    /// Maximum daily loss % of challenge size.
    pub daily_dd_limit_pct: Decimal,
    /// Maximum total loss % of challenge size.
    pub max_dd_limit_pct: Decimal,
    /// Minimum qualifying trading days required.
    pub min_trading_days: u32,
    /// Whether total DD trails the stage equity peak.
    pub trailing_dd: bool,
    /// Max single-day profit as % of total profit (0 = disabled).
    pub consistency_cap_pct: Decimal,
    /// How the daily loss anchor is set.
    pub daily_dd_mode: DailyDrawdownMode,
    /// How unrealized overnight equity is approximated at rollover.
    pub overnight_equity_mode: OvernightEquityMode,
    /// Minimum hold time for a trade to count toward trading-day minimums.
    pub min_trade_duration_secs_for_day_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeRules {
    pub name: String,
    pub stages: Vec<ChallengeStageRules>,
}

impl ChallengeRules {
    /// FTMO 1-Step style.
    pub fn ftmo_1step() -> Self {
        Self {
            name: "FTMO 1-Step".into(),
            stages: vec![ChallengeStageRules {
                name: "Evaluation".into(),
                target_pct: dec!(10.0),
                daily_dd_limit_pct: dec!(5.0),
                max_dd_limit_pct: dec!(10.0),
                min_trading_days: 4,
                trailing_dd: false,
                consistency_cap_pct: Decimal::ZERO,
                daily_dd_mode: DailyDrawdownMode::DayStartBalance,
                overnight_equity_mode: OvernightEquityMode::ClosedTradesOnly,
                min_trade_duration_secs_for_day_count: 0,
            }],
        }
    }

    /// BrightFunded evaluation as of March 30, 2026.
    ///
    /// The official evaluation is a 2-stage process:
    /// - Challenge: 8% target, 5 min trading days
    /// - Verification: 5% target, 5 min trading days
    /// - Daily loss: 5% of challenge size
    /// - Max loss: 10% of challenge size
    ///
    /// Note: the official daily loss is anchored to the end-of-day high watermark
    /// between balance and equity. This simulator only sees closed trades, so the
    /// watermark is approximated conservatively by staking the full eventual PnL
    /// of trades still open at rollover into the next day's breach level.
    pub fn brightfunded_evaluation() -> Self {
        let stage = |name: &str, target_pct: Decimal| ChallengeStageRules {
            name: name.into(),
            target_pct,
            daily_dd_limit_pct: dec!(5.0),
            max_dd_limit_pct: dec!(10.0),
            min_trading_days: 5,
            trailing_dd: false,
            consistency_cap_pct: Decimal::ZERO,
            daily_dd_mode: DailyDrawdownMode::EndOfDayHighWatermark,
            overnight_equity_mode: OvernightEquityMode::FullTradePnlAtRollover,
            min_trade_duration_secs_for_day_count: 60,
        };

        Self {
            name: "BrightFunded Evaluation".into(),
            stages: vec![
                stage("Challenge", dec!(8.0)),
                stage("Verification", dec!(5.0)),
            ],
        }
    }

    /// 2-Step Pro with consistency rule.
    pub fn two_step_pro() -> Self {
        Self {
            name: "2-Step Pro".into(),
            stages: vec![ChallengeStageRules {
                name: "Evaluation".into(),
                target_pct: dec!(6.0),
                daily_dd_limit_pct: dec!(3.0),
                max_dd_limit_pct: dec!(6.0),
                min_trading_days: 3,
                trailing_dd: false,
                consistency_cap_pct: dec!(45.0),
                daily_dd_mode: DailyDrawdownMode::DayStartBalance,
                overnight_equity_mode: OvernightEquityMode::ClosedTradesOnly,
                min_trade_duration_secs_for_day_count: 0,
            }],
        }
    }
}

// ---------------------------------------------------------------------------
// Simulation result
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeStageSimResult {
    pub stage_rules: ChallengeStageRules,
    pub starting_balance: Decimal,
    pub final_balance: Decimal,
    pub profit_pct: Decimal,
    pub target_reached: bool,
    pub daily_dd_breached: bool,
    pub max_dd_breached: bool,
    pub min_days_met: bool,
    pub consistency_met: bool,
    pub passed: bool,
    pub days_to_target: Option<u32>,
    pub trading_days: u32,
    pub total_trades: usize,
    pub max_daily_dd_pct: Decimal,
    pub max_total_dd_pct: Decimal,
    pub breach_reason: Option<String>,
    pub trades_consumed: usize,
    pub completion_day: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeSimResult {
    pub rules: ChallengeRules,
    pub stage_results: Vec<ChallengeStageSimResult>,
    pub starting_balance: Decimal,
    pub final_balance: Decimal,
    pub profit_pct: Decimal,
    pub target_reached: bool,
    pub daily_dd_breached: bool,
    pub max_dd_breached: bool,
    pub min_days_met: bool,
    pub consistency_met: bool,
    pub passed: bool,
    pub days_to_target: Option<u32>,
    pub trading_days: u32,
    pub total_trades: usize,
    pub max_daily_dd_pct: Decimal,
    pub max_total_dd_pct: Decimal,
    pub breach_reason: Option<String>,
    pub completed_stages: usize,
}

/// Simulate a prop firm challenge using pre-computed trade results.
///
/// Multi-stage evaluations consume trades sequentially and reset the balance at
/// each new stage, which matches how a new evaluation account starts after
/// passing the prior stage. A later stage begins on the next trading day after
/// the prior stage passes.
pub fn simulate_challenge(
    trades: &[TradeResult],
    starting_balance: Decimal,
    rules: &ChallengeRules,
) -> ChallengeSimResult {
    let mut stage_results = Vec::with_capacity(rules.stages.len());
    let mut trade_offset = 0usize;

    for stage_rules in &rules.stages {
        let stage_result = simulate_stage(&trades[trade_offset..], starting_balance, stage_rules);
        let completion_day = stage_result.completion_day;
        let stage_consumed = stage_result.trades_consumed;
        let stage_passed = stage_result.passed;

        trade_offset += stage_consumed;
        stage_results.push(stage_result);

        if !stage_passed {
            break;
        }

        if let Some(day) = completion_day {
            while trade_offset < trades.len() && utc_day(trades[trade_offset].closed_at) == day {
                trade_offset += 1;
            }
        }
    }

    let completed_stages = stage_results.iter().filter(|stage| stage.passed).count();
    let passed = completed_stages == rules.stages.len();
    let headline = stage_results.last();

    let final_balance = headline
        .map(|stage| stage.final_balance)
        .unwrap_or(starting_balance);
    let profit_pct = headline
        .map(|stage| stage.profit_pct)
        .unwrap_or(Decimal::ZERO);
    let target_reached =
        !rules.stages.is_empty() && stage_results.len() == rules.stages.len() && passed;
    let daily_dd_breached = stage_results.iter().any(|stage| stage.daily_dd_breached);
    let max_dd_breached = stage_results.iter().any(|stage| stage.max_dd_breached);
    let min_days_met = stage_results.iter().all(|stage| stage.min_days_met);
    let consistency_met = stage_results.iter().all(|stage| stage.consistency_met);
    let days_to_target = if passed {
        Some(
            stage_results
                .iter()
                .filter_map(|stage| stage.days_to_target)
                .sum(),
        )
    } else {
        headline.and_then(|stage| stage.days_to_target)
    };
    let trading_days = stage_results.iter().map(|stage| stage.trading_days).sum();
    let total_trades = stage_results.iter().map(|stage| stage.total_trades).sum();
    let max_daily_dd_pct = stage_results
        .iter()
        .map(|stage| stage.max_daily_dd_pct)
        .max()
        .unwrap_or(Decimal::ZERO);
    let max_total_dd_pct = stage_results
        .iter()
        .map(|stage| stage.max_total_dd_pct)
        .max()
        .unwrap_or(Decimal::ZERO);
    let breach_reason = first_failure_reason(&stage_results);

    ChallengeSimResult {
        rules: rules.clone(),
        stage_results,
        starting_balance,
        final_balance,
        profit_pct,
        target_reached,
        daily_dd_breached,
        max_dd_breached,
        min_days_met,
        consistency_met,
        passed,
        days_to_target,
        trading_days,
        total_trades,
        max_daily_dd_pct,
        max_total_dd_pct,
        breach_reason,
        completed_stages,
    }
}

fn simulate_stage(
    trades: &[TradeResult],
    starting_balance: Decimal,
    rules: &ChallengeStageRules,
) -> ChallengeStageSimResult {
    let mut balance = starting_balance;
    let mut high_water_mark = starting_balance;
    let mut current_day = trades.first().map_or(0, |trade| utc_day(trade.closed_at));
    let mut daily_reference_balance = match rules.daily_dd_mode {
        DailyDrawdownMode::DayStartBalance => starting_balance,
        DailyDrawdownMode::EndOfDayHighWatermark => end_of_day_high_watermark(
            trades,
            current_day,
            starting_balance,
            rules.overnight_equity_mode,
        ),
    };

    let mut max_daily_dd_pct = Decimal::ZERO;
    let mut max_total_dd_pct = Decimal::ZERO;
    let mut daily_dd_breached = false;
    let mut max_dd_breached = false;
    let mut target_reached = false;
    let mut passed = false;
    let mut days_to_target = None;
    let mut breach_reason = None;
    let mut trades_consumed = 0usize;
    let mut completion_day = None;

    let daily_limit_amount = starting_balance * rules.daily_dd_limit_pct / dec!(100);

    let mut trading_day_set = HashSet::new();
    let mut daily_pnl_map: HashMap<i64, Decimal> = HashMap::new();

    for (idx, trade) in trades.iter().enumerate() {
        let trade_day = utc_day(trade.closed_at);

        if trade_day != current_day {
            current_day = trade_day;
            daily_reference_balance = match rules.daily_dd_mode {
                DailyDrawdownMode::DayStartBalance => balance,
                DailyDrawdownMode::EndOfDayHighWatermark => end_of_day_high_watermark(
                    trades,
                    trade_day,
                    balance,
                    rules.overnight_equity_mode,
                ),
            };
        }

        if trade.closed_at - trade.opened_at >= rules.min_trade_duration_secs_for_day_count {
            trading_day_set.insert(trade_day);
        }

        balance += trade.pnl;
        *daily_pnl_map.entry(trade_day).or_insert(Decimal::ZERO) += trade.pnl;
        trades_consumed = idx + 1;

        if balance > high_water_mark {
            high_water_mark = balance;
        }

        let daily_loss_amount = daily_reference_balance - balance;
        let daily_dd_pct = if starting_balance > Decimal::ZERO {
            daily_loss_amount / starting_balance * dec!(100)
        } else {
            Decimal::ZERO
        };
        if daily_dd_pct > max_daily_dd_pct {
            max_daily_dd_pct = daily_dd_pct;
        }
        if daily_loss_amount >= daily_limit_amount && !daily_dd_breached {
            daily_dd_breached = true;
            breach_reason = Some(format!(
                "{} daily DD {:.2}% >= {:.2}% on day {}",
                rules.name, daily_dd_pct, rules.daily_dd_limit_pct, trade_day
            ));
        }

        let total_dd_base = if rules.trailing_dd {
            high_water_mark
        } else {
            starting_balance
        };
        let total_dd = total_dd_base - balance;
        let total_dd_pct = if total_dd_base > Decimal::ZERO {
            total_dd / total_dd_base * dec!(100)
        } else {
            Decimal::ZERO
        };
        if total_dd_pct > max_total_dd_pct {
            max_total_dd_pct = total_dd_pct;
        }
        if total_dd_pct >= rules.max_dd_limit_pct && !max_dd_breached {
            max_dd_breached = true;
            breach_reason = Some(format!(
                "{} total DD {:.2}% >= {:.2}% on day {}",
                rules.name, total_dd_pct, rules.max_dd_limit_pct, trade_day
            ));
        }

        if daily_dd_breached || max_dd_breached {
            completion_day = Some(trade_day);
            break;
        }

        let profit_pct = if starting_balance > Decimal::ZERO {
            (balance - starting_balance) / starting_balance * dec!(100)
        } else {
            Decimal::ZERO
        };

        if profit_pct >= rules.target_pct && !target_reached {
            target_reached = true;
            days_to_target = Some(trading_day_set.len() as u32);
        }

        let min_days_met = trading_day_set.len() as u32 >= rules.min_trading_days;
        let consistency_met = consistency_check(&daily_pnl_map, rules.consistency_cap_pct);

        if target_reached && min_days_met && consistency_met {
            passed = true;
            completion_day = Some(trade_day);
            break;
        }
    }

    let trading_days = trading_day_set.len() as u32;
    let min_days_met = trading_days >= rules.min_trading_days;
    let consistency_met = consistency_check(&daily_pnl_map, rules.consistency_cap_pct);
    let profit_pct = if starting_balance > Decimal::ZERO {
        (balance - starting_balance) / starting_balance * dec!(100)
    } else {
        Decimal::ZERO
    };

    ChallengeStageSimResult {
        stage_rules: rules.clone(),
        starting_balance,
        final_balance: balance,
        profit_pct,
        target_reached,
        daily_dd_breached,
        max_dd_breached,
        min_days_met,
        consistency_met,
        passed,
        days_to_target,
        trading_days,
        total_trades: trades_consumed,
        max_daily_dd_pct,
        max_total_dd_pct,
        breach_reason,
        trades_consumed,
        completion_day,
    }
}

fn consistency_check(daily_pnl_map: &HashMap<i64, Decimal>, cap_pct: Decimal) -> bool {
    if cap_pct <= Decimal::ZERO || daily_pnl_map.is_empty() {
        return true;
    }

    let total_profit: Decimal = daily_pnl_map
        .values()
        .filter(|pnl| **pnl > Decimal::ZERO)
        .sum();
    if total_profit <= Decimal::ZERO {
        return true;
    }

    let max_day_profit = daily_pnl_map
        .values()
        .copied()
        .max()
        .unwrap_or(Decimal::ZERO);
    let max_day_share = max_day_profit / total_profit * dec!(100);
    max_day_share <= cap_pct
}

fn end_of_day_high_watermark(
    trades: &[TradeResult],
    next_trade_day: i64,
    realized_balance: Decimal,
    overnight_equity_mode: OvernightEquityMode,
) -> Decimal {
    let rollover_ts = next_trade_day * 86_400;

    let overnight_pnl = match overnight_equity_mode {
        OvernightEquityMode::ClosedTradesOnly => Decimal::ZERO,
        OvernightEquityMode::FullTradePnlAtRollover => trades
            .iter()
            .filter(|trade| trade.opened_at < rollover_ts && trade.closed_at >= rollover_ts)
            .map(|trade| trade.pnl)
            .sum(),
    };

    realized_balance.max(realized_balance + overnight_pnl)
}

fn first_failure_reason(stage_results: &[ChallengeStageSimResult]) -> Option<String> {
    for stage in stage_results {
        if stage.passed {
            continue;
        }

        if let Some(reason) = &stage.breach_reason {
            return Some(reason.clone());
        }
        if !stage.target_reached {
            return Some(format!("{} target not reached", stage.stage_rules.name));
        }
        if !stage.min_days_met {
            return Some(format!(
                "{} minimum trading days not met",
                stage.stage_rules.name
            ));
        }
        if !stage.consistency_met {
            return Some(format!(
                "{} consistency rule failed",
                stage.stage_rules.name
            ));
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Batch simulation: run the same trades against multiple challenge types
// ---------------------------------------------------------------------------

pub fn simulate_challenges(
    trades: &[TradeResult],
    starting_balance: Decimal,
    rule_sets: &[ChallengeRules],
) -> Vec<ChallengeSimResult> {
    rule_sets
        .iter()
        .map(|rules| simulate_challenge(trades, starting_balance, rules))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gadarah_core::HeadId;

    fn trade(opened_at: i64, closed_at: i64, pnl: Decimal) -> TradeResult {
        TradeResult {
            head: HeadId::Momentum,
            pnl,
            r_multiple: if pnl >= Decimal::ZERO {
                dec!(2.0)
            } else {
                dec!(-1.0)
            },
            opened_at,
            closed_at,
            is_winner: pnl >= Decimal::ZERO,
        }
    }

    fn winning_trades(count: usize, per_trade_pnl: Decimal) -> Vec<TradeResult> {
        (0..count)
            .map(|i| {
                let opened_at = 1700000000 + (i as i64) * 86400;
                trade(opened_at, opened_at + 3600, per_trade_pnl)
            })
            .collect()
    }

    #[test]
    fn challenge_pass_scenario() {
        let trades = winning_trades(10, dec!(120));
        let result = simulate_challenge(&trades, dec!(10000), &ChallengeRules::ftmo_1step());

        assert!(result.passed);
        assert!(result.target_reached);
        assert!(!result.daily_dd_breached);
        assert!(!result.max_dd_breached);
        assert!(result.min_days_met);
        assert!(result.days_to_target.is_some());
        assert_eq!(result.stage_results.len(), 1);
    }

    #[test]
    fn challenge_dd_breach() {
        let trades = vec![
            trade(1700000000, 1700001800, dec!(-300)),
            trade(1700003600, 1700005400, dec!(-300)),
        ];

        let result = simulate_challenge(&trades, dec!(10000), &ChallengeRules::ftmo_1step());
        assert!(!result.passed);
        assert!(result.daily_dd_breached);
    }

    #[test]
    fn daily_dd_uses_fixed_challenge_size() {
        let trades = vec![
            trade(1700000000, 1700001800, dec!(1000)),
            trade(1700086400, 1700088200, dec!(-505)),
        ];

        let result = simulate_challenge(&trades, dec!(10000), &ChallengeRules::ftmo_1step());

        assert!(result.daily_dd_breached);
        assert_eq!(result.stage_results[0].total_trades, 2);
    }

    #[test]
    fn challenge_not_enough_days() {
        let trades = vec![
            trade(1700000000, 1700001800, dec!(600)),
            trade(1700086400, 1700088200, dec!(600)),
        ];

        let result = simulate_challenge(&trades, dec!(10000), &ChallengeRules::ftmo_1step());
        assert!(!result.passed);
        assert!(result.stage_results[0].target_reached);
        assert!(!result.min_days_met);
    }

    #[test]
    fn consistency_rule_fail() {
        let mut trades = winning_trades(5, dec!(30));
        trades[0].pnl = dec!(500);

        let result = simulate_challenge(&trades, dec!(10000), &ChallengeRules::two_step_pro());
        assert!(!result.consistency_met);
    }

    #[test]
    fn brightfunded_runs_two_stages() {
        let mut trades = winning_trades(10, dec!(200));
        for trade in trades.iter_mut().skip(5) {
            trade.pnl = dec!(120);
        }

        let result = simulate_challenge(
            &trades,
            dec!(10000),
            &ChallengeRules::brightfunded_evaluation(),
        );

        assert!(result.passed);
        assert_eq!(result.stage_results.len(), 2);
        assert_eq!(result.completed_stages, 2);
        assert_eq!(result.stage_results[0].stage_rules.name, "Challenge");
        assert_eq!(result.stage_results[1].stage_rules.name, "Verification");
    }

    #[test]
    fn brightfunded_day_count_requires_sixty_second_hold() {
        let trades = vec![
            trade(1700000000, 1700000030, dec!(300)),
            trade(1700086400, 1700086430, dec!(300)),
            trade(1700172800, 1700172830, dec!(300)),
            trade(1700259200, 1700259230, dec!(300)),
            trade(1700345600, 1700345720, dec!(300)),
        ];

        let result = simulate_challenge(
            &trades,
            dec!(10000),
            &ChallengeRules::brightfunded_evaluation(),
        );

        assert!(!result.passed);
        assert_eq!(result.stage_results[0].trading_days, 1);
        assert!(!result.stage_results[0].min_days_met);
    }

    #[test]
    fn overnight_equity_anchor_is_stricter_when_full_trade_pnl_is_staked() {
        let challenge = ChallengeStageRules {
            name: "Challenge".into(),
            target_pct: dec!(8.0),
            daily_dd_limit_pct: dec!(5.0),
            max_dd_limit_pct: dec!(10.0),
            min_trading_days: 1,
            trailing_dd: false,
            consistency_cap_pct: Decimal::ZERO,
            daily_dd_mode: DailyDrawdownMode::EndOfDayHighWatermark,
            overnight_equity_mode: OvernightEquityMode::FullTradePnlAtRollover,
            min_trade_duration_secs_for_day_count: 0,
        };
        let mut closed_only = challenge.clone();
        closed_only.overnight_equity_mode = OvernightEquityMode::ClosedTradesOnly;

        let day0 = 20_000 * 86_400;
        let trades = vec![
            // Open overnight winner: no day-1 realized balance change, but +$500 at rollover.
            trade(day0 + 3_600, day0 + 86_400 + 3_600, dec!(500)),
            // Day-2 realized giveback.
            trade(day0 + 86_400 + 7_200, day0 + 86_400 + 10_800, dec!(-600)),
        ];

        let strict = simulate_stage(&trades, dec!(10000), &challenge);
        let relaxed = simulate_stage(&trades, dec!(10000), &closed_only);

        assert!(strict.daily_dd_breached);
        assert!(!relaxed.daily_dd_breached);
        assert!(strict.max_daily_dd_pct > relaxed.max_daily_dd_pct);
    }

    #[test]
    fn batch_simulate() {
        let trades = winning_trades(10, dec!(120));
        let rules = vec![
            ChallengeRules::ftmo_1step(),
            ChallengeRules::brightfunded_evaluation(),
            ChallengeRules::two_step_pro(),
        ];
        let results = simulate_challenges(&trades, dec!(10000), &rules);
        assert_eq!(results.len(), 3);
    }
}
