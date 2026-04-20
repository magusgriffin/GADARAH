//! Slippage shock: baseline slippage budget inflated for the whole run.
//!
//! Mirrors a session where order routing is degraded — every fill pays extra.

use gadarah_broker::MockConfig;
use gadarah_core::Bar;
use rust_decimal::Decimal;

use super::{StressScenario, StressedInput};

#[derive(Debug, Clone)]
pub struct SlippageShockScenario {
    /// Baseline slippage is replaced with this value in pips.
    pub shock_pips: Decimal,
    /// Commission inflation multiplier.
    pub commission_multiplier: Decimal,
}

impl StressScenario for SlippageShockScenario {
    fn name(&self) -> &'static str {
        "slippage_shock"
    }

    fn apply(&self, bars: Vec<Bar>, mut mock: MockConfig) -> StressedInput {
        mock.slippage_pips = self.shock_pips;
        mock.commission_per_lot *= self.commission_multiplier;
        StressedInput::from_scenario(self, bars, mock)
    }
}
