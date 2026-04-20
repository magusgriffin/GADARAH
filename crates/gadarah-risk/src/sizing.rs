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
    /// Expected round-trip cost per lot in USD: (spread pips × pip_value) +
    /// commission + expected slippage. Deducted from the risk budget before
    /// lot derivation so a 0.50% risk trade actually loses ≤ 0.50% at SL.
    /// Zero = no deduction (legacy behavior for back-compat in old fixtures).
    pub cost_per_lot_usd: Decimal,
    /// Notional contract units per lot (100_000 for major FX, 100 for XAUUSD,
    /// etc.). Used together with `price` and `leverage` to compute margin.
    pub contract_size: Decimal,
    /// Current mid-price for margin calculation.
    pub price: Decimal,
    /// Account leverage (e.g. 30 for 30:1). Must be > 0 when `max_margin_util_pct > 0`.
    pub leverage: Decimal,
    /// Maximum fraction of equity that a single position may consume as
    /// initial margin. Range \[0, 1]. Zero disables the cap.
    pub max_margin_util_pct: Decimal,
}

impl SizingInputs {
    /// Build `SizingInputs` with cost/margin fields zeroed. Call-site escape
    /// hatch for tests and the few places that genuinely don't care about
    /// costs. Production paths should fill in real cost + margin numbers.
    pub fn bare(
        risk_pct: RiskPercent,
        account_equity: Decimal,
        sl_distance_price: Decimal,
        pip_size: Decimal,
        pip_value_per_lot: Decimal,
        min_lot: Decimal,
        max_lot: Decimal,
        lot_step: Decimal,
    ) -> Self {
        Self {
            risk_pct,
            account_equity,
            sl_distance_price,
            pip_size,
            pip_value_per_lot,
            min_lot,
            max_lot,
            lot_step,
            cost_per_lot_usd: Decimal::ZERO,
            contract_size: Decimal::ZERO,
            price: Decimal::ZERO,
            leverage: Decimal::ZERO,
            max_margin_util_pct: Decimal::ZERO,
        }
    }
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

    // Phase A5: deduct round-trip cost from the risk budget. The effective
    // loss at SL is `lots * (sl_pips * pip_value + cost_per_lot)`; we solve
    // for lots such that this loss stays inside the risk budget.
    let risk_budget_usd = inputs.account_equity * inputs.risk_pct.as_fraction();
    let loss_per_lot_at_sl = sl_pips * inputs.pip_value_per_lot + inputs.cost_per_lot_usd;
    if loss_per_lot_at_sl <= Decimal::ZERO {
        return Err(SizingError::ZeroPipValue);
    }

    // If costs already exceed the budget the trade is unprofitable even at
    // minimum size — refuse rather than silently clamp to min_lot.
    if inputs.cost_per_lot_usd >= risk_budget_usd {
        return Err(SizingError::CostsExceedRisk {
            costs: inputs.cost_per_lot_usd,
            budget: risk_budget_usd,
        });
    }

    let raw_lots = risk_budget_usd / loss_per_lot_at_sl;

    // Step-round down: floor(raw / step) * step
    let stepped = (raw_lots / inputs.lot_step).floor() * inputs.lot_step;

    // Clamp to broker limits first
    let mut final_lots = stepped.max(inputs.min_lot).min(inputs.max_lot);

    // Phase A4: margin utilization cap. Only engages when the user opted in
    // by setting `max_margin_util_pct > 0` and supplied the underlying
    // contract/price/leverage. Otherwise treat as "disabled".
    if inputs.max_margin_util_pct > Decimal::ZERO {
        if inputs.leverage <= Decimal::ZERO
            || inputs.contract_size <= Decimal::ZERO
            || inputs.price <= Decimal::ZERO
        {
            // Caller asked for a cap but gave us unusable inputs. Refuse.
            return Err(SizingError::MarginExceeded {
                required: Decimal::ZERO,
                budget: Decimal::ZERO,
            });
        }

        let margin_per_lot = inputs.contract_size * inputs.price / inputs.leverage;
        let margin_budget = inputs.account_equity * inputs.max_margin_util_pct;
        let margin_required = final_lots * margin_per_lot;

        if margin_required > margin_budget {
            let max_lots_by_margin = margin_budget / margin_per_lot;
            let capped = (max_lots_by_margin / inputs.lot_step).floor() * inputs.lot_step;

            if capped < inputs.min_lot {
                return Err(SizingError::MarginExceeded {
                    required: margin_required,
                    budget: margin_budget,
                });
            }

            final_lots = capped;
        }
    }

    // Sanity check: actual risk (SL loss only, excluding costs) must not
    // exceed input risk by >5%. Costs are deducted from the budget up-front,
    // so pure SL risk stays below the nominal percent — this check catches
    // rounding drift, not cost accounting.
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

    fn bare_inputs(risk_pct: Decimal, sl_distance: Decimal, equity: Decimal) -> SizingInputs {
        SizingInputs::bare(
            RiskPercent::new(risk_pct).unwrap(),
            equity,
            sl_distance,
            dec!(0.0001),
            dec!(10.0),
            dec!(0.01),
            dec!(50.0),
            dec!(0.01),
        )
    }

    #[test]
    fn bare_inputs_produce_legacy_behavior() {
        // 1% of $10_000 = $100 risk. SL=20 pips * $10/pip = $200 per lot.
        // → 0.50 lots, step-rounded.
        let lots = calculate_lots(&bare_inputs(dec!(1.0), dec!(0.0020), dec!(10_000))).unwrap();
        assert_eq!(lots, dec!(0.50));
    }

    #[test]
    fn costs_reduce_lots_vs_no_costs() {
        let zero_cost = calculate_lots(&bare_inputs(dec!(1.0), dec!(0.0020), dec!(10_000))).unwrap();
        let mut with_cost = bare_inputs(dec!(1.0), dec!(0.0020), dec!(10_000));
        with_cost.cost_per_lot_usd = dec!(30); // $30/lot round-trip
        let after = calculate_lots(&with_cost).unwrap();
        assert!(after < zero_cost, "costs must reduce lot count");
    }

    #[test]
    fn costs_that_consume_budget_error() {
        let mut inputs = bare_inputs(dec!(0.1), dec!(0.0020), dec!(10_000));
        // 0.1% of $10_000 = $10 budget. A $50/lot cost blows past it.
        inputs.cost_per_lot_usd = dec!(50);
        matches!(
            calculate_lots(&inputs),
            Err(SizingError::CostsExceedRisk { .. })
        );
    }

    #[test]
    fn margin_cap_clamps_lots_when_enabled() {
        // Without cap: 2% of 100k at 20 pips → 10 lots
        let mut inputs = bare_inputs(dec!(2.0), dec!(0.0020), dec!(100_000));
        inputs.contract_size = dec!(100_000);
        inputs.price = dec!(1.10);
        inputs.leverage = dec!(30);
        // 50% margin budget = $50k. margin_per_lot = 100k*1.10/30 = $3666.67
        // → max lots by margin = 50000 / 3666.67 ≈ 13.6 → step rounds to 13.63
        // Disabled case should be 10 (risk-bound), so cap is not the binding
        // constraint here — invert: force max_margin_util_pct tiny.
        inputs.max_margin_util_pct = dec!(0.05); // $5k budget → ~1.36 lots
        let lots = calculate_lots(&inputs).unwrap();
        assert!(lots <= dec!(1.37) && lots >= dec!(1.30), "lots = {lots}");
    }

    #[test]
    fn margin_cap_rejects_when_below_min_lot() {
        let mut inputs = bare_inputs(dec!(2.0), dec!(0.0020), dec!(100_000));
        inputs.contract_size = dec!(100_000);
        inputs.price = dec!(1.10);
        inputs.leverage = dec!(30);
        inputs.max_margin_util_pct = dec!(0.00001); // tiny budget
        inputs.min_lot = dec!(0.01);
        assert!(matches!(
            calculate_lots(&inputs),
            Err(SizingError::MarginExceeded { .. })
        ));
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
