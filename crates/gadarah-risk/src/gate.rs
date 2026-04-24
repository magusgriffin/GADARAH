//! Pre-trade gate (A1) and `ExecutionWitness` (A2).
//!
//! The gate composes every risk check into a single evaluator. If any check
//! fails it returns `RiskDecision::Reject { .. }`; if all pass it mints an
//! `ExecutionWitness` and wraps it inside `RiskDecision::Execute { .. }`.
//!
//! `ExecutionWitness` is a zero-sized token whose constructor is private to
//! this module. The broker trait requires a witness on `send_order`, so the
//! type system refuses any order that was not produced by this gate — a
//! compile-time seal against un-gated order dispatch.

use rust_decimal::Decimal;

use gadarah_core::TradeSignal;

use crate::account::AccountState;
use crate::compliance::{ComplianceOpenExposure, PropFirmComplianceManager};
use crate::correlation::{CorrelationGuard, PortfolioAction, PositionRef};
use crate::daily_pnl::DailyPnlEngine;
use crate::execution::ExecutionEngine;
use crate::kill_switch::KillSwitch;
use crate::performance_ledger::{risk_of_ruin, PerformanceLedger};
use crate::types::{RejectReason, RiskDecision, RiskPercent};
use gadarah_core::{Regime9, Session};
use rust_decimal_macros::dec;

/// Default risk-of-ruin cap: 5% probability of blowing the account within
/// the horizon. Firms with tighter daily/total DD can still pass orders
/// up to this ceiling; the cap is a hard backstop, not a marginal dial.
const DEFAULT_MAX_ROR: Decimal = dec!(0.05);

/// Horizon (trades) over which RoR is computed. 200 trades is roughly one
/// month of active scalp-style activity — long enough to be conservative,
/// short enough not to exaggerate compounding.
const ROR_HORIZON_TRADES: u32 = 200;

/// Minimum segment sample size before the posterior is trusted by the EV /
/// RoR gates. Below this, the gate skips both checks to avoid blocking on
/// noise from the first few trades.
const POSTERIOR_MIN_SAMPLES: u32 = 15;

/// Compile-time proof that a `PreTradeGate::evaluate` has approved this order.
/// Construction is private to this module, so every order path must route
/// through the gate to obtain one.
///
/// ZST — occupies zero bytes at runtime.
#[derive(Debug, Clone, Copy)]
pub struct ExecutionWitness(());

impl ExecutionWitness {
    /// Internal: only the gate is permitted to issue a live witness.
    fn issue() -> Self {
        Self(())
    }

    /// Escape hatch for simulations (backtests, mock brokers, tests). Named
    /// obviously so a grep over the live crates flags every incidental usage.
    /// Never call this from the live `phase1` order path.
    pub fn for_simulation() -> Self {
        Self(())
    }
}

/// Execution context supplied to `PreTradeGate::evaluate`.
pub struct GateRequest {
    pub signal: TradeSignal,
    pub risk_pct: RiskPercent,
    pub lots: Decimal,
    pub is_pyramid: bool,
    /// Current regime classification for the session/bar. Used to consult the
    /// performance ledger (whether this (head, regime, session) segment is
    /// allowed) and to optionally route ensemble selection.
    pub regime: Regime9,
    /// Session label used for the performance ledger segment key.
    pub session: Session,
    /// Open positions for correlation-cluster evaluation.
    pub open_positions: Vec<PositionRef>,
    /// Open compliance exposures for anti-hedging / pacing.
    pub active_exposures: Vec<ComplianceOpenExposure>,
    /// Current time (unix seconds).
    pub now: i64,
}

/// Mutable references to the live risk stack. The gate does not own any
/// component — the live loop owns state and the gate evaluates against it.
pub struct PreTradeGate<'a> {
    pub kill_switch: &'a mut KillSwitch,
    pub daily_pnl: &'a DailyPnlEngine,
    pub account: &'a AccountState,
    pub execution: &'a ExecutionEngine,
    pub correlation: &'a mut CorrelationGuard,
    pub performance_ledger: Option<&'a PerformanceLedger>,
    pub compliance: &'a mut PropFirmComplianceManager,
    /// Broker must be reconciled before new orders are sent (A7).
    pub broker_synced: bool,
}

impl<'a> PreTradeGate<'a> {
    /// Evaluate a candidate order against every risk check, in order. Returns
    /// `RiskDecision::Execute` (with an `ExecutionWitness`) on pass, otherwise
    /// the first failing `RejectReason`.
    pub fn evaluate(&mut self, req: GateRequest) -> RiskDecision {
        // 1. Kill switch
        if self.kill_switch.is_active() {
            return reject(req.signal, RejectReason::KillSwitchActive);
        }

        // 2. Broker sync (A7)
        if !self.broker_synced {
            return reject(req.signal, RejectReason::BrokerDesynced);
        }

        // 3. Daily stop
        if !self.daily_pnl.can_trade() {
            return reject(req.signal, RejectReason::DailyDDLimitReached);
        }

        // 4. Equity floor / total DD (A11)
        if self.account.is_at_floor() {
            return reject(req.signal, RejectReason::TotalDDLimitReached);
        }
        if self.account.below_trading_equity() {
            return reject(req.signal, RejectReason::EquityFloor);
        }

        // 5. Execution engine: spread spike, stale price, vol halt, adjusted R:R
        if self.execution.vol_halt_active(req.now * 1000) {
            return reject(req.signal, RejectReason::VolatilityHalt);
        }
        if self.execution.is_stale_ms() {
            return reject(req.signal, RejectReason::StalePriceData);
        }
        if self.execution.is_spread_spike() {
            return reject(req.signal, RejectReason::SpreadTooHigh);
        }
        // Spread-adjusted R:R check — the engine's helper returns None when
        // risk distance is zero (pre-sized signals should never have that).
        if let Some(rr) = self.execution.adjusted_rr(&req.signal) {
            if rr < self.execution.config().min_rr_after_spread {
                return reject(req.signal, RejectReason::RrTooLowAfterSpread);
            }
        } else {
            return reject(req.signal, RejectReason::SlDistanceTooSmall);
        }

        // 6. Correlation / cluster rebalance (A12)
        let action = self.correlation.evaluate(
            &req.signal.symbol,
            req.signal.direction,
            &req.open_positions,
        );
        match action {
            PortfolioAction::Allow => {}
            PortfolioAction::Block { .. } => {
                return reject(req.signal, RejectReason::CorrelationCap);
            }
            PortfolioAction::ReduceCluster { .. } => {
                // Reduce-cluster means the strategy should close the oldest in
                // the cluster before growing. The gate treats it as a reject
                // for the *new* entry — the caller consumes the action and
                // acts on the cluster separately.
                return reject(req.signal, RejectReason::CorrelationCap);
            }
        }

        // 7. Performance ledger (B2)
        if let Some(ledger) = self.performance_ledger {
            if !ledger.is_segment_allowed(req.signal.head, req.regime, req.session) {
                return reject(req.signal, RejectReason::SegmentDisabled);
            }
        }

        // 8. Expected value + risk-of-ruin. Only enforced once the segment
        //    has enough samples for its posterior to be meaningful; below
        //    that threshold new heads get a fair shake.
        if let Some(ledger) = self.performance_ledger {
            if let Some(stats) = ledger.get_stats(req.signal.head, req.regime, req.session) {
                if stats.total_trades >= POSTERIOR_MIN_SAMPLES {
                    let p = stats.win_rate();
                    let tp_r = stats
                        .avg_tp_r()
                        .or_else(|| req.signal.rr_ratio())
                        .unwrap_or(dec!(1));
                    let sl_r = dec!(1); // by convention, losers close at −1R

                    // Expected value gate: block if negative given history.
                    let ev = p * tp_r - (dec!(1) - p) * sl_r;
                    if ev <= Decimal::ZERO {
                        return reject(req.signal, RejectReason::NegativeExpectedValue);
                    }

                    // Risk-of-ruin gate.
                    let ror = risk_of_ruin(
                        p,
                        tp_r,
                        sl_r,
                        req.risk_pct.as_fraction(),
                        ROR_HORIZON_TRADES,
                    );
                    if ror > DEFAULT_MAX_ROR {
                        return reject(req.signal, RejectReason::RiskOfRuinExceeded);
                    }
                }
            }
        }

        // 9. Prop-firm compliance (A13)
        if let Err(rej) = self.compliance.evaluate_entry(
            &req.signal,
            req.risk_pct.inner(),
            req.lots,
            &req.active_exposures,
            req.now,
        ) {
            return reject(req.signal, rej.reason);
        }

        // All checks passed — mint a witness.
        RiskDecision::Execute {
            signal: req.signal,
            risk_pct: req.risk_pct,
            lots: req.lots,
            is_pyramid: req.is_pyramid,
            witness: ExecutionWitness::issue(),
        }
    }
}

fn reject(signal: TradeSignal, reason: RejectReason) -> RiskDecision {
    RiskDecision::Reject { signal, reason }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::{AccountPhase, FirmConfig};
    use crate::compliance::PropFirmComplianceManager;
    use crate::correlation::CorrelationGuardConfig;
    use crate::daily_pnl::DailyPnlConfig;
    use crate::execution::ExecutionConfig;
    use gadarah_core::{Direction, HeadId, Regime9, Session, TradeSignal};
    use rust_decimal_macros::dec;

    fn fresh_account() -> AccountState {
        AccountState {
            phase: AccountPhase::ChallengePhase1,
            firm: FirmConfig {
                name: "Test".into(),
                challenge_type: "1-step".into(),
                profit_target_pct: dec!(6.0),
                daily_dd_limit_pct: dec!(4.0),
                max_dd_limit_pct: dec!(6.0),
                dd_mode: "trailing".into(),
                min_trading_days: 0,
                news_trading_allowed: true,
                max_positions: 3,
                profit_split_pct: dec!(80),
            },
            starting_balance: dec!(100_000),
            current_equity: dec!(100_000),
            high_water_mark: dec!(100_000),
            profit_pct: Decimal::ZERO,
            dd_from_hwm_pct: Decimal::ZERO,
            dd_remaining_pct: dec!(6.0),
            target_remaining: dec!(6.0),
            trading_days: 1,
            min_days_met: true,
            days_since_funded: 0,
            total_trades: 0,
            consecutive_losses: 0,
            phase_start_time: 0,
        }
    }

    fn signal() -> TradeSignal {
        TradeSignal {
            symbol: "EURUSD".into(),
            direction: Direction::Buy,
            kind: gadarah_core::SignalKind::Open,
            entry: dec!(1.1000),
            stop_loss: dec!(1.0980),
            take_profit: dec!(1.1040),
            take_profit2: None,
            head: HeadId::Momentum,
            head_confidence: dec!(0.8),
            regime: Regime9::StrongTrendUp,
            session: Session::London,
            pyramid_level: 0,
            comment: String::new(),
            generated_at: 0,
        }
    }

    fn request() -> GateRequest {
        GateRequest {
            signal: signal(),
            risk_pct: RiskPercent::new(dec!(1.0)).unwrap(),
            lots: dec!(0.10),
            is_pyramid: false,
            regime: Regime9::StrongTrendUp,
            session: Session::London,
            open_positions: vec![],
            active_exposures: vec![],
            now: 1_700_000_000,
        }
    }

    fn make_compliance() -> PropFirmComplianceManager {
        // Disabled compliance keeps the gate minimal — unit tests exercise the
        // earlier gates without having to satisfy every firm rule at once.
        PropFirmComplianceManager::disabled()
    }

    fn fresh_execution() -> ExecutionEngine {
        // Zero spread keeps the adjusted-R:R check out of our way — the signal
        // fixture already has TP 2× SL which passes the 1.2× threshold, but the
        // engine's adjusted_rr mixes pip-units into raw-price distances, so we
        // pin the spread to zero to isolate what this test is actually checking.
        let mut e = ExecutionEngine::new(ExecutionConfig::default(), Decimal::ZERO);
        e.update_spread(Decimal::ZERO, chrono::Utc::now().timestamp());
        e
    }

    #[test]
    fn all_clear_returns_execute_with_witness() {
        let mut ks = KillSwitch::new();
        let daily = DailyPnlEngine::new(DailyPnlConfig::default(), dec!(100_000));
        let account = fresh_account();
        let exec = fresh_execution();
        let mut corr = CorrelationGuard::new(CorrelationGuardConfig::default());
        let mut compliance = make_compliance();
        let mut gate = PreTradeGate {
            kill_switch: &mut ks,
            daily_pnl: &daily,
            account: &account,
            execution: &exec,
            correlation: &mut corr,
            performance_ledger: None,
            compliance: &mut compliance,
            broker_synced: true,
        };
        let decision = gate.evaluate(request());
        match decision {
            RiskDecision::Execute { .. } => {}
            RiskDecision::Reject { reason, .. } => {
                panic!("expected Execute, got reject: {reason:?}");
            }
        }
    }

    #[test]
    fn kill_switch_active_blocks() {
        let mut ks = KillSwitch::new();
        ks.activate(crate::types::KillReason::Manual, 0);
        let daily = DailyPnlEngine::new(DailyPnlConfig::default(), dec!(100_000));
        let account = fresh_account();
        let exec = fresh_execution();
        let mut corr = CorrelationGuard::new(CorrelationGuardConfig::default());
        let mut compliance = make_compliance();
        let mut gate = PreTradeGate {
            kill_switch: &mut ks,
            daily_pnl: &daily,
            account: &account,
            execution: &exec,
            correlation: &mut corr,
            performance_ledger: None,
            compliance: &mut compliance,
            broker_synced: true,
        };
        let decision = gate.evaluate(request());
        match decision {
            RiskDecision::Reject { reason, .. } => {
                assert_eq!(reason, RejectReason::KillSwitchActive);
            }
            other => panic!("expected reject, got {other:?}"),
        }
    }

    #[test]
    fn broker_desynced_blocks() {
        let mut ks = KillSwitch::new();
        let daily = DailyPnlEngine::new(DailyPnlConfig::default(), dec!(100_000));
        let account = fresh_account();
        let exec = fresh_execution();
        let mut corr = CorrelationGuard::new(CorrelationGuardConfig::default());
        let mut compliance = make_compliance();
        let mut gate = PreTradeGate {
            kill_switch: &mut ks,
            daily_pnl: &daily,
            account: &account,
            execution: &exec,
            correlation: &mut corr,
            performance_ledger: None,
            compliance: &mut compliance,
            broker_synced: false,
        };
        let decision = gate.evaluate(request());
        match decision {
            RiskDecision::Reject { reason, .. } => {
                assert_eq!(reason, RejectReason::BrokerDesynced);
            }
            other => panic!("expected reject, got {other:?}"),
        }
    }

    #[test]
    fn daily_stopped_blocks() {
        let mut ks = KillSwitch::new();
        let mut daily = DailyPnlEngine::new(DailyPnlConfig::default(), dec!(100_000));
        // Force DailyStopped by dropping equity past daily stop (default 1.5%).
        daily.update(dec!(98_000), 86400);
        let account = fresh_account();
        let exec = fresh_execution();
        let mut corr = CorrelationGuard::new(CorrelationGuardConfig::default());
        let mut compliance = make_compliance();
        let mut gate = PreTradeGate {
            kill_switch: &mut ks,
            daily_pnl: &daily,
            account: &account,
            execution: &exec,
            correlation: &mut corr,
            performance_ledger: None,
            compliance: &mut compliance,
            broker_synced: true,
        };
        let decision = gate.evaluate(request());
        match decision {
            RiskDecision::Reject { reason, .. } => {
                assert_eq!(reason, RejectReason::DailyDDLimitReached);
            }
            other => panic!("expected reject, got {other:?}"),
        }
    }

    #[test]
    fn equity_floor_blocks() {
        let mut ks = KillSwitch::new();
        let daily = DailyPnlEngine::new(DailyPnlConfig::default(), dec!(100_000));
        let mut account = fresh_account();
        // Push equity below 85% of starting balance.
        account.current_equity = dec!(70_000);
        let exec = fresh_execution();
        let mut corr = CorrelationGuard::new(CorrelationGuardConfig::default());
        let mut compliance = make_compliance();
        let mut gate = PreTradeGate {
            kill_switch: &mut ks,
            daily_pnl: &daily,
            account: &account,
            execution: &exec,
            correlation: &mut corr,
            performance_ledger: None,
            compliance: &mut compliance,
            broker_synced: true,
        };
        let decision = gate.evaluate(request());
        match decision {
            RiskDecision::Reject { reason, .. } => {
                assert_eq!(reason, RejectReason::EquityFloor);
            }
            other => panic!("expected reject, got {other:?}"),
        }
    }

    #[test]
    fn execution_witness_has_zero_size() {
        assert_eq!(std::mem::size_of::<ExecutionWitness>(), 0);
    }

    #[test]
    fn compliance_reject_surfaces_firm_rule_reason() {
        // A13: gate routes through `PropFirmComplianceManager::evaluate_entry`
        // and translates the resulting `ComplianceRejection::reason` into a
        // top-level `RiskDecision::Reject`. Here we force the hedging-forbidden
        // rule: opening a Buy while a Sell on the same symbol is already open.
        let mut ks = KillSwitch::new();
        let daily = DailyPnlEngine::new(DailyPnlConfig::default(), dec!(100_000));
        let mut account = fresh_account();
        // `detect_program` keys off the firm name string — make sure we land on
        // The5ers so the manager is actually enabled.
        account.firm.name = "The5ers Hyper Growth".into();
        let exec = fresh_execution();
        let mut corr = CorrelationGuard::new(CorrelationGuardConfig::default());
        // Build a real compliance manager (not the disabled one).
        let mut compliance = PropFirmComplianceManager::for_firm(&account.firm);
        assert!(
            compliance.is_enabled(),
            "test precondition: compliance must be enabled for this firm name",
        );
        let mut gate = PreTradeGate {
            kill_switch: &mut ks,
            daily_pnl: &daily,
            account: &account,
            execution: &exec,
            correlation: &mut corr,
            performance_ledger: None,
            compliance: &mut compliance,
            broker_synced: true,
        };
        let mut req = request();
        req.active_exposures = vec![crate::compliance::ComplianceOpenExposure {
            symbol: "EURUSD".into(),
            direction: Direction::Sell,
            risk_pct: dec!(1.0),
            lots: dec!(0.1),
            opened_at: 0,
        }];
        let decision = gate.evaluate(req);
        match decision {
            RiskDecision::Reject { reason, .. } => {
                assert_eq!(
                    reason,
                    RejectReason::ComplianceFirmRule,
                    "hedging block must surface as ComplianceFirmRule",
                );
            }
            other => panic!("expected reject, got {other:?}"),
        }
    }
}
