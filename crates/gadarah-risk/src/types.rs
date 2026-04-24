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
        /// Zero-sized proof that the `PreTradeGate` cleared this order. The
        /// broker trait requires a witness on `send_order`, so un-gated orders
        /// cannot compile.
        witness: crate::gate::ExecutionWitness,
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
    EquityFloor,
    MarginExceeded,
    SegmentDisabled,
    BrokerDesynced,
    CorrelationCap,
    RegimeConfidenceLow,
    MtfCounterTrend,
    PartialFillRejected,
    /// Expected value of this trade is non-positive given the latest
    /// segment posterior and the signal's own R:R.
    NegativeExpectedValue,
    /// Kelly-calibrated risk-of-ruin for the current sizing exceeds the
    /// firm-configured cap. Blocks orders that would be statistically
    /// likely to blow the account given the current edge.
    RiskOfRuinExceeded,
}

/// Stable, machine-readable reasons the kill switch was armed. Used in place of
/// free-form strings so that downstream code (GUI, audit log, metrics) can
/// route on a finite enum rather than substring-matching a sentence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KillReason {
    DailyDD,
    TotalDD,
    BrokerStale,
    VolHalt,
    ComplianceTrip,
    ConsecutiveLosses,
    DriftDetector,
    Manual,
}

impl std::fmt::Display for KillReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DailyDD => write!(f, "daily DD 95% trigger"),
            Self::TotalDD => write!(f, "total DD 95% trigger"),
            Self::BrokerStale => write!(f, "broker data stale"),
            Self::VolHalt => write!(f, "volatility halt"),
            Self::ComplianceTrip => write!(f, "prop-firm compliance trip"),
            Self::ConsecutiveLosses => write!(f, "consecutive-loss cooldown"),
            Self::DriftDetector => write!(f, "drift detector halt"),
            Self::Manual => write!(f, "manual"),
        }
    }
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
            Self::StalePriceData => write!(f, "Stale price data"),
            Self::EquityFloor => write!(f, "Equity at account floor"),
            Self::MarginExceeded => write!(f, "Margin utilization cap exceeded"),
            Self::SegmentDisabled => write!(f, "Performance ledger disabled this segment"),
            Self::BrokerDesynced => write!(f, "Broker not reconciled after reconnect"),
            Self::CorrelationCap => write!(f, "Correlated-exposure cap reached"),
            Self::RegimeConfidenceLow => write!(f, "Regime confidence below threshold"),
            Self::MtfCounterTrend => write!(f, "Counter to higher-timeframe bias"),
            Self::PartialFillRejected => write!(f, "Partial fill below minimum"),
            Self::NegativeExpectedValue => write!(f, "Negative expected value"),
            Self::RiskOfRuinExceeded => write!(f, "Risk-of-ruin above firm cap"),
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

    #[error("Margin utilization cap exceeded: required {required}, budget {budget}")]
    MarginExceeded { required: Decimal, budget: Decimal },

    #[error("Round-trip costs exceed risk budget: costs {costs}, budget {budget}")]
    CostsExceedRisk { costs: Decimal, budget: Decimal },
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn risk_percent_rejects_below_min() {
        assert!(RiskPercent::new(dec!(0.0)).is_err());
        assert!(RiskPercent::new(dec!(0.005)).is_err());
    }

    #[test]
    fn risk_percent_rejects_above_max() {
        assert!(RiskPercent::new(dec!(5.01)).is_err());
        assert!(RiskPercent::new(dec!(100.0)).is_err());
    }

    #[test]
    fn risk_percent_accepts_valid_bounds() {
        assert!(RiskPercent::new(dec!(0.01)).is_ok());
        assert!(RiskPercent::new(dec!(5.0)).is_ok());
        assert!(RiskPercent::new(dec!(1.0)).is_ok());
    }

    #[test]
    fn risk_percent_clamped_never_out_of_bounds() {
        let r = RiskPercent::clamped(dec!(999));
        assert_eq!(r.inner(), RiskPercent::MAX);
        let r = RiskPercent::clamped(dec!(-5));
        assert_eq!(r.inner(), RiskPercent::MIN);
    }

    #[test]
    fn risk_percent_as_fraction() {
        let r = RiskPercent::new(dec!(1.0)).unwrap();
        assert_eq!(r.as_fraction(), dec!(0.01));
    }
}
