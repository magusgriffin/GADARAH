use rust_decimal::Decimal;
use rust_decimal_macros::dec;

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
