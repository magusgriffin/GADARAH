use std::collections::{HashMap, HashSet};

use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::SeedableRng;
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TotalDrawdownMode {
    StaticFromStart,
    PercentOfPeak,
    FixedAmountFromPeakLockedAtStart,
    FixedAmountFromEndOfDayPeak,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DailyLimitAction {
    FailAccount,
    PauseDay,
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
    /// How max total drawdown is anchored.
    pub total_dd_mode: TotalDrawdownMode,
    /// Whether hitting the daily limit fails the account or only pauses the day.
    pub daily_limit_action: DailyLimitAction,
    /// Minimum realized daily profit needed for a day to count toward min-day rules.
    pub min_day_profit_pct_for_day_count: Decimal,
    /// Minimum hold time for a trade to count toward trading-day minimums.
    pub min_trade_duration_secs_for_day_count: i64,
}

impl ChallengeStageRules {
    pub fn daily_limit_label(&self) -> &'static str {
        match self.daily_limit_action {
            DailyLimitAction::FailAccount => "Daily DD",
            DailyLimitAction::PauseDay => "Daily Pause",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeRules {
    pub name: String,
    pub stages: Vec<ChallengeStageRules>,
}

impl ChallengeRules {
    /// FTMO 1-Step as reflected in FTMO Trading Objectives as of April 2026.
    ///
    /// - Profit target: 10%
    /// - Daily loss limit: 3% of initial balance, recalculated from the prior
    ///   end-of-day account balance at midnight CE(S)T
    /// - Max loss limit: 10% of initial balance, trailing from the highest
    ///   preceding end-of-day balance (can only increase, never decrease)
    /// - Best Day Rule: most profitable day must be <= 50% of positive-day profit
    /// - No prescribed minimum duration; in practice the Best Day Rule usually
    ///   makes one-day passes impossible and two-day passes rare
    /// - No time limit
    /// - News trading allowed during the evaluation
    pub fn ftmo_1step() -> Self {
        Self {
            name: "FTMO 1-Step".into(),
            stages: vec![ChallengeStageRules {
                name: "FTMO Challenge".into(),
                target_pct: dec!(10.0),
                daily_dd_limit_pct: dec!(3.0),
                max_dd_limit_pct: dec!(10.0),
                min_trading_days: 0,
                trailing_dd: false,
                consistency_cap_pct: dec!(50.0),
                daily_dd_mode: DailyDrawdownMode::DayStartBalance,
                overnight_equity_mode: OvernightEquityMode::ClosedTradesOnly,
                total_dd_mode: TotalDrawdownMode::FixedAmountFromEndOfDayPeak,
                daily_limit_action: DailyLimitAction::FailAccount,
                min_day_profit_pct_for_day_count: Decimal::ZERO,
                min_trade_duration_secs_for_day_count: 0,
            }],
        }
    }

    /// FTMO 2-Step (Challenge + Verification) as of April 2026.
    ///
    /// Stage 1 — FTMO Challenge:
    /// - Profit target: 10%
    /// - Daily loss limit: 5% of initial balance
    /// - Max loss limit: 10% of initial balance (static)
    /// - Minimum trading days: 4
    /// - No time limit
    ///
    /// Stage 2 — Verification:
    /// - Profit target: 5%
    /// - Same DD rules as Stage 1
    /// - Minimum trading days: 4
    /// - No time limit
    ///
    /// Daily loss = max(start-of-day balance, start-of-day equity) minus
    /// current equity (including unrealized). Breaching either daily or
    /// total limit fails the account immediately.
    pub fn ftmo_2step() -> Self {
        let stage = |name: &str, target_pct: Decimal| ChallengeStageRules {
            name: name.into(),
            target_pct,
            daily_dd_limit_pct: dec!(5.0),
            max_dd_limit_pct: dec!(10.0),
            min_trading_days: 4,
            trailing_dd: false,
            consistency_cap_pct: Decimal::ZERO,
            daily_dd_mode: DailyDrawdownMode::EndOfDayHighWatermark,
            overnight_equity_mode: OvernightEquityMode::FullTradePnlAtRollover,
            total_dd_mode: TotalDrawdownMode::StaticFromStart,
            daily_limit_action: DailyLimitAction::FailAccount,
            min_day_profit_pct_for_day_count: Decimal::ZERO,
            min_trade_duration_secs_for_day_count: 0,
        };

        Self {
            name: "FTMO 2-Step".into(),
            stages: vec![
                stage("FTMO Challenge", dec!(10.0)),
                stage("Verification", dec!(5.0)),
            ],
        }
    }

    /// The5ers Hyper Growth as of April 2, 2026.
    ///
    /// - One-step / instant-style challenge
    /// - Profit target: 10%
    /// - Daily pause: 3% from the higher of start-of-day balance or equity
    /// - Stopout level: 6% below the initial account size
    /// - No minimum trading days
    /// - First payout 14 days after funded
    pub fn the5ers_hyper_growth() -> Self {
        Self {
            name: "The5ers Hyper Growth".into(),
            stages: vec![ChallengeStageRules {
                name: "Level 1".into(),
                target_pct: dec!(10.0),
                daily_dd_limit_pct: dec!(3.0),
                max_dd_limit_pct: dec!(6.0),
                min_trading_days: 0,
                trailing_dd: false,
                consistency_cap_pct: Decimal::ZERO,
                daily_dd_mode: DailyDrawdownMode::EndOfDayHighWatermark,
                overnight_equity_mode: OvernightEquityMode::FullTradePnlAtRollover,
                total_dd_mode: TotalDrawdownMode::StaticFromStart,
                daily_limit_action: DailyLimitAction::PauseDay,
                min_day_profit_pct_for_day_count: Decimal::ZERO,
                min_trade_duration_secs_for_day_count: 0,
            }],
        }
    }

    /// FundingPips 1-Step as reflected in FundingPips terms as of April 2, 2026.
    ///
    /// - 10% target
    /// - 3% daily loss from the higher of start-of-day balance or equity
    /// - 6% max loss from initial balance
    /// - 3 minimum trading days
    pub fn fundingpips_1step() -> Self {
        Self {
            name: "FundingPips 1-Step".into(),
            stages: vec![ChallengeStageRules {
                name: "Student".into(),
                target_pct: dec!(10.0),
                daily_dd_limit_pct: dec!(3.0),
                max_dd_limit_pct: dec!(6.0),
                min_trading_days: 3,
                trailing_dd: false,
                consistency_cap_pct: Decimal::ZERO,
                daily_dd_mode: DailyDrawdownMode::EndOfDayHighWatermark,
                overnight_equity_mode: OvernightEquityMode::FullTradePnlAtRollover,
                total_dd_mode: TotalDrawdownMode::StaticFromStart,
                daily_limit_action: DailyLimitAction::FailAccount,
                min_day_profit_pct_for_day_count: Decimal::ZERO,
                min_trade_duration_secs_for_day_count: 0,
            }],
        }
    }

    /// FundingPips Zero instant-funded rules approximation from the terms.
    ///
    /// - No evaluation target
    /// - 3% daily loss from the higher of start-of-day balance or equity
    /// - 5% trailing loss that locks at start after +5%
    /// - 7 profitable days per 30-day cycle, each >= 0.25% of account balance
    pub fn fundingpips_zero() -> Self {
        Self {
            name: "FundingPips Zero".into(),
            stages: vec![ChallengeStageRules {
                name: "Master".into(),
                target_pct: Decimal::ZERO,
                daily_dd_limit_pct: dec!(3.0),
                max_dd_limit_pct: dec!(5.0),
                min_trading_days: 7,
                trailing_dd: true,
                consistency_cap_pct: Decimal::ZERO,
                daily_dd_mode: DailyDrawdownMode::EndOfDayHighWatermark,
                overnight_equity_mode: OvernightEquityMode::FullTradePnlAtRollover,
                total_dd_mode: TotalDrawdownMode::FixedAmountFromPeakLockedAtStart,
                daily_limit_action: DailyLimitAction::FailAccount,
                min_day_profit_pct_for_day_count: dec!(0.25),
                min_trade_duration_secs_for_day_count: 0,
            }],
        }
    }

    /// Alpha Capital Alpha One 1-step evaluation as of April 2, 2026.
    ///
    /// - Profit target: 10%
    /// - Daily loss: 4% of the higher of start-of-day balance or equity
    /// - Max loss: 6% trailing by fixed amount, locking at the starting balance
    /// - Minimum trading days: 1
    pub fn alpha_one() -> Self {
        Self {
            name: "Alpha Capital Alpha One".into(),
            stages: vec![ChallengeStageRules {
                name: "Assessment".into(),
                target_pct: dec!(10.0),
                daily_dd_limit_pct: dec!(4.0),
                max_dd_limit_pct: dec!(6.0),
                min_trading_days: 1,
                trailing_dd: true,
                consistency_cap_pct: Decimal::ZERO,
                daily_dd_mode: DailyDrawdownMode::EndOfDayHighWatermark,
                overnight_equity_mode: OvernightEquityMode::FullTradePnlAtRollover,
                total_dd_mode: TotalDrawdownMode::FixedAmountFromPeakLockedAtStart,
                daily_limit_action: DailyLimitAction::FailAccount,
                min_day_profit_pct_for_day_count: Decimal::ZERO,
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
            total_dd_mode: TotalDrawdownMode::StaticFromStart,
            daily_limit_action: DailyLimitAction::FailAccount,
            min_day_profit_pct_for_day_count: Decimal::ZERO,
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
                total_dd_mode: TotalDrawdownMode::StaticFromStart,
                daily_limit_action: DailyLimitAction::FailAccount,
                min_day_profit_pct_for_day_count: Decimal::ZERO,
                min_trade_duration_secs_for_day_count: 0,
            }],
        }
    }

    /// Blue Guardian instant funding style guardrail check.
    pub fn blue_guardian_instant() -> Self {
        Self {
            name: "Blue Guardian Instant".into(),
            stages: vec![ChallengeStageRules {
                name: "Instant".into(),
                target_pct: Decimal::ZERO,
                daily_dd_limit_pct: dec!(4.0),
                max_dd_limit_pct: dec!(6.0),
                min_trading_days: 0,
                trailing_dd: true,
                consistency_cap_pct: Decimal::ZERO,
                daily_dd_mode: DailyDrawdownMode::DayStartBalance,
                overnight_equity_mode: OvernightEquityMode::ClosedTradesOnly,
                total_dd_mode: TotalDrawdownMode::PercentOfPeak,
                daily_limit_action: DailyLimitAction::FailAccount,
                min_day_profit_pct_for_day_count: Decimal::ZERO,
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
    pub daily_limit_hit: bool,
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
    pub daily_limit_hit: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeBatchResult {
    pub runs: usize,
    pub passed_runs: usize,
    pub pass_rate: Decimal,
    pub dd_breach_rate: Decimal,
    pub avg_days_to_pass: Option<Decimal>,
    pub median_days_to_pass: Option<u32>,
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
    let daily_limit_hit = stage_results.iter().any(|stage| stage.daily_limit_hit);
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
        daily_limit_hit,
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
    let mut end_of_day_peak_balance = starting_balance;
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
    let mut daily_limit_hit = false;
    let mut max_dd_breached = false;
    let mut target_reached = false;
    let mut passed = false;
    let mut days_to_target = None;
    let mut breach_reason = None;
    let mut trades_consumed = 0usize;
    let mut completion_day = None;

    let daily_limit_amount = starting_balance * rules.daily_dd_limit_pct / dec!(100);

    let mut eligible_trading_day_set = HashSet::new();
    let mut daily_pnl_map: HashMap<i64, Decimal> = HashMap::new();

    let mut idx = 0usize;
    while idx < trades.len() {
        let trade = &trades[idx];
        let trade_day = utc_day(trade.closed_at);

        if trade_day != current_day {
            if rules.total_dd_mode == TotalDrawdownMode::FixedAmountFromEndOfDayPeak {
                end_of_day_peak_balance = end_of_day_peak_balance.max(balance);
            }
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
            eligible_trading_day_set.insert(trade_day);
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
        if daily_loss_amount >= daily_limit_amount {
            daily_limit_hit = true;
            if rules.daily_limit_action == DailyLimitAction::FailAccount {
                daily_dd_breached = true;
                breach_reason = Some(format!(
                    "{} daily DD {:.2}% >= {:.2}% on day {}",
                    rules.name, daily_dd_pct, rules.daily_dd_limit_pct, trade_day
                ));
            } else if breach_reason.is_none() {
                breach_reason = Some(format!(
                    "{} daily pause {:.2}% >= {:.2}% on day {}",
                    rules.name, daily_dd_pct, rules.daily_dd_limit_pct, trade_day
                ));
            }
        }

        let total_dd_peak = match rules.total_dd_mode {
            TotalDrawdownMode::FixedAmountFromEndOfDayPeak => end_of_day_peak_balance,
            _ => high_water_mark,
        };
        let total_dd_reference = total_dd_reference_balance(starting_balance, total_dd_peak, rules);
        let total_dd_floor = total_dd_breach_floor(starting_balance, total_dd_peak, rules);
        let total_dd = (total_dd_reference - balance).max(Decimal::ZERO);
        let total_dd_pct = if starting_balance > Decimal::ZERO {
            total_dd / starting_balance * dec!(100)
        } else {
            Decimal::ZERO
        };
        if total_dd_pct > max_total_dd_pct {
            max_total_dd_pct = total_dd_pct;
        }
        if balance <= total_dd_floor && !max_dd_breached {
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

        if rules.daily_limit_action == DailyLimitAction::PauseDay && daily_limit_hit {
            while idx + 1 < trades.len() && utc_day(trades[idx + 1].closed_at) == trade_day {
                idx += 1;
                trades_consumed = idx + 1;
            }
        }

        let profit_pct = if starting_balance > Decimal::ZERO {
            (balance - starting_balance) / starting_balance * dec!(100)
        } else {
            Decimal::ZERO
        };

        if profit_pct >= rules.target_pct && !target_reached {
            target_reached = true;
            days_to_target = Some(qualifying_trading_days(
                &eligible_trading_day_set,
                &daily_pnl_map,
                starting_balance,
                rules.min_day_profit_pct_for_day_count,
            ));
        }

        let min_days_met = qualifying_trading_days(
            &eligible_trading_day_set,
            &daily_pnl_map,
            starting_balance,
            rules.min_day_profit_pct_for_day_count,
        ) >= rules.min_trading_days;
        let consistency_met = consistency_check(&daily_pnl_map, rules.consistency_cap_pct);

        if target_reached && min_days_met && consistency_met {
            passed = true;
            completion_day = Some(trade_day);
            break;
        }

        idx += 1;
    }

    let trading_days = qualifying_trading_days(
        &eligible_trading_day_set,
        &daily_pnl_map,
        starting_balance,
        rules.min_day_profit_pct_for_day_count,
    );
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
        daily_limit_hit,
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

fn qualifying_trading_days(
    eligible_days: &HashSet<i64>,
    daily_pnl_map: &HashMap<i64, Decimal>,
    starting_balance: Decimal,
    min_day_profit_pct: Decimal,
) -> u32 {
    let min_day_profit = starting_balance * min_day_profit_pct / dec!(100);
    eligible_days
        .iter()
        .filter(|day| daily_pnl_map.get(day).copied().unwrap_or(Decimal::ZERO) >= min_day_profit)
        .count() as u32
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

fn total_dd_reference_balance(
    starting_balance: Decimal,
    high_water_mark: Decimal,
    rules: &ChallengeStageRules,
) -> Decimal {
    let limit_amount = drawdown_limit_amount(starting_balance, rules.max_dd_limit_pct);
    match rules.total_dd_mode {
        TotalDrawdownMode::StaticFromStart => starting_balance,
        TotalDrawdownMode::PercentOfPeak => high_water_mark,
        TotalDrawdownMode::FixedAmountFromPeakLockedAtStart => {
            high_water_mark.min(starting_balance + limit_amount)
        }
        TotalDrawdownMode::FixedAmountFromEndOfDayPeak => high_water_mark.max(starting_balance),
    }
}

fn total_dd_breach_floor(
    starting_balance: Decimal,
    high_water_mark: Decimal,
    rules: &ChallengeStageRules,
) -> Decimal {
    let limit_amount = drawdown_limit_amount(starting_balance, rules.max_dd_limit_pct);
    match rules.total_dd_mode {
        TotalDrawdownMode::StaticFromStart => starting_balance - limit_amount,
        TotalDrawdownMode::PercentOfPeak => {
            high_water_mark - (high_water_mark * rules.max_dd_limit_pct / dec!(100))
        }
        TotalDrawdownMode::FixedAmountFromPeakLockedAtStart => {
            (high_water_mark - limit_amount).min(starting_balance)
        }
        TotalDrawdownMode::FixedAmountFromEndOfDayPeak => high_water_mark - limit_amount,
    }
}

fn drawdown_limit_amount(starting_balance: Decimal, limit_pct: Decimal) -> Decimal {
    starting_balance * limit_pct / dec!(100)
}

fn first_failure_reason(stage_results: &[ChallengeStageSimResult]) -> Option<String> {
    for stage in stage_results {
        if stage.passed {
            continue;
        }

        if let Some(reason) = &stage.breach_reason {
            if stage.stage_rules.daily_limit_action == DailyLimitAction::PauseDay
                && stage.daily_limit_hit
                && !stage.max_dd_breached
                && !stage.daily_dd_breached
            {
                continue;
            }
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

pub fn simulate_challenge_batch(
    trades: &[TradeResult],
    starting_balance: Decimal,
    rules: &ChallengeRules,
    runs: usize,
    seed: u64,
) -> ChallengeBatchResult {
    if trades.is_empty() || runs == 0 {
        return ChallengeBatchResult {
            runs,
            passed_runs: 0,
            pass_rate: Decimal::ZERO,
            dd_breach_rate: Decimal::ZERO,
            avg_days_to_pass: None,
            median_days_to_pass: None,
        };
    }

    let mut rng = StdRng::seed_from_u64(seed);
    let mut shuffled = trades.to_vec();
    let mut passed_runs = 0usize;
    let mut dd_breaches = 0usize;
    let mut days_to_pass = Vec::new();

    for _ in 0..runs {
        shuffled.shuffle(&mut rng);
        let result = simulate_challenge(&shuffled, starting_balance, rules);
        if result.passed {
            passed_runs += 1;
            if let Some(days) = result.days_to_target {
                days_to_pass.push(days);
            }
        }
        if result.daily_dd_breached || result.max_dd_breached {
            dd_breaches += 1;
        }
    }

    days_to_pass.sort_unstable();
    let avg_days_to_pass = if days_to_pass.is_empty() {
        None
    } else {
        let total_days: u32 = days_to_pass.iter().sum();
        Some(Decimal::from(total_days) / Decimal::from(days_to_pass.len()))
    };
    let median_days_to_pass = if days_to_pass.is_empty() {
        None
    } else {
        Some(days_to_pass[days_to_pass.len() / 2])
    };

    ChallengeBatchResult {
        runs,
        passed_runs,
        pass_rate: Decimal::from(passed_runs) / Decimal::from(runs),
        dd_breach_rate: Decimal::from(dd_breaches) / Decimal::from(runs),
        avg_days_to_pass,
        median_days_to_pass,
    }
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
    fn the5ers_daily_pause_does_not_fail_the_account() {
        let day0 = 20_000 * 86_400;
        let trades = vec![
            trade(day0 + 3_600, day0 + 7_200, dec!(-310)),
            trade(day0 + 10_800, day0 + 14_400, dec!(-100)),
            trade(day0 + 86_400 + 3_600, day0 + 86_400 + 7_200, dec!(1400)),
        ];

        let result = simulate_challenge(
            &trades,
            dec!(10000),
            &ChallengeRules::the5ers_hyper_growth(),
        );

        assert!(result.passed);
        assert!(result.daily_limit_hit);
        assert!(!result.daily_dd_breached);
        assert!(!result.max_dd_breached);
    }

    #[test]
    fn fundingpips_zero_requires_profitable_days_threshold() {
        let day0 = 20_000 * 86_400;
        let mut trades = Vec::new();
        for offset in 0..7 {
            trades.push(trade(
                day0 + i64::from(offset) * 86_400 + 3_600,
                day0 + i64::from(offset) * 86_400 + 7_200,
                dec!(30),
            ));
        }

        let result = simulate_challenge(&trades, dec!(10000), &ChallengeRules::fundingpips_zero());

        assert!(result.passed);
        assert_eq!(result.stage_results[0].trading_days, 7);
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
    fn challenge_best_day_rule_blocks_imbalanced_two_day_pass() {
        let trades = vec![
            trade(1700000000, 1700001800, dec!(700)),
            trade(1700086400, 1700088200, dec!(300)),
        ];

        let result = simulate_challenge(&trades, dec!(10000), &ChallengeRules::ftmo_1step());
        assert!(!result.passed);
        assert!(result.stage_results[0].target_reached);
        assert!(result.min_days_met);
        assert!(!result.consistency_met);
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
    fn alpha_one_trailing_dd_locks_at_start_balance() {
        let day0 = 20_000 * 86_400;
        let trades = vec![
            trade(day0 + 3_600, day0 + 5_400, dec!(700)),
            trade(day0 + 86_400 + 3_600, day0 + 86_400 + 5_400, dec!(-710)),
        ];

        let result = simulate_challenge(&trades, dec!(10000), &ChallengeRules::alpha_one());

        assert!(!result.passed);
        assert!(result.max_dd_breached);
        assert_eq!(result.stage_results[0].stage_rules.name, "Assessment");
        assert_eq!(result.stage_results[0].max_total_dd_pct, dec!(6.10));
    }

    #[test]
    fn ftmo_1step_max_loss_only_reprices_after_day_close() {
        let day0 = 20_000 * 86_400;
        let trades = vec![
            trade(day0 + 3_600, day0 + 5_400, dec!(700)),
            trade(day0 + 7_200, day0 + 9_000, dec!(-1_200)),
            trade(day0 + 86_400 + 3_600, day0 + 86_400 + 5_400, dec!(-600)),
        ];

        let mut rules = ChallengeRules::ftmo_1step();
        rules.stages[0].daily_dd_limit_pct = dec!(100.0);
        let result = simulate_challenge(&trades, dec!(10000), &rules);

        assert!(!result.passed);
        assert!(result.max_dd_breached);
        assert_eq!(result.stage_results[0].total_trades, 3);
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
            total_dd_mode: TotalDrawdownMode::StaticFromStart,
            daily_limit_action: DailyLimitAction::FailAccount,
            min_day_profit_pct_for_day_count: Decimal::ZERO,
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
            ChallengeRules::the5ers_hyper_growth(),
            ChallengeRules::fundingpips_1step(),
            ChallengeRules::fundingpips_zero(),
            ChallengeRules::alpha_one(),
            ChallengeRules::brightfunded_evaluation(),
            ChallengeRules::two_step_pro(),
        ];
        let results = simulate_challenges(&trades, dec!(10000), &rules);
        assert_eq!(results.len(), 7);
    }

    #[test]
    fn challenge_batch_reports_pass_rate() {
        let trades = winning_trades(10, dec!(120));
        let batch =
            simulate_challenge_batch(&trades, dec!(10000), &ChallengeRules::ftmo_1step(), 25, 42);

        assert_eq!(batch.runs, 25);
        assert!(batch.pass_rate > Decimal::ZERO);
        assert!(batch.avg_days_to_pass.is_some());
    }
}
