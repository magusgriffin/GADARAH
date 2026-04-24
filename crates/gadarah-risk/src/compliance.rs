use std::collections::VecDeque;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use gadarah_core::{Direction, HeadId, TradeSignal};

use crate::{FirmConfig, RejectReason};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceBlackoutWindow {
    pub starts_at: i64,
    pub ends_at: i64,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceOpenExposure {
    pub symbol: String,
    pub direction: Direction,
    pub risk_pct: Decimal,
    pub lots: Decimal,
    pub opened_at: i64,
}

// ---------------------------------------------------------------------------
// Compliance config (shared across firms, with firm-specific defaults)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceConfig {
    pub min_seconds_between_entries: i64,
    pub max_entries_per_minute: usize,
    pub same_trade_idea_window_secs: i64,
    pub max_trade_idea_risk_pct: Decimal,
    pub recent_baseline_trades: usize,
    pub max_lot_jump_vs_median: Decimal,
    pub max_risk_jump_vs_median: Decimal,
    /// Maximum allowable spread in pips at entry time. None = no limit.
    /// FundingPips enforces ≤ 1.0 pip on all entries.
    pub max_spread_pips: Option<Decimal>,
}

/// Backwards-compatible alias for existing code referencing the old name.
pub type FundingPipsComplianceConfig = ComplianceConfig;

impl Default for ComplianceConfig {
    fn default() -> Self {
        Self {
            min_seconds_between_entries: 60,
            max_entries_per_minute: 2,
            same_trade_idea_window_secs: 300,
            max_trade_idea_risk_pct: dec!(3.0),
            recent_baseline_trades: 5,
            max_lot_jump_vs_median: dec!(2.5),
            max_risk_jump_vs_median: dec!(2.0),
            max_spread_pips: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Rejection type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComplianceRejection {
    pub reason: RejectReason,
    pub detail: String,
}

// ---------------------------------------------------------------------------
// Internal entry record
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct RecordedEntry {
    symbol: String,
    direction: Direction,
    risk_pct: Decimal,
    lots: Decimal,
    opened_at: i64,
}

// ---------------------------------------------------------------------------
// Detected firm program
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FirmProgram {
    // FundingPips variants
    FundingPipsOneStep,
    FundingPipsTwoStep,
    FundingPipsTwoStepPro,
    FundingPipsZero,
    // FTMO variants
    FtmoOneStep,
    FtmoTwoStep,
    // The5ers variants
    The5ersHyperGrowth,
    // Generic / unknown — basic guardrails only
    Generic,
    // Disabled — no compliance checks
    Disabled,
}

impl FirmProgram {
    fn label(&self) -> &'static str {
        match self {
            Self::FundingPipsOneStep => "FundingPips 1-Step",
            Self::FundingPipsTwoStep => "FundingPips 2-Step",
            Self::FundingPipsTwoStepPro => "FundingPips 2-Step Pro",
            Self::FundingPipsZero => "FundingPips Zero",
            Self::FtmoOneStep => "FTMO 1-Step",
            Self::FtmoTwoStep => "FTMO 2-Step",
            Self::The5ersHyperGrowth => "The5ers Hyper Growth",
            Self::Generic => "Generic",
            Self::Disabled => "Disabled",
        }
    }

    fn is_fundingpips(&self) -> bool {
        matches!(
            self,
            Self::FundingPipsOneStep
                | Self::FundingPipsTwoStep
                | Self::FundingPipsTwoStepPro
                | Self::FundingPipsZero
        )
    }

    fn is_ftmo(&self) -> bool {
        matches!(self, Self::FtmoOneStep | Self::FtmoTwoStep)
    }
}

// ---------------------------------------------------------------------------
// PropFirmComplianceManager — unified compliance for all supported firms
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PropFirmComplianceManager {
    program: FirmProgram,
    config: ComplianceConfig,
    blackout_windows: Vec<ComplianceBlackoutWindow>,
    recent_entries: VecDeque<RecordedEntry>,
    /// Per-firm override for whether news-head trading is permitted. Populated
    /// from `FirmConfig::news_trading_allowed` at construction. Scalp heads are
    /// still gated separately.
    news_trading_allowed: bool,
}

/// Backwards-compatible alias — existing code referencing FundingPipsComplianceManager
/// continues to work without changes.
pub type FundingPipsComplianceManager = PropFirmComplianceManager;

impl PropFirmComplianceManager {
    pub fn disabled() -> Self {
        Self {
            program: FirmProgram::Disabled,
            config: ComplianceConfig::default(),
            blackout_windows: Vec::new(),
            recent_entries: VecDeque::new(),
            news_trading_allowed: true,
        }
    }

    pub fn for_firm(firm: &FirmConfig) -> Self {
        let program = detect_program(firm);
        if program == FirmProgram::Disabled {
            return Self {
                news_trading_allowed: firm.news_trading_allowed,
                ..Self::disabled()
            };
        }

        let config = config_for_program(program);

        Self {
            program,
            config,
            blackout_windows: Vec::new(),
            recent_entries: VecDeque::new(),
            news_trading_allowed: firm.news_trading_allowed,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.program != FirmProgram::Disabled
    }

    pub fn program_label(&self) -> &'static str {
        self.program.label()
    }

    pub fn set_blackout_windows(&mut self, blackout_windows: Vec<ComplianceBlackoutWindow>) {
        self.blackout_windows = blackout_windows;
    }

    /// Returns the maximum allowed spread in pips for this firm, or None if unlimited.
    pub fn max_spread_pips(&self) -> Option<Decimal> {
        self.config.max_spread_pips
    }

    pub fn evaluate_entry(
        &mut self,
        signal: &TradeSignal,
        risk_pct: Decimal,
        lots: Decimal,
        active_exposures: &[ComplianceOpenExposure],
        timestamp: i64,
    ) -> Result<(), ComplianceRejection> {
        if !self.is_enabled() {
            return Ok(());
        }

        self.prune(timestamp);

        // ── Gate 1: Head blocking ───────────────────────────────────────
        // Tick-scalping heads are blocked across the board — every supported
        // firm flags rapid-fire entries as "gambling style" and denies payout.
        if matches!(signal.head, HeadId::ScalpM1 | HeadId::ScalpM5) {
            return Err(self.reject(format!(
                "{}: tick-scalping heads blocked to avoid pattern flags",
                self.program.label()
            )));
        }
        // News-head permission is per-firm and read from FirmConfig rather than
        // hardcoded: The5ers, FTMO, and the stricter firms all make their own
        // call. FundingPips remains blanket-blocked because their TOS forbids
        // news straddling outright regardless of the toml flag.
        if matches!(signal.head, HeadId::News) {
            if self.program.is_fundingpips() || !self.news_trading_allowed {
                return Err(self.reject(format!(
                    "{}: news-head trading disabled for this firm profile",
                    self.program.label()
                )));
            }
        }

        // ── Gate 2: Blackout windows (news embargo) ────────────────────
        // All firms: blackout windows block entries during high-impact events.
        // FundingPips: mandatory (TOS).
        // FTMO: enforced conservatively — FTMO reviews accounts that look like
        //   they're gambling on news spikes.
        // The5ers: enforced — daily 3% pause makes news-spike losses deadly.
        if let Some(window) = self
            .blackout_windows
            .iter()
            .find(|window| timestamp >= window.starts_at && timestamp <= window.ends_at)
        {
            return Err(self.reject(format!("news blackout active: {}", window.label)));
        }

        // ── Gate 3: Anti-hedging ───────────────────────────────────────
        // FundingPips: explicitly prohibited.
        // FTMO: hedging on same account is prohibited.
        // The5ers: not prohibited, but hedging on a 3% daily pause account
        //   is self-destructive. Blocked to protect the account.
        if active_exposures.iter().any(|exposure| {
            exposure.symbol == signal.symbol && exposure.direction != signal.direction
        }) {
            return Err(self.reject(format!(
                "{}: opposite-direction exposure would violate anti-hedging policy",
                self.program.label()
            )));
        }

        // ── Gate 4: Entry pacing (anti-HFT) ───────────────────────────
        // FundingPips: strict — known to flag rapid-fire entries.
        // FTMO: enforced — rapid entries trigger review.
        // The5ers: lighter touch but still enforced for safety.
        if self.recent_entries.back().is_some_and(|entry| {
            timestamp - entry.opened_at < self.config.min_seconds_between_entries
        }) {
            return Err(self.reject(format!(
                "{}: entry pacing too aggressive (min {}s between entries)",
                self.program.label(),
                self.config.min_seconds_between_entries
            )));
        }

        let entries_last_minute = self
            .recent_entries
            .iter()
            .filter(|entry| timestamp - entry.opened_at < 60)
            .count();
        if entries_last_minute >= self.config.max_entries_per_minute {
            return Err(self.reject(format!(
                "{}: too many entries in the last minute ({}/{})",
                self.program.label(),
                entries_last_minute,
                self.config.max_entries_per_minute
            )));
        }

        // ── Gate 5: Lot-size / risk-jump detection ─────────────────────
        // All firms: sudden lot-size jumps look like gambling to reviewers.
        // FTMO is particularly strict — they reject accounts that show
        // inconsistent sizing patterns.
        if self.recent_entries.len() >= self.config.recent_baseline_trades {
            let recent = self
                .recent_entries
                .iter()
                .rev()
                .take(self.config.recent_baseline_trades)
                .cloned()
                .collect::<Vec<_>>();
            let median_lots = median_decimal(recent.iter().map(|entry| entry.lots));
            let median_risk = median_decimal(recent.iter().map(|entry| entry.risk_pct));

            if median_lots > Decimal::ZERO
                && lots > median_lots * self.config.max_lot_jump_vs_median
            {
                return Err(self.reject(format!(
                    "{}: lot size jump too large ({lots} vs median {median_lots})",
                    self.program.label()
                )));
            }
            if median_risk > Decimal::ZERO
                && risk_pct > median_risk * self.config.max_risk_jump_vs_median
            {
                return Err(self.reject(format!(
                    "{}: risk jump too large ({risk_pct}% vs median {median_risk}%)",
                    self.program.label()
                )));
            }
        }

        // ── Gate 6: FundingPips Zero same-trade-idea cap ───────────────
        if self.program == FirmProgram::FundingPipsZero {
            let same_trade_idea_risk = active_exposures
                .iter()
                .filter(|exposure| {
                    exposure.symbol == signal.symbol
                        && exposure.direction == signal.direction
                        && timestamp - exposure.opened_at <= self.config.same_trade_idea_window_secs
                })
                .map(|exposure| exposure.risk_pct)
                .sum::<Decimal>()
                + self
                    .recent_entries
                    .iter()
                    .filter(|entry| {
                        entry.symbol == signal.symbol
                            && entry.direction == signal.direction
                            && timestamp - entry.opened_at
                                <= self.config.same_trade_idea_window_secs
                    })
                    .map(|entry| entry.risk_pct)
                    .sum::<Decimal>()
                + risk_pct;

            if same_trade_idea_risk > self.config.max_trade_idea_risk_pct {
                return Err(self.reject(format!(
                    "same trade idea risk {same_trade_idea_risk}% exceeds FundingPips Zero 3% cap"
                )));
            }
        }

        // ── Gate 7: FTMO consistency sizing guard ──────────────────────
        // FTMO funded accounts are reviewed for "consistent strategy."
        // If the bot suddenly changes risk per trade by more than 1.5x the
        // running average, block it. This is stricter than the generic
        // lot-jump gate above because FTMO has explicitly denied payouts
        // for inconsistent sizing.
        if self.program.is_ftmo() && self.recent_entries.len() >= 3 {
            let avg_risk: Decimal = self
                .recent_entries
                .iter()
                .rev()
                .take(self.config.recent_baseline_trades)
                .map(|e| e.risk_pct)
                .sum::<Decimal>()
                / Decimal::from(
                    self.recent_entries
                        .len()
                        .min(self.config.recent_baseline_trades),
                );
            if avg_risk > Decimal::ZERO && risk_pct > avg_risk * dec!(1.5) {
                return Err(self.reject(format!(
                    "FTMO consistency: risk {risk_pct}% exceeds 1.5x avg {avg_risk}%"
                )));
            }
        }

        Ok(())
    }

    pub fn record_entry(
        &mut self,
        signal: &TradeSignal,
        risk_pct: Decimal,
        lots: Decimal,
        timestamp: i64,
    ) {
        if !self.is_enabled() {
            return;
        }

        self.prune(timestamp);
        self.recent_entries.push_back(RecordedEntry {
            symbol: signal.symbol.clone(),
            direction: signal.direction,
            risk_pct,
            lots,
            opened_at: timestamp,
        });
    }

    fn prune(&mut self, timestamp: i64) {
        let keep_secs = self
            .config
            .same_trade_idea_window_secs
            .max(3600)
            .max(self.config.min_seconds_between_entries);
        while self
            .recent_entries
            .front()
            .is_some_and(|entry| timestamp - entry.opened_at > keep_secs)
        {
            self.recent_entries.pop_front();
        }
    }

    fn reject(&self, detail: impl Into<String>) -> ComplianceRejection {
        ComplianceRejection {
            reason: RejectReason::ComplianceFirmRule,
            detail: detail.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Program detection from FirmConfig
// ---------------------------------------------------------------------------

fn detect_program(firm: &FirmConfig) -> FirmProgram {
    let name = firm.name.to_lowercase();
    let challenge_type = firm.challenge_type.to_lowercase();

    // FundingPips
    if name.contains("fundingpips") || challenge_type.contains("fundingpips") {
        if name.contains("zero") || challenge_type.contains("zero") {
            return FirmProgram::FundingPipsZero;
        } else if name.contains("1-step")
            || name.contains("1 step")
            || challenge_type.contains("1step")
        {
            return FirmProgram::FundingPipsOneStep;
        } else if name.contains("2-step pro") || challenge_type.contains("2step_pro") {
            return FirmProgram::FundingPipsTwoStepPro;
        } else if name.contains("2-step") || challenge_type.contains("2step") {
            return FirmProgram::FundingPipsTwoStep;
        }
        return FirmProgram::FundingPipsOneStep; // default FP variant
    }

    // FTMO
    if name.contains("ftmo") || challenge_type.starts_with("ftmo") {
        if name.contains("2-step") || name.contains("2 step") || challenge_type.contains("2step") {
            return FirmProgram::FtmoTwoStep;
        }
        return FirmProgram::FtmoOneStep;
    }

    // The5ers
    if name.contains("the5ers") || name.contains("5ers") || challenge_type.contains("hyper_growth")
    {
        return FirmProgram::The5ersHyperGrowth;
    }

    // Known firms that should still get basic guardrails
    if name.contains("alpha capital")
        || name.contains("blue guardian")
        || name.contains("brightfunded")
    {
        return FirmProgram::Generic;
    }

    FirmProgram::Disabled
}

// ---------------------------------------------------------------------------
// Per-program compliance config tuning
// ---------------------------------------------------------------------------

fn config_for_program(program: FirmProgram) -> ComplianceConfig {
    match program {
        // FundingPips: strictest pacing — known to flag rapid entries
        // Also enforces max 1.0 pip spread per their TOS.
        FirmProgram::FundingPipsOneStep
        | FirmProgram::FundingPipsTwoStep
        | FirmProgram::FundingPipsTwoStepPro
        | FirmProgram::FundingPipsZero => ComplianceConfig {
            min_seconds_between_entries: 60,
            max_entries_per_minute: 2,
            same_trade_idea_window_secs: 300,
            max_trade_idea_risk_pct: dec!(3.0),
            recent_baseline_trades: 5,
            max_lot_jump_vs_median: dec!(2.5),
            max_risk_jump_vs_median: dec!(2.0),
            max_spread_pips: Some(dec!(1.0)),
        },

        // FTMO: tight lot-jump and risk-consistency rules.
        // FTMO reviews every funded account and denies payouts for
        // "inconsistent strategy." Tighter lot-jump threshold (2.0x vs 2.5x).
        FirmProgram::FtmoOneStep | FirmProgram::FtmoTwoStep => ComplianceConfig {
            min_seconds_between_entries: 45,
            max_entries_per_minute: 3,
            same_trade_idea_window_secs: 300,
            max_trade_idea_risk_pct: dec!(5.0), // FTMO doesn't have FP's 3% cap
            recent_baseline_trades: 5,
            max_lot_jump_vs_median: dec!(2.0),  // stricter than FP
            max_risk_jump_vs_median: dec!(1.5), // stricter — FTMO consistency
            max_spread_pips: None,
        },

        // The5ers: 3% daily pause is the binding constraint.
        // Slightly more relaxed pacing than FTMO/FP — The5ers doesn't flag
        // for rapid entries as aggressively.
        FirmProgram::The5ersHyperGrowth => ComplianceConfig {
            min_seconds_between_entries: 30,
            max_entries_per_minute: 3,
            same_trade_idea_window_secs: 300,
            max_trade_idea_risk_pct: dec!(5.0),
            recent_baseline_trades: 5,
            max_lot_jump_vs_median: dec!(2.5),
            max_risk_jump_vs_median: dec!(2.0),
            max_spread_pips: None,
        },

        // Generic / other firms: sensible defaults
        FirmProgram::Generic => ComplianceConfig::default(),

        FirmProgram::Disabled => ComplianceConfig::default(),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn median_decimal<I>(values: I) -> Decimal
where
    I: IntoIterator<Item = Decimal>,
{
    let mut values = values.into_iter().collect::<Vec<_>>();
    if values.is_empty() {
        return Decimal::ZERO;
    }
    values.sort();
    let mid = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[mid - 1] + values[mid]) / dec!(2)
    } else {
        values[mid]
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use gadarah_core::{Regime9, Session, SignalKind};

    fn fundingpips_zero_firm() -> FirmConfig {
        FirmConfig {
            name: "FundingPips Zero".into(),
            challenge_type: "fundingpips_zero".into(),
            profit_target_pct: Decimal::ZERO,
            daily_dd_limit_pct: dec!(3.0),
            max_dd_limit_pct: dec!(5.0),
            dd_mode: "trailing_locked_to_start".into(),
            min_trading_days: 7,
            news_trading_allowed: false,
            max_positions: 5,
            profit_split_pct: dec!(80.0),
        }
    }

    fn ftmo_1step_firm() -> FirmConfig {
        FirmConfig {
            name: "FTMO 1-Step".into(),
            challenge_type: "ftmo_1step".into(),
            profit_target_pct: dec!(10.0),
            daily_dd_limit_pct: dec!(3.0),
            max_dd_limit_pct: dec!(10.0),
            dd_mode: "eod_trailing".into(),
            min_trading_days: 0,
            news_trading_allowed: true,
            max_positions: 5,
            profit_split_pct: dec!(80.0),
        }
    }

    fn the5ers_firm() -> FirmConfig {
        FirmConfig {
            name: "The5ers - Hyper Growth".into(),
            challenge_type: "hyper_growth".into(),
            profit_target_pct: dec!(10.0),
            daily_dd_limit_pct: dec!(3.0),
            max_dd_limit_pct: dec!(6.0),
            dd_mode: "static".into(),
            min_trading_days: 0,
            news_trading_allowed: true,
            max_positions: 5,
            profit_split_pct: dec!(80.0),
        }
    }

    fn signal(direction: Direction, head: HeadId, generated_at: i64) -> TradeSignal {
        TradeSignal {
            symbol: "EURUSD".into(),
            direction,
            kind: SignalKind::Open,
            entry: dec!(1.1000),
            stop_loss: dec!(1.0980),
            take_profit: dec!(1.1040),
            take_profit2: None,
            head,
            head_confidence: dec!(0.8),
            regime: Regime9::WeakTrendUp,
            session: Session::London,
            pyramid_level: 0,
            comment: String::new(),
            generated_at,
        }
    }

    // ── FundingPips tests (existing, preserved) ─────────────────────────

    #[test]
    fn zero_rejects_same_trade_idea_above_three_percent() {
        let firm = fundingpips_zero_firm();
        let mut manager = PropFirmComplianceManager::for_firm(&firm);
        let signal = signal(Direction::Buy, HeadId::Momentum, 1_000);

        manager.record_entry(&signal, dec!(2.0), dec!(1.0), 1_000);
        let result = manager.evaluate_entry(&signal, dec!(1.1), dec!(1.0), &[], 1_200);

        assert!(result.is_err());
    }

    #[test]
    fn blackout_window_blocks_trade() {
        let firm = fundingpips_zero_firm();
        let mut manager = PropFirmComplianceManager::for_firm(&firm);
        manager.set_blackout_windows(vec![ComplianceBlackoutWindow {
            starts_at: 900,
            ends_at: 1_100,
            label: "CPI".into(),
        }]);

        let result = manager.evaluate_entry(
            &signal(Direction::Buy, HeadId::Momentum, 1_000),
            dec!(1.0),
            dec!(1.0),
            &[],
            1_000,
        );

        assert!(result.is_err());
    }

    #[test]
    fn opposite_direction_exposure_is_rejected() {
        let firm = fundingpips_zero_firm();
        let mut manager = PropFirmComplianceManager::for_firm(&firm);
        let exposure = ComplianceOpenExposure {
            symbol: "EURUSD".into(),
            direction: Direction::Sell,
            risk_pct: dec!(1.0),
            lots: dec!(1.0),
            opened_at: 1_000,
        };

        let result = manager.evaluate_entry(
            &signal(Direction::Buy, HeadId::Momentum, 1_500),
            dec!(1.0),
            dec!(1.0),
            &[exposure],
            1_500,
        );

        assert!(result.is_err());
    }

    #[test]
    fn scalping_heads_are_blocked() {
        let firm = fundingpips_zero_firm();
        let mut manager = PropFirmComplianceManager::for_firm(&firm);

        let result = manager.evaluate_entry(
            &signal(Direction::Buy, HeadId::ScalpM1, 1_000),
            dec!(1.0),
            dec!(1.0),
            &[],
            1_000,
        );

        assert!(result.is_err());
    }

    // ── FTMO tests ──────────────────────────────────────────────────────

    #[test]
    fn ftmo_detects_program_correctly() {
        let firm = ftmo_1step_firm();
        let manager = PropFirmComplianceManager::for_firm(&firm);
        assert!(manager.is_enabled());
        assert_eq!(manager.program_label(), "FTMO 1-Step");
    }

    #[test]
    fn ftmo_2step_detects_correctly() {
        let firm = FirmConfig {
            name: "FTMO 2-Step".into(),
            challenge_type: "ftmo_2step".into(),
            ..ftmo_1step_firm()
        };
        let manager = PropFirmComplianceManager::for_firm(&firm);
        assert_eq!(manager.program_label(), "FTMO 2-Step");
    }

    #[test]
    fn ftmo_blocks_scalp_heads_always() {
        let firm = ftmo_1step_firm();
        let mut manager = PropFirmComplianceManager::for_firm(&firm);

        for head in [HeadId::ScalpM1, HeadId::ScalpM5] {
            let result = manager.evaluate_entry(
                &signal(Direction::Buy, head, 1_000),
                dec!(1.0),
                dec!(1.0),
                &[],
                1_000,
            );
            assert!(result.is_err(), "FTMO should block scalp head {:?}", head);
        }
    }

    #[test]
    fn ftmo_news_head_follows_firm_flag() {
        // news_trading_allowed = true → news head is permitted.
        let firm_on = ftmo_1step_firm();
        assert!(firm_on.news_trading_allowed);
        let mut manager_on = PropFirmComplianceManager::for_firm(&firm_on);
        assert!(manager_on
            .evaluate_entry(
                &signal(Direction::Buy, HeadId::News, 1_000),
                dec!(1.0),
                dec!(1.0),
                &[],
                1_000,
            )
            .is_ok());

        // news_trading_allowed = false → news head is blocked.
        let mut firm_off = ftmo_1step_firm();
        firm_off.news_trading_allowed = false;
        let mut manager_off = PropFirmComplianceManager::for_firm(&firm_off);
        assert!(manager_off
            .evaluate_entry(
                &signal(Direction::Buy, HeadId::News, 1_000),
                dec!(1.0),
                dec!(1.0),
                &[],
                1_000,
            )
            .is_err());
    }

    #[test]
    fn ftmo_enforces_blackout_windows() {
        let firm = ftmo_1step_firm();
        let mut manager = PropFirmComplianceManager::for_firm(&firm);
        manager.set_blackout_windows(vec![ComplianceBlackoutWindow {
            starts_at: 900,
            ends_at: 1_100,
            label: "NFP".into(),
        }]);

        let result = manager.evaluate_entry(
            &signal(Direction::Buy, HeadId::Momentum, 1_000),
            dec!(1.0),
            dec!(1.0),
            &[],
            1_000,
        );
        assert!(result.is_err());
    }

    #[test]
    fn ftmo_rejects_risk_consistency_violation() {
        let firm = ftmo_1step_firm();
        let mut manager = PropFirmComplianceManager::for_firm(&firm);
        let sig = signal(Direction::Buy, HeadId::Momentum, 1_000);

        // Build baseline of consistent 0.5% risk
        for i in 0..5 {
            let t = 1_000 + (i * 120);
            manager.record_entry(&sig, dec!(0.5), dec!(0.1), t);
        }

        // Attempt 2x jump — should be blocked by FTMO consistency gate
        let result = manager.evaluate_entry(
            &signal(Direction::Buy, HeadId::Momentum, 2_000),
            dec!(1.0),
            dec!(0.1),
            &[],
            2_000,
        );
        assert!(result.is_err());
    }

    #[test]
    fn ftmo_anti_hedging_enforced() {
        let firm = ftmo_1step_firm();
        let mut manager = PropFirmComplianceManager::for_firm(&firm);
        let exposure = ComplianceOpenExposure {
            symbol: "EURUSD".into(),
            direction: Direction::Sell,
            risk_pct: dec!(1.0),
            lots: dec!(1.0),
            opened_at: 1_000,
        };

        let result = manager.evaluate_entry(
            &signal(Direction::Buy, HeadId::Momentum, 1_500),
            dec!(1.0),
            dec!(1.0),
            &[exposure],
            1_500,
        );
        assert!(result.is_err());
    }

    // ── The5ers tests ───────────────────────────────────────────────────

    #[test]
    fn the5ers_detects_program_correctly() {
        let firm = the5ers_firm();
        let manager = PropFirmComplianceManager::for_firm(&firm);
        assert!(manager.is_enabled());
        assert_eq!(manager.program_label(), "The5ers Hyper Growth");
    }

    #[test]
    fn the5ers_allows_news_head() {
        let firm = the5ers_firm();
        let mut manager = PropFirmComplianceManager::for_firm(&firm);

        // The5ers does not block News head (they allow news trading)
        let result = manager.evaluate_entry(
            &signal(Direction::Buy, HeadId::News, 1_000),
            dec!(1.0),
            dec!(1.0),
            &[],
            1_000,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn the5ers_blocks_scalp_heads() {
        let firm = the5ers_firm();
        let mut manager = PropFirmComplianceManager::for_firm(&firm);

        for head in [HeadId::ScalpM1, HeadId::ScalpM5] {
            let result = manager.evaluate_entry(
                &signal(Direction::Buy, head, 1_000),
                dec!(1.0),
                dec!(1.0),
                &[],
                1_000,
            );
            assert!(result.is_err(), "The5ers should block {:?}", head);
        }
    }

    #[test]
    fn the5ers_enforces_blackout_windows() {
        let firm = the5ers_firm();
        let mut manager = PropFirmComplianceManager::for_firm(&firm);
        manager.set_blackout_windows(vec![ComplianceBlackoutWindow {
            starts_at: 900,
            ends_at: 1_100,
            label: "FOMC".into(),
        }]);

        let result = manager.evaluate_entry(
            &signal(Direction::Buy, HeadId::Momentum, 1_000),
            dec!(1.0),
            dec!(1.0),
            &[],
            1_000,
        );
        assert!(result.is_err());
    }

    #[test]
    fn the5ers_anti_hedging_enforced() {
        let firm = the5ers_firm();
        let mut manager = PropFirmComplianceManager::for_firm(&firm);
        let exposure = ComplianceOpenExposure {
            symbol: "EURUSD".into(),
            direction: Direction::Sell,
            risk_pct: dec!(1.0),
            lots: dec!(1.0),
            opened_at: 1_000,
        };

        let result = manager.evaluate_entry(
            &signal(Direction::Buy, HeadId::Momentum, 1_500),
            dec!(1.0),
            dec!(1.0),
            &[exposure],
            1_500,
        );
        assert!(result.is_err());
    }

    // ── Generic firm test ───────────────────────────────────────────────

    #[test]
    fn generic_firm_gets_basic_guardrails() {
        let firm = FirmConfig {
            name: "Blue Guardian".into(),
            challenge_type: "instant".into(),
            profit_target_pct: Decimal::ZERO,
            daily_dd_limit_pct: dec!(4.0),
            max_dd_limit_pct: dec!(6.0),
            dd_mode: "trailing".into(),
            min_trading_days: 0,
            news_trading_allowed: true,
            max_positions: 5,
            profit_split_pct: dec!(80.0),
        };
        let manager = PropFirmComplianceManager::for_firm(&firm);
        assert!(manager.is_enabled());
        assert_eq!(manager.program_label(), "Generic");
    }
}
