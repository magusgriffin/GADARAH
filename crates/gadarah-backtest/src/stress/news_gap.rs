//! News gap: mid-bar 100-pip discontinuity, spread widens to ~10× for 60 s.

use gadarah_broker::MockConfig;
use gadarah_core::Bar;
use rust_decimal::Decimal;

use super::{shift_bar, StressScenario, StressedInput};

#[derive(Debug, Clone)]
pub struct NewsGapScenario {
    pub at_bar: usize,
    pub gap_pips: Decimal,
    pub pip_size: Decimal,
    /// Multiplier applied to the baseline spread for the whole run.
    pub spread_multiplier: Decimal,
}

impl StressScenario for NewsGapScenario {
    fn name(&self) -> &'static str {
        "news_gap"
    }

    fn apply(&self, mut bars: Vec<Bar>, mut mock: MockConfig) -> StressedInput {
        let offset = self.gap_pips * self.pip_size;
        if self.at_bar < bars.len() {
            for bar in bars.iter_mut().skip(self.at_bar) {
                shift_bar(bar, offset);
            }
        }
        mock.spread_pips *= self.spread_multiplier;
        StressedInput::from_scenario(self, bars, mock)
    }
}
