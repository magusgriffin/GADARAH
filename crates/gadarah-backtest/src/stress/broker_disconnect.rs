//! Broker disconnect: 30-second reconnect window, partial fills on return.
//!
//! Models the live path exercised by workstreams A7 + A8: the data gap alone
//! is reproduced here (bars deleted).  Partial-fill semantics are validated
//! inside the broker unit tests; this scenario ensures the replay engine
//! survives the gap without diverging.

use gadarah_broker::MockConfig;
use gadarah_core::Bar;

use super::{StressScenario, StressedInput};

#[derive(Debug, Clone)]
pub struct BrokerDisconnectScenario {
    pub at_bar: usize,
    pub missing_bars: usize,
}

impl StressScenario for BrokerDisconnectScenario {
    fn name(&self) -> &'static str {
        "broker_disconnect"
    }

    fn apply(&self, mut bars: Vec<Bar>, mock: MockConfig) -> StressedInput {
        let start = self.at_bar.min(bars.len());
        let end = (start + self.missing_bars).min(bars.len());
        if start < end {
            bars.drain(start..end);
        }
        StressedInput::from_scenario(self, bars, mock)
    }
}
