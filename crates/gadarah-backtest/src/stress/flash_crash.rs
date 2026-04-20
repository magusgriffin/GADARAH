//! Flash crash: 500-pip move in 60 s plus a 5 s data dropout.

use gadarah_broker::MockConfig;
use gadarah_core::Bar;
use rust_decimal::Decimal;

use super::{shift_bar, StressScenario, StressedInput};

#[derive(Debug, Clone)]
pub struct FlashCrashScenario {
    pub at_bar: usize,
    pub crash_pips: Decimal,
    pub pip_size: Decimal,
    /// Number of bars to delete to simulate a data dropout.
    pub dropout_bars: usize,
}

impl StressScenario for FlashCrashScenario {
    fn name(&self) -> &'static str {
        "flash_crash"
    }

    fn apply(&self, mut bars: Vec<Bar>, mock: MockConfig) -> StressedInput {
        let offset = self.crash_pips * self.pip_size;
        if self.at_bar < bars.len() {
            // Single-bar cliff: only the crash bar's low + close collapse
            // (open/high stay — the cliff is intrabar).
            let crash_bar = &mut bars[self.at_bar];
            crash_bar.low -= offset;
            crash_bar.close -= offset;
            // Subsequent bars sit at the lower plateau.
            for bar in bars.iter_mut().skip(self.at_bar + 1) {
                shift_bar(bar, -offset);
            }
            let dropout_start = self.at_bar + 1;
            let dropout_end = (dropout_start + self.dropout_bars).min(bars.len());
            bars.drain(dropout_start..dropout_end);
        }
        StressedInput::from_scenario(self, bars, mock)
    }
}
