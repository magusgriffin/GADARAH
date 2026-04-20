//! Bar-level stress scenarios.
//!
//! Each scenario mutates a baseline `Vec<Bar>` (and/or a `MockConfig`) to
//! simulate a specific adverse market condition:
//!
//! - `news_gap`         — 100-pip gap mid-bar, spread widens 10× for ~60s.
//! - `flash_crash`      — 500-pip move inside a minute + 5s data dropout.
//! - `slippage_shock`   — session-long slippage distribution centered at 5 pips.
//! - `broker_disconnect`— 30s gap with partial fill on reconnect.
//! - `weekend_gap`      — Sunday open is 200 pips off last Friday close.
//!
//! Callers run `run_replay` on the mutated bars and assert DD stays inside
//! firm limits.  The library does not run the replay itself — it only shapes
//! the input so the same replayer is exercised consistently.

pub mod broker_disconnect;
pub mod flash_crash;
pub mod news_gap;
pub mod slippage_shock;
pub mod weekend_gap;

use gadarah_broker::MockConfig;
use gadarah_core::Bar;
use rust_decimal::Decimal;

pub use broker_disconnect::BrokerDisconnectScenario;
pub use flash_crash::FlashCrashScenario;
pub use news_gap::NewsGapScenario;
pub use slippage_shock::SlippageShockScenario;
pub use weekend_gap::WeekendGapScenario;

/// One shaped baseline ready for `run_replay`.
#[derive(Debug, Clone)]
pub struct StressedInput {
    pub scenario: &'static str,
    pub bars: Vec<Bar>,
    pub mock_config: MockConfig,
}

impl StressedInput {
    /// Construct from a scenario, pulling the scenario name so each `apply()`
    /// impl doesn't have to repeat its own name literal.
    pub fn from_scenario<S: StressScenario + ?Sized>(
        scenario: &S,
        bars: Vec<Bar>,
        mock_config: MockConfig,
    ) -> Self {
        Self {
            scenario: scenario.name(),
            bars,
            mock_config,
        }
    }
}

/// Anything that can shape a baseline into a stressed input.
pub trait StressScenario {
    fn name(&self) -> &'static str;
    fn apply(&self, bars: Vec<Bar>, mock: MockConfig) -> StressedInput;
}

/// Helper: move a bar's OHLC by `offset` (price units, not pips).
pub fn shift_bar(bar: &mut Bar, offset: Decimal) {
    bar.open += offset;
    bar.high += offset;
    bar.low += offset;
    bar.close += offset;
}

#[cfg(test)]
mod tests {
    use super::*;
    use gadarah_core::Timeframe;
    use rust_decimal_macros::dec;

    fn baseline() -> Vec<Bar> {
        (0..100)
            .map(|i| Bar {
                open: dec!(1.1000),
                high: dec!(1.1005),
                low: dec!(1.0995),
                close: dec!(1.1000),
                volume: 100,
                timestamp: i,
                timeframe: Timeframe::M5,
            })
            .collect()
    }

    #[test]
    fn news_gap_shifts_tail_bars_but_not_head() {
        let scen = NewsGapScenario {
            at_bar: 50,
            gap_pips: dec!(100),
            pip_size: dec!(0.0001),
            spread_multiplier: dec!(10),
        };
        let out = scen.apply(baseline(), MockConfig::default());
        assert_eq!(out.bars[0].close, dec!(1.1000));
        assert_eq!(out.bars[49].close, dec!(1.1000));
        assert_eq!(out.bars[50].close, dec!(1.1100));
        assert!(out.mock_config.spread_pips > MockConfig::default().spread_pips);
    }

    #[test]
    fn flash_crash_drops_price_and_removes_bars() {
        let scen = FlashCrashScenario {
            at_bar: 20,
            crash_pips: dec!(500),
            pip_size: dec!(0.0001),
            dropout_bars: 5,
        };
        let initial_len = baseline().len();
        let out = scen.apply(baseline(), MockConfig::default());
        assert_eq!(out.bars.len(), initial_len - 5);
        assert!(out.bars[20].close < dec!(1.10));
    }

    #[test]
    fn slippage_shock_inflates_cost_config_only() {
        let initial_len = baseline().len();
        let scen = SlippageShockScenario {
            shock_pips: dec!(5),
            commission_multiplier: dec!(2),
        };
        let out = scen.apply(baseline(), MockConfig::default());
        assert_eq!(out.bars.len(), initial_len);
        assert_eq!(out.mock_config.slippage_pips, dec!(5));
        assert!(out.mock_config.commission_per_lot > MockConfig::default().commission_per_lot);
    }

    #[test]
    fn broker_disconnect_removes_expected_window() {
        let scen = BrokerDisconnectScenario {
            at_bar: 10,
            missing_bars: 30,
        };
        let out = scen.apply(baseline(), MockConfig::default());
        assert_eq!(out.bars.len(), 70);
    }

    #[test]
    fn weekend_gap_down_subtracts_offset() {
        let scen = WeekendGapScenario {
            at_bar: 0,
            gap_pips: dec!(200),
            pip_size: dec!(0.0001),
            gap_down: true,
        };
        let out = scen.apply(baseline(), MockConfig::default());
        assert_eq!(out.bars[0].close, dec!(1.0800));
    }
}
