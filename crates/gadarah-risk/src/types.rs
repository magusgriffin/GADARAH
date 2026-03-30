use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use gadarah_core::TradeSignal;

// ---------------------------------------------------------------------------
// RiskPercent newtype — HYDRA Bug 1 prevention
// ---------------------------------------------------------------------------

/// A validated risk percentage clamped to [0.01, 5.0].
/// Prevents catastrophic sizing errors by ensuring risk is always within bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RiskPercent(Decimal);

impl RiskPercent {
    pub const MIN: Decimal = dec!(0.01);
    pub const MAX: Decimal = dec!(5.0);

    /// Create a new `RiskPercent`, returning an error if the value is outside [0.01, 5.0].
    pub fn new(pct: Decimal) -> Result<Self, RiskError> {
        if pct < Self::MIN || pct > Self::MAX {
            return Err(RiskError::InvalidRiskPercent {
                value: pct,
                min: Self::MIN,
                max: Self::MAX,
            });
        }
        Ok(Self(pct))
    }

    /// Create a `RiskPercent` by clamping the value to [0.01, 5.0].
    /// This never fails — out-of-range values are silently clamped.
    pub fn clamped(pct: Decimal) -> Self {
        Self(pct.max(Self::MIN).min(Self::MAX))
    }

    /// Return the inner percentage value (e.g. 1.0 for 1%).
    pub fn inner(&self) -> Decimal {
        self.0
    }

    /// Return the risk as a fraction (e.g. 0.01 for 1%).
    /// Used in dollar calculations: risk_usd = equity * as_fraction().
    pub fn as_fraction(&self) -> Decimal {
        self.0 / dec!(100)
    }
}

// ---------------------------------------------------------------------------
// RiskDecision
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum RiskDecision {
    Execute {
        signal: TradeSignal,
        risk_pct: RiskPercent,
        lots: Decimal,
        is_pyramid: bool,
    },
    Reject {
        signal: TradeSignal,
        reason: RejectReason,
    },
}

// ---------------------------------------------------------------------------
// RejectReason
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RejectReason {
    KillSwitchActive,
    DailyDDLimitReached,
    TotalDDLimitReached,
    SpreadTooHigh,
    VolatilityHalt,
    SlDistanceTooSmall,
    DailyTargetReached,
    SessionNotAllowed,
    MaxPositionsReached,
    EquityCurveFilter,
    ComplianceFirmRule,
    ConsecutiveLossHalt,
    DriftDetectorHalt,
    PerformanceLedgerBlock,
    RrTooLowAfterSpread,
    StalePriceData,
}

impl std::fmt::Display for RejectReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::KillSwitchActive => write!(f, "Kill switch is active"),
            Self::DailyDDLimitReached => write!(f, "Daily drawdown limit reached"),
            Self::TotalDDLimitReached => write!(f, "Total drawdown limit reached"),
            Self::SpreadTooHigh => write!(f, "Spread too high"),
            Self::VolatilityHalt => write!(f, "Volatility halt"),
            Self::SlDistanceTooSmall => write!(f, "Stop-loss distance too small (< 2 pips)"),
            Self::DailyTargetReached => write!(f, "Daily target already reached"),
            Self::SessionNotAllowed => write!(f, "Session not allowed for trading"),
            Self::MaxPositionsReached => write!(f, "Maximum positions reached"),
            Self::EquityCurveFilter => write!(f, "Equity curve filter blocked trade"),
            Self::ComplianceFirmRule => write!(f, "Prop firm compliance rule"),
            Self::ConsecutiveLossHalt => write!(f, "Consecutive loss halt (cooldown)"),
            Self::DriftDetectorHalt => write!(f, "Drift detector halt"),
            Self::PerformanceLedgerBlock => write!(f, "Performance ledger blocked segment"),
            Self::RrTooLowAfterSpread => write!(f, "R:R too low after spread adjustment"),
            Self::StalePriceData => write!(f, "Stale price data (> 2 seconds old)"),
        }
    }
}

// ---------------------------------------------------------------------------
// RiskError
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum RiskError {
    #[error("Invalid risk percent: {value} (must be between {min} and {max})")]
    InvalidRiskPercent {
        value: Decimal,
        min: Decimal,
        max: Decimal,
    },

    #[error("Sizing error: {0}")]
    Sizing(#[from] SizingError),
}

// ---------------------------------------------------------------------------
// SizingError
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum SizingError {
    #[error("Stop-loss distance too small: {pips} pips (minimum 2)")]
    SlDistanceTooSmall { pips: Decimal },

    #[error("Rounding exceeded risk by >5%: computed risk = {computed}%")]
    RoundingExceededRisk { computed: Decimal },

    #[error("Zero or negative equity: {equity}")]
    InvalidEquity { equity: Decimal },

    #[error("Zero pip value per lot")]
    ZeroPipValue,
}
