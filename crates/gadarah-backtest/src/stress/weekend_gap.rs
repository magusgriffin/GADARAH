//! Weekend gap: Sunday open is `gap_pips` away from Friday's close.

use gadarah_broker::MockConfig;
use gadarah_core::Bar;
use rust_decimal::Decimal;

use super::{shift_bar, StressScenario, StressedInput};

#[derive(Debug, Clone)]
pub struct WeekendGapScenario {
    /// Bar index at which the simulated Monday session begins.
    pub at_bar: usize,
    pub gap_pips: Decimal,
    pub pip_size: Decimal,
    /// True for gap-down, false for gap-up.
    pub gap_down: bool,
}

impl StressScenario for WeekendGapScenario {
    fn name(&self) -> &'static str {
        "weekend_gap"
    }

    fn apply(&self, mut bars: Vec<Bar>, mock: MockConfig) -> StressedInput {
        let raw = self.gap_pips * self.pip_size;
        let offset = if self.gap_down { -raw } else { raw };
        if self.at_bar < bars.len() {
            for bar in bars.iter_mut().skip(self.at_bar) {
                shift_bar(bar, offset);
            }
        }
        StressedInput::from_scenario(self, bars, mock)
    }
}
