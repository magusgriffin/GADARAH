use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use crate::types::{RiskPercent, SizingError};

#[derive(Debug, Clone, Copy)]
pub struct SizingInputs {
    pub risk_pct: RiskPercent,
    pub account_equity: Decimal,
    pub sl_distance_price: Decimal,
    pub pip_size: Decimal,
    pub pip_value_per_lot: Decimal,
    pub min_lot: Decimal,
    pub max_lot: Decimal,
    pub lot_step: Decimal,
}

// ---------------------------------------------------------------------------
// Kelly Criterion — adaptive risk sizing based on realized edge
// ---------------------------------------------------------------------------

/// Edge statistics needed to compute the Kelly fraction.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EdgeStats {
    /// Win rate as a fraction (e.g. 0.55 for 55%).
    pub win_rate: Decimal,
    /// Average winner / average loser (e.g. 1.8 means winners are 1.8x losers).
    pub payoff_ratio: Decimal,
    /// Total trades used to compute these stats.
    pub sample_size: u32,
}

/// Compute the Kelly-optimal risk fraction, then scale it down for safety.
///
/// Full Kelly is notoriously aggressive — most practitioners use "fractional Kelly"
/// (typically 25-50% of the full Kelly fraction).  The `kelly_fraction` parameter
/// controls this (0.25 = quarter-Kelly, the conservative default).
///
/// Returns `None` if the edge is negative, the sample size is too small (<20),
/// or the inputs are degenerate.  The result is a risk percentage clamped to
/// [`RiskPercent`] bounds (0.01–5.0%).
pub fn kelly_risk_pct(edge: &EdgeStats, kelly_fraction: Decimal) -> Option<RiskPercent> {
    // Need meaningful sample
    if edge.sample_size < 20 {
        return None;
    }
    if edge.win_rate <= Decimal::ZERO || edge.payoff_ratio <= Decimal::ZERO {
        return None;
    }

    let w = edge.win_rate;
    let r = edge.payoff_ratio;
    let loss_rate = Decimal::ONE - w;

    // Kelly formula: f* = W - (1 - W) / R = W - L / R
    // where W = win rate, L = loss rate, R = avg_win / avg_loss
    if r.is_zero() {
        return None;
    }
    let full_kelly = w - loss_rate / r;

    // Negative Kelly = negative edge, don't trade
    if full_kelly <= Decimal::ZERO {
        return None;
    }

    // Apply fractional Kelly and convert to percentage
    let risk_pct = full_kelly * kelly_fraction * dec!(100);

    Some(RiskPercent::clamped(risk_pct))
}

/// Calculate position size in lots using the exact formula from ULTIMATE.md 9.1.
///
/// Steps:
/// 1. Convert SL distance from price to pips
/// 2. Reject if SL < 2 pips (HYDRA Bug 3 prevention)
/// 3. Calculate risk in USD
/// 4. Derive raw lots from risk / (pips * pip_value)
/// 5. Step-round down to lot_step increments
/// 6. Clamp to [min_lot, max_lot]
/// 7. Sanity check: actual risk must not exceed input risk by >5%
pub fn calculate_lots(inputs: &SizingInputs) -> Result<Decimal, SizingError> {
    if inputs.account_equity <= Decimal::ZERO {
        return Err(SizingError::InvalidEquity {
            equity: inputs.account_equity,
        });
    }
    if inputs.pip_value_per_lot.is_zero() {
        return Err(SizingError::ZeroPipValue);
    }

    let sl_pips = inputs.sl_distance_price / inputs.pip_size;
    if sl_pips < dec!(2) {
        return Err(SizingError::SlDistanceTooSmall { pips: sl_pips });
    }

    let risk_usd = inputs.account_equity * inputs.risk_pct.as_fraction();
    let raw_lots = risk_usd / (sl_pips * inputs.pip_value_per_lot);

    // Step-round down: floor(raw / step) * step
    let stepped = (raw_lots / inputs.lot_step).floor() * inputs.lot_step;

    // Clamp to broker limits
    let final_lots = stepped.max(inputs.min_lot).min(inputs.max_lot);

    // Sanity check: actual risk must not exceed input risk by >5%
    let actual_risk = final_lots * sl_pips * inputs.pip_value_per_lot;
    let actual_risk_pct = actual_risk / inputs.account_equity * dec!(100);
    if actual_risk_pct > inputs.risk_pct.inner() * dec!(1.05) {
        return Err(SizingError::RoundingExceededRisk {
            computed: actual_risk_pct,
        });
    }

    Ok(final_lots)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kelly_positive_edge_returns_risk() {
        let edge = EdgeStats {
            win_rate: dec!(0.55),
            payoff_ratio: dec!(1.8),
            sample_size: 50,
        };
        let risk = kelly_risk_pct(&edge, dec!(0.25));
        assert!(risk.is_some());
        let r = risk.unwrap();
        assert!(r.inner() > Decimal::ZERO);
        assert!(r.inner() <= dec!(5.0));
    }

    #[test]
    fn kelly_negative_edge_returns_none() {
        let edge = EdgeStats {
            win_rate: dec!(0.30),
            payoff_ratio: dec!(1.0),
            sample_size: 50,
        };
        // Full Kelly = 0.30 - 0.70/1.0 = -0.40 → negative edge
        assert!(kelly_risk_pct(&edge, dec!(0.25)).is_none());
    }

    #[test]
    fn kelly_too_few_trades_returns_none() {
        let edge = EdgeStats {
            win_rate: dec!(0.60),
            payoff_ratio: dec!(2.0),
            sample_size: 10,
        };
        assert!(kelly_risk_pct(&edge, dec!(0.25)).is_none());
    }

    #[test]
    fn kelly_quarter_is_conservative() {
        // Use a moderate edge so neither fraction hits the 5% cap
        let edge = EdgeStats {
            win_rate: dec!(0.52),
            payoff_ratio: dec!(1.3),
            sample_size: 100,
        };
        let quarter = kelly_risk_pct(&edge, dec!(0.25)).unwrap();
        let half = kelly_risk_pct(&edge, dec!(0.50)).unwrap();
        assert!(quarter.inner() < half.inner());
    }

    #[test]
    fn kelly_result_is_clamped_to_risk_percent_bounds() {
        // Very high edge → full Kelly would be huge, but clamped to 5%
        let edge = EdgeStats {
            win_rate: dec!(0.80),
            payoff_ratio: dec!(3.0),
            sample_size: 100,
        };
        let risk = kelly_risk_pct(&edge, dec!(1.0)).unwrap();
        assert!(risk.inner() <= dec!(5.0));
    }
}
